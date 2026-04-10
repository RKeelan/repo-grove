use anyhow::Result;
use rayon::prelude::*;

use crate::config::Config;
use crate::git;
use crate::index;
use crate::models::{path_display_string, ReadinessMode, Repo, RepoReadiness};

pub fn run(config: &Config, repo_filter: Option<&str>) -> Result<()> {
    let idx = index::load(config)?;
    let repos = index::filter_repos(&idx, repo_filter, "Index is empty — nothing to check.")?;

    if repos.is_empty() {
        return Ok(());
    }

    let results: Vec<Result<RepoReadiness>> =
        repos.par_iter().map(|repo| check_repo(repo)).collect();

    let mut ok_count: u32 = 0;
    let mut fail_count: u32 = 0;

    for result in results {
        let readiness = result?;
        let json = serde_json::to_string(&readiness)?;
        println!("{json}");

        if readiness.ok {
            ok_count += 1;
        } else {
            fail_count += 1;
        }
    }

    eprintln!(
        "Checked {} repo(s): {} ok, {} not ready",
        repos.len(),
        ok_count,
        fail_count
    );

    Ok(())
}

pub fn check_repo(repo: &Repo) -> Result<RepoReadiness> {
    let now = chrono::Utc::now().to_rfc3339();
    let default_branch = repo.default_branch.clone();

    let repo_path = match &repo.path {
        Some(p) if p.join(".git").exists() => p.clone(),
        other => {
            let path_str = path_display_string(other.as_ref());
            return Ok(RepoReadiness {
                timestamp: now,
                repo_path: path_str,
                repo_full_name: repo.full_name.clone(),
                ok: false,
                mode: ReadinessMode::Missing,
                work_path: None,
                default_branch,
                message: "missing local git repo".to_string(),
                dirty_status: None,
            });
        }
    };

    let path_str = repo_path.to_string_lossy().to_string();

    git::run(&repo_path, &["fetch", "--prune"])?;

    let status_output = git::run(
        &repo_path,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    let status_text = status_output.stdout.trim().to_string();
    if !status_text.is_empty() {
        return Ok(RepoReadiness {
            timestamp: now,
            repo_path: path_str,
            repo_full_name: repo.full_name.clone(),
            ok: false,
            mode: ReadinessMode::Dirty,
            work_path: None,
            default_branch,
            message: "primary checkout is not pristine; resolve the dirty state before proceeding \
                      (commit, stash, or clean untracked files)"
                .to_string(),
            dirty_status: Some(status_text),
        });
    }

    let pull_output = git::run(&repo_path, &["pull", "--ff-only"])?;

    if pull_output.success {
        Ok(RepoReadiness {
            timestamp: now,
            repo_path: path_str.clone(),
            repo_full_name: repo.full_name.clone(),
            ok: true,
            mode: ReadinessMode::Pristine,
            work_path: Some(path_str),
            default_branch,
            message: "fetch --prune and pull --ff-only succeeded in primary checkout".to_string(),
            dirty_status: None,
        })
    } else {
        Ok(RepoReadiness {
            timestamp: now,
            repo_path: path_str.clone(),
            repo_full_name: repo.full_name.clone(),
            ok: false,
            mode: ReadinessMode::PristinePullFailed,
            work_path: Some(path_str),
            default_branch,
            message: "pull --ff-only failed in primary checkout; the local branch may have \
                      diverged from the remote"
                .to_string(),
            dirty_status: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::models::Repo;

    #[test]
    fn check_repo_missing_path_returns_missing() {
        let repo = Repo {
            owner: "test".to_string(),
            repo: "no-local".to_string(),
            full_name: "test/no-local".to_string(),
            owner_dir: "test".to_string(),
            path: None,
            is_private: false,
            is_archived: false,
            default_branch: Some("main".to_string()),
        };

        let result = check_repo(&repo).unwrap();
        assert!(!result.ok);
        assert_eq!(result.mode, ReadinessMode::Missing);
        assert_eq!(result.message, "missing local git repo");
    }

    #[test]
    fn check_repo_nonexistent_git_dir_returns_missing() {
        let repo = Repo {
            owner: "test".to_string(),
            repo: "no-git".to_string(),
            full_name: "test/no-git".to_string(),
            owner_dir: "test".to_string(),
            path: Some(PathBuf::from("/nonexistent/path/no-git")),
            is_private: false,
            is_archived: false,
            default_branch: Some("main".to_string()),
        };

        let result = check_repo(&repo).unwrap();
        assert!(!result.ok);
        assert_eq!(result.mode, ReadinessMode::Missing);
    }

    #[test]
    fn readiness_serialises_to_expected_json() {
        let readiness = RepoReadiness {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            repo_path: "/src/test/repo".to_string(),
            repo_full_name: "test/repo".to_string(),
            ok: true,
            mode: ReadinessMode::Pristine,
            work_path: Some("/src/test/repo".to_string()),
            default_branch: Some("main".to_string()),
            message: "all good".to_string(),
            dirty_status: None,
        };

        let json: serde_json::Value = serde_json::to_value(&readiness).unwrap();
        assert_eq!(json["mode"], "pristine");
        assert_eq!(json["ok"], true);
        assert!(json["dirty_status"].is_null());
    }
}
