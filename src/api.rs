use anyhow::Result;
use serde_json::Value;

use crate::config::Config;
use crate::gh;
use crate::index;
use crate::models::Repo;

pub struct ApiOptions<'a> {
    pub endpoint: &'a str,
    pub method: &'a str,
    pub skip_archived: bool,
    pub owner: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub public_only: bool,
    pub private_only: bool,
    pub dry_run: bool,
}

pub fn run(config: &Config, opts: &ApiOptions) -> Result<()> {
    let idx = index::load(config)?;
    let repos = index::filter_repos(&idx, opts.repo, "Index is empty — nothing to process.")?;

    if repos.is_empty() {
        return Ok(());
    }

    let repos: Vec<&Repo> = repos
        .into_iter()
        .filter(|r| {
            if opts.skip_archived && r.is_archived {
                return false;
            }
            if let Some(owner) = opts.owner {
                if r.owner != owner {
                    return false;
                }
            }
            if opts.public_only && r.is_private {
                return false;
            }
            if opts.private_only && !r.is_private {
                return false;
            }
            true
        })
        .collect();

    let total = repos.len();
    let mut ok: u32 = 0;
    let mut failed: u32 = 0;

    for (i, repo) in repos.iter().enumerate() {
        let n = i + 1;

        let default_branch = match &repo.default_branch {
            Some(b) => b.as_str(),
            None => {
                eprintln!("[{n}] SKIP {} (no default branch)", repo.full_name);
                continue;
            }
        };

        let endpoint = expand_endpoint(opts.endpoint, repo, default_branch);

        let extra_args: Vec<&str> = if opts.method != "GET" {
            vec!["-X", opts.method]
        } else {
            vec![]
        };

        if opts.dry_run {
            let method_part = if opts.method != "GET" {
                format!("-X {} ", opts.method)
            } else {
                String::new()
            };
            eprintln!(
                "[{n}] DRY {} -> gh api {method_part}{endpoint}",
                repo.full_name
            );
            ok += 1;
            continue;
        }

        eprintln!("[{n}] RUN {} ({default_branch})", repo.full_name);
        match gh::api(&endpoint, &extra_args) {
            Ok(output) if output.success => {
                emit_jsonl(&repo.full_name, &output.stdout);
                ok += 1;
            }
            Ok(output) => {
                eprint!("{}", output.stderr);
                eprintln!("[{n}] FAIL {}", repo.full_name);
                failed += 1;
            }
            Err(e) => {
                eprintln!("[{n}] FAIL {}: {e}", repo.full_name);
                failed += 1;
            }
        }
    }

    eprintln!("Completed: total={total} ok={ok} failed={failed}");

    if failed > 0 {
        anyhow::bail!("{failed} API call(s) failed");
    }

    Ok(())
}

/// Emit one JSONL line per repo: `{"repo": "owner/repo", "response": <parsed JSON>}`.
/// If the response isn't valid JSON, fall back to embedding it as a string.
fn emit_jsonl(full_name: &str, raw: &str) {
    let response: Value =
        serde_json::from_str(raw.trim()).unwrap_or_else(|_| Value::String(raw.trim().to_string()));
    let wrapper = serde_json::json!({
        "repo": full_name,
        "response": response,
    });
    println!(
        "{}",
        serde_json::to_string(&wrapper).expect("JSON serialisation cannot fail")
    );
}

fn expand_endpoint(template: &str, repo: &Repo, default_branch: &str) -> String {
    template
        .replace("{owner}", &repo.owner)
        .replace("{repo}", &repo.repo)
        .replace("{full_name}", &repo.full_name)
        .replace("{default_branch}", default_branch)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn test_repo() -> Repo {
        Repo {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            full_name: "octocat/hello-world".to_string(),
            owner_dir: "octocat".to_string(),
            path: Some(PathBuf::from("/src/octocat/hello-world")),
            is_private: false,
            is_archived: false,
            default_branch: Some("main".to_string()),
        }
    }

    #[test]
    fn expand_endpoint_replaces_all_placeholders() {
        let repo = test_repo();
        let result = expand_endpoint(
            "repos/{full_name}/branches/{default_branch}/protection",
            &repo,
            "main",
        );
        assert_eq!(result, "repos/octocat/hello-world/branches/main/protection");
    }

    #[test]
    fn expand_endpoint_replaces_owner_and_repo() {
        let repo = test_repo();
        let result = expand_endpoint("orgs/{owner}/repos/{repo}", &repo, "main");
        assert_eq!(result, "orgs/octocat/repos/hello-world");
    }

    #[test]
    fn expand_endpoint_no_placeholders() {
        let repo = test_repo();
        let result = expand_endpoint("rate_limit", &repo, "main");
        assert_eq!(result, "rate_limit");
    }

    #[test]
    fn emit_jsonl_wraps_json_response() {
        // emit_jsonl writes to stdout, so test the logic directly
        let raw = r#"{"total_count": 2, "workflows": []}"#;
        let parsed: Value = serde_json::from_str(raw).unwrap();
        let wrapper = serde_json::json!({"repo": "octocat/hello-world", "response": parsed});
        let line = serde_json::to_string(&wrapper).unwrap();
        let roundtrip: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(roundtrip["repo"], "octocat/hello-world");
        assert_eq!(roundtrip["response"]["total_count"], 2);
    }

    #[test]
    fn emit_jsonl_falls_back_to_string_for_non_json() {
        let raw = "Not JSON at all";
        let response: Value =
            serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.trim().to_string()));
        assert_eq!(response, Value::String("Not JSON at all".to_string()));
    }
}
