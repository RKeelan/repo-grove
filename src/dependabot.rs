use anyhow::Result;
use serde::Deserialize;

use crate::config::Config;
use crate::gh;
use crate::index;

#[derive(Debug, Default)]
struct Summary {
    merged: u32,
    rebase_requested: u32,
    unstable: u32,
    already_closed: u32,
    skipped: u32,
}

#[derive(Deserialize)]
struct GhPrDetail {
    state: String,
    mergeable: String,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: String,
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<Vec<StatusCheck>>,
}

#[derive(Deserialize)]
struct StatusCheck {
    name: String,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct GhPr {
    number: u64,
    title: String,
    author: gh::GhAuthor,
}

pub fn run_merge(config: &Config, repo_filter: Option<&str>) -> Result<()> {
    let idx = index::load(config)?;
    let repos = index::filter_repos(&idx, repo_filter, "Index is empty — nothing to process.")?;

    if repos.is_empty() {
        return Ok(());
    }

    let mut summary = Summary::default();

    for repo in &repos {
        if repo.is_archived {
            continue;
        }

        let prs = match fetch_dependabot_prs(&repo.full_name) {
            Ok(prs) => prs,
            Err(e) => {
                eprintln!("Warning: failed to list PRs for {}: {e}", repo.full_name);
                continue;
            }
        };

        for pr in prs {
            eprintln!("{}#{} {}", repo.full_name, pr.number, pr.title);
            handle_pr(&repo.full_name, pr.number, &mut summary);
        }
    }

    eprintln!();
    eprintln!(
        "Summary: merged={} rebase_requested={} unstable={} already_closed={} skipped={}",
        summary.merged,
        summary.rebase_requested,
        summary.unstable,
        summary.already_closed,
        summary.skipped,
    );

    Ok(())
}

fn fetch_dependabot_prs(full_name: &str) -> Result<Vec<GhPr>> {
    let prs: Vec<GhPr> = gh::json(&[
        "pr",
        "list",
        "--repo",
        full_name,
        "--state",
        "open",
        "--json",
        "number,title,author",
    ])?;

    Ok(prs
        .into_iter()
        .filter(|pr| pr.author.is_dependabot())
        .collect())
}

fn fetch_pr_detail(full_name: &str, number: &str) -> Result<GhPrDetail> {
    gh::json(&[
        "pr",
        "view",
        number,
        "--repo",
        full_name,
        "--json",
        "state,mergeable,mergeStateStatus,statusCheckRollup",
    ])
}

fn handle_pr(full_name: &str, number: u64, summary: &mut Summary) {
    let num_str = number.to_string();

    let detail = match fetch_pr_detail(full_name, &num_str) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  Warning: could not fetch PR details: {e}");
            summary.skipped += 1;
            return;
        }
    };

    let state = detail.state.as_str();
    let mergeable = detail.mergeable.as_str();
    let merge_state = detail.merge_state_status.as_str();

    if state == "MERGED" || state == "CLOSED" {
        eprintln!("  → already {state}, skipping");
        summary.already_closed += 1;
        return;
    }

    if merge_state == "CLEAN" && mergeable == "MERGEABLE" {
        match gh::run(&["pr", "merge", &num_str, "--repo", full_name, "--squash"]) {
            Ok(output) if output.success => {
                eprintln!("  → merged (squash)");
                summary.merged += 1;
            }
            Ok(output) => {
                eprintln!("  → merge failed: {}", output.stderr.trim());
                summary.skipped += 1;
            }
            Err(e) => {
                eprintln!("  → merge failed: {e}");
                summary.skipped += 1;
            }
        }
        return;
    }

    if mergeable == "CONFLICTING" || merge_state == "DIRTY" {
        request_rebase(full_name, &num_str);
        eprintln!("  → requested rebase (conflicting)");
        summary.rebase_requested += 1;
        return;
    }

    if merge_state == "UNSTABLE" {
        let failing = failing_check_names(&detail);
        eprintln!(
            "  → unstable; failing checks: {}",
            if failing.is_empty() {
                "unknown".to_string()
            } else {
                failing.join(", ")
            }
        );
        request_rebase(full_name, &num_str);
        eprintln!("  → requested rebase (may be behind)");
        summary.unstable += 1;
        return;
    }

    eprintln!(
        "  → unhandled (state={state} mergeable={mergeable} mergeStateStatus={merge_state}), skipping"
    );
    summary.skipped += 1;
}

fn request_rebase(full_name: &str, number: &str) {
    if let Err(e) = gh::run(&[
        "pr",
        "comment",
        number,
        "--repo",
        full_name,
        "--body",
        "@dependabot rebase",
    ]) {
        eprintln!("  Warning: failed to comment rebase request: {e}");
    }
}

fn failing_check_names(detail: &GhPrDetail) -> Vec<String> {
    detail
        .status_check_rollup
        .as_ref()
        .map(|checks| {
            checks
                .iter()
                .filter(|c| {
                    c.conclusion
                        .as_deref()
                        .map(|s| s.eq_ignore_ascii_case("failure"))
                        .unwrap_or(false)
                })
                .map(|c| c.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failing_check_names_extracts_failures() {
        let detail = GhPrDetail {
            state: "OPEN".to_string(),
            mergeable: "MERGEABLE".to_string(),
            merge_state_status: "UNSTABLE".to_string(),
            status_check_rollup: Some(vec![
                StatusCheck {
                    name: "ci".to_string(),
                    conclusion: Some("FAILURE".to_string()),
                },
                StatusCheck {
                    name: "lint".to_string(),
                    conclusion: Some("SUCCESS".to_string()),
                },
                StatusCheck {
                    name: "test".to_string(),
                    conclusion: Some("failure".to_string()),
                },
            ]),
        };
        let names = failing_check_names(&detail);
        assert_eq!(names, vec!["ci", "test"]);
    }

    #[test]
    fn failing_check_names_handles_none() {
        let detail = GhPrDetail {
            state: "OPEN".to_string(),
            mergeable: "MERGEABLE".to_string(),
            merge_state_status: "UNSTABLE".to_string(),
            status_check_rollup: None,
        };
        let names = failing_check_names(&detail);
        assert!(names.is_empty());
    }

    #[test]
    fn summary_defaults_to_zero() {
        let s = Summary::default();
        assert_eq!(s.merged, 0);
        assert_eq!(s.rebase_requested, 0);
        assert_eq!(s.unstable, 0);
        assert_eq!(s.already_closed, 0);
        assert_eq!(s.skipped, 0);
    }
}
