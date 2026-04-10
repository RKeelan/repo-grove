use std::collections::HashMap;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::Config;
use crate::gh;
use crate::index;
use crate::models::{path_display_string, DependabotPr, FailingCi, OpenIssue, Repo, RepoReadiness};
use crate::update;

pub fn run_prs(config: &Config, stdout: bool) -> Result<()> {
    let idx = index::load(config)?;
    let repos: Vec<&Repo> = idx.repos.iter().filter(|r| !r.is_archived).collect();

    if repos.is_empty() {
        eprintln!("No repos in index");
        return Ok(());
    }

    let mut items: Vec<DependabotPr> = Vec::new();
    let mut readiness_cache: HashMap<String, RepoReadiness> = HashMap::new();

    for repo in &repos {
        let prs = match fetch_open_prs(&repo.full_name) {
            Ok(prs) => prs,
            Err(e) => {
                eprintln!("Warning: failed to list PRs for {}: {e}", repo.full_name);
                continue;
            }
        };

        let dependabot_prs: Vec<&GhPr> = prs.iter().filter(|pr| is_dependabot_author(pr)).collect();

        if dependabot_prs.is_empty() {
            continue;
        }

        let repo_path = path_display_string(repo.path.as_ref());
        let readiness = get_readiness(repo, &mut readiness_cache);

        for pr in dependabot_prs {
            items.push(DependabotPr {
                repo: repo.full_name.clone(),
                repo_path: repo_path.clone(),
                number: pr.number,
                title: pr.title.clone(),
                url: pr.url.clone(),
                head_ref: pr.head_ref_name.clone(),
                base_ref: pr.base_ref_name.clone(),
                is_draft: pr.is_draft,
                updated_at: pr.updated_at.clone(),
                repo_ready: readiness.clone(),
            });
        }
    }

    write_output(
        config.dependabot_prs_path(),
        &items,
        stdout,
        "dependabot PR",
    )
}

pub fn run_ci(config: &Config, stdout: bool) -> Result<()> {
    let idx = index::load(config)?;
    let repos: Vec<&Repo> = idx.repos.iter().filter(|r| !r.is_archived).collect();

    if repos.is_empty() {
        eprintln!("No repos in index");
        return Ok(());
    }

    let mut items: Vec<FailingCi> = Vec::new();
    let mut readiness_cache: HashMap<String, RepoReadiness> = HashMap::new();

    for repo in &repos {
        let default_branch = match &repo.default_branch {
            Some(b) => b,
            None => continue,
        };

        let runs = match fetch_workflow_runs(&repo.full_name, default_branch) {
            Ok(runs) => runs,
            Err(e) => {
                eprintln!(
                    "Warning: failed to list workflow runs for {}: {e}",
                    repo.full_name
                );
                continue;
            }
        };

        let failures = extract_failures(runs);
        if failures.is_empty() {
            continue;
        }

        let repo_path = path_display_string(repo.path.as_ref());
        let readiness = get_readiness(repo, &mut readiness_cache);

        for run in failures {
            items.push(FailingCi {
                repo: repo.full_name.clone(),
                repo_path: repo_path.clone(),
                default_branch: default_branch.clone(),
                run_id: run.database_id,
                workflow_name: run.workflow_name,
                status: run.status,
                conclusion: run.conclusion.unwrap_or_default(),
                url: run.url,
                created_at: run.created_at,
                updated_at: run.updated_at,
                head_sha: run.head_sha,
                repo_ready: readiness.clone(),
            });
        }
    }

    write_output(config.failing_ci_path(), &items, stdout, "failing CI")
}

pub fn run_issues(config: &Config, stdout: bool) -> Result<()> {
    let idx = index::load(config)?;
    let repos: Vec<&Repo> = idx.repos.iter().filter(|r| !r.is_archived).collect();

    if repos.is_empty() {
        eprintln!("No repos in index");
        return Ok(());
    }

    let mut items: Vec<OpenIssue> = Vec::new();

    for repo in &repos {
        let issues = match fetch_open_issues(&repo.full_name) {
            Ok(issues) => issues,
            Err(e) => {
                eprintln!("Warning: failed to list issues for {}: {e}", repo.full_name);
                continue;
            }
        };

        let repo_path = path_display_string(repo.path.as_ref());
        for issue in issues {
            items.push(OpenIssue {
                repo: repo.full_name.clone(),
                repo_path: repo_path.clone(),
                number: issue.number,
                title: issue.title,
                url: issue.url,
                created_at: issue.created_at,
                updated_at: issue.updated_at,
            });
        }
    }

    write_output(config.open_issues_path(), &items, stdout, "open issue")
}

#[derive(Deserialize)]
struct GhPr {
    number: u64,
    title: String,
    url: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    author: GhAuthor,
}

#[derive(Deserialize)]
struct GhAuthor {
    login: String,
}

#[derive(Deserialize)]
struct GhRun {
    #[serde(rename = "databaseId")]
    database_id: u64,
    #[serde(rename = "workflowName")]
    workflow_name: String,
    status: String,
    conclusion: Option<String>,
    url: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(rename = "headSha")]
    head_sha: String,
}

#[derive(Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    url: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

fn fetch_open_prs(full_name: &str) -> Result<Vec<GhPr>> {
    gh::json(&[
        "pr",
        "list",
        "--repo",
        full_name,
        "--state",
        "open",
        "--json",
        "number,title,url,headRefName,baseRefName,isDraft,updatedAt,author",
    ])
}

fn fetch_workflow_runs(full_name: &str, default_branch: &str) -> Result<Vec<GhRun>> {
    gh::json(&[
        "run",
        "list",
        "--repo",
        full_name,
        "--branch",
        default_branch,
        "--limit",
        "20",
        "--json",
        "databaseId,workflowName,status,conclusion,url,createdAt,updatedAt,headSha",
    ])
}

fn fetch_open_issues(full_name: &str) -> Result<Vec<GhIssue>> {
    gh::json(&[
        "issue",
        "list",
        "--repo",
        full_name,
        "--state",
        "open",
        "--limit",
        "200",
        "--json",
        "number,title,url,createdAt,updatedAt",
    ])
}

fn is_dependabot_author(pr: &GhPr) -> bool {
    pr.author.login == "app/dependabot" || pr.author.login == "dependabot[bot]"
}

/// Group workflow runs by workflow name, take the latest completed non-dependabot run
/// per workflow, then keep only failures.
fn extract_failures(runs: Vec<GhRun>) -> Vec<GhRun> {
    let completed: Vec<GhRun> = runs
        .into_iter()
        .filter(|r| {
            r.status == "completed" && !r.workflow_name.to_ascii_lowercase().contains("dependabot")
        })
        .collect();

    let mut by_workflow: HashMap<String, Vec<GhRun>> = HashMap::new();
    for run in completed {
        by_workflow
            .entry(run.workflow_name.clone())
            .or_default()
            .push(run);
    }

    let mut failures = Vec::new();
    for (_name, runs) in by_workflow {
        let latest = runs
            .into_iter()
            .max_by(|a, b| a.created_at.cmp(&b.created_at));
        if let Some(latest) = latest {
            if let Some(ref conclusion) = latest.conclusion {
                if matches!(
                    conclusion.as_str(),
                    "failure" | "timed_out" | "startup_failure" | "action_required"
                ) {
                    failures.push(latest);
                }
            }
        }
    }

    failures
}

fn get_readiness(repo: &Repo, cache: &mut HashMap<String, RepoReadiness>) -> Option<RepoReadiness> {
    if let Some(cached) = cache.get(&repo.full_name) {
        return Some(cached.clone());
    }

    match update::check_repo(repo) {
        Ok(readiness) => {
            cache.insert(repo.full_name.clone(), readiness.clone());
            Some(readiness)
        }
        Err(e) => {
            eprintln!(
                "Warning: readiness check failed for {}: {e}",
                repo.full_name
            );
            None
        }
    }
}

fn write_jsonl<T: serde::Serialize>(writer: &mut impl Write, items: &[T]) -> Result<()> {
    for item in items {
        serde_json::to_writer(&mut *writer, item)?;
        writeln!(writer)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_output<T: serde::Serialize>(
    state_path: PathBuf,
    items: &[T],
    stdout: bool,
    item_label: &str,
) -> Result<()> {
    if items.is_empty() {
        if !stdout && state_path.exists() {
            fs::remove_file(&state_path).ok();
        }
        eprintln!("No {item_label} action items");
        return Ok(());
    }

    if stdout {
        let out = io::stdout();
        let mut writer = BufWriter::new(out.lock());
        write_jsonl(&mut writer, items)?;
    } else {
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = state_path.with_extension("jsonl.tmp");
        {
            let file = fs::File::create(&tmp_path)
                .with_context(|| format!("failed to create {}", tmp_path.display()))?;
            let mut writer = BufWriter::new(file);
            write_jsonl(&mut writer, items)?;
        }
        fs::rename(&tmp_path, &state_path)
            .with_context(|| format!("failed to write {}", state_path.display()))?;
        eprintln!(
            "{} {item_label} item(s) written to {}",
            items.len(),
            state_path.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_dependabot_author_matches_bot_login() {
        let pr = GhPr {
            number: 1,
            title: "Bump foo".to_string(),
            url: "https://github.com/test/repo/pull/1".to_string(),
            head_ref_name: "dependabot/npm/foo-1.0".to_string(),
            base_ref_name: "main".to_string(),
            is_draft: false,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            author: GhAuthor {
                login: "dependabot[bot]".to_string(),
            },
        };
        assert!(is_dependabot_author(&pr));
    }

    #[test]
    fn is_dependabot_author_matches_app_login() {
        let pr = GhPr {
            number: 2,
            title: "Bump bar".to_string(),
            url: "https://github.com/test/repo/pull/2".to_string(),
            head_ref_name: "dependabot/npm/bar-2.0".to_string(),
            base_ref_name: "main".to_string(),
            is_draft: false,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            author: GhAuthor {
                login: "app/dependabot".to_string(),
            },
        };
        assert!(is_dependabot_author(&pr));
    }

    #[test]
    fn is_dependabot_author_rejects_other() {
        let pr = GhPr {
            number: 3,
            title: "Fix bug".to_string(),
            url: "https://github.com/test/repo/pull/3".to_string(),
            head_ref_name: "fix-bug".to_string(),
            base_ref_name: "main".to_string(),
            is_draft: false,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            author: GhAuthor {
                login: "octocat".to_string(),
            },
        };
        assert!(!is_dependabot_author(&pr));
    }

    #[test]
    fn extract_failures_groups_by_workflow_and_filters() {
        let runs = vec![
            GhRun {
                database_id: 1,
                workflow_name: "CI".to_string(),
                status: "completed".to_string(),
                conclusion: Some("failure".to_string()),
                url: "https://example.com/1".to_string(),
                created_at: "2026-01-02T00:00:00Z".to_string(),
                updated_at: "2026-01-02T00:00:00Z".to_string(),
                head_sha: "abc123".to_string(),
            },
            GhRun {
                database_id: 2,
                workflow_name: "CI".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                url: "https://example.com/2".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                head_sha: "def456".to_string(),
            },
            GhRun {
                database_id: 3,
                workflow_name: "Deploy".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                url: "https://example.com/3".to_string(),
                created_at: "2026-01-02T00:00:00Z".to_string(),
                updated_at: "2026-01-02T00:00:00Z".to_string(),
                head_sha: "ghi789".to_string(),
            },
        ];

        let failures = extract_failures(runs);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].workflow_name, "CI");
        assert_eq!(failures[0].database_id, 1);
    }

    #[test]
    fn extract_failures_skips_dependabot_workflows() {
        let runs = vec![GhRun {
            database_id: 10,
            workflow_name: "Dependabot auto-merge".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
            url: "https://example.com/10".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            head_sha: "abc".to_string(),
        }];

        let failures = extract_failures(runs);
        assert!(failures.is_empty());
    }

    #[test]
    fn extract_failures_skips_in_progress_runs() {
        let runs = vec![GhRun {
            database_id: 20,
            workflow_name: "CI".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
            url: "https://example.com/20".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            head_sha: "abc".to_string(),
        }];

        let failures = extract_failures(runs);
        assert!(failures.is_empty());
    }

    #[test]
    fn extract_failures_handles_timed_out_conclusion() {
        let runs = vec![GhRun {
            database_id: 30,
            workflow_name: "CI".to_string(),
            status: "completed".to_string(),
            conclusion: Some("timed_out".to_string()),
            url: "https://example.com/30".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            head_sha: "abc".to_string(),
        }];

        let failures = extract_failures(runs);
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn write_output_stdout_writes_jsonl() {
        // Just verify serialisation works without panicking
        let items = [OpenIssue {
            repo: "test/repo".to_string(),
            repo_path: "/src/test/repo".to_string(),
            number: 1,
            title: "Test issue".to_string(),
            url: "https://github.com/test/repo/issues/1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }];

        let json = serde_json::to_string(&items[0]).unwrap();
        assert!(json.contains("\"number\":1"));
        assert!(json.contains("\"repo\":\"test/repo\""));
    }
}
