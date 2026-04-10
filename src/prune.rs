use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;

use crate::config::Config;
use crate::gh;
use crate::git;
use crate::index;

pub fn run(config: &Config, repo_filter: Option<&str>) -> Result<()> {
    let idx = index::load(config)?;
    let repos = index::filter_repos(&idx, repo_filter, "Index is empty — nothing to prune.")?;

    if repos.is_empty() {
        return Ok(());
    }

    let mut total_deleted: u32 = 0;
    let mut total_skipped: u32 = 0;
    let mut total_repos: u32 = 0;

    for repo in &repos {
        let repo_path = match &repo.path {
            Some(p) if p.join(".git").exists() => p,
            _ => {
                eprintln!("{}: skipping (not cloned locally)", repo.full_name);
                continue;
            }
        };

        let default_branch = match &repo.default_branch {
            Some(b) => b.as_str(),
            None => {
                eprintln!("{}: skipping (no default branch in index)", repo.full_name);
                continue;
            }
        };

        total_repos += 1;
        let (deleted, skipped) = prune_repo(repo_path, &repo.full_name, default_branch)?;
        total_deleted += deleted;
        total_skipped += skipped;
    }

    eprintln!(
        "Pruned {} repo(s): {} branch(es) deleted, {} skipped",
        total_repos, total_deleted, total_skipped
    );

    Ok(())
}

fn prune_repo(repo_path: &Path, full_name: &str, default_branch: &str) -> Result<(u32, u32)> {
    let checkout = git::run(repo_path, &["checkout", default_branch])?;
    if !checkout.success {
        eprintln!(
            "{}: failed to checkout {}: {}",
            full_name,
            default_branch,
            checkout.stderr.trim()
        );
        return Ok((0, 0));
    }

    let pull = git::run(repo_path, &["pull", "--ff-only"])?;
    if !pull.success {
        eprintln!(
            "{}: pull --ff-only failed: {}",
            full_name,
            pull.stderr.trim()
        );
        // Continue with pruning even if pull fails — branches can still be evaluated
    }

    let merged_branches = list_merged_branches(repo_path, default_branch)?;

    let all_local = list_local_branches(repo_path, default_branch)?;
    let squash_merged = find_squash_merged_branches(full_name, &all_local, &merged_branches)?;

    let mut deleted: u32 = 0;
    let mut skipped: u32 = 0;

    for branch in &merged_branches {
        let (d, s) = delete_branch(repo_path, full_name, branch, "merged", false)?;
        deleted += d;
        skipped += s;
    }

    for branch in &squash_merged {
        if confirm_delete(full_name, branch)? {
            let (d, s) = delete_branch(repo_path, full_name, branch, "squash-merged", true)?;
            deleted += d;
            skipped += s;
        } else {
            skipped += 1;
        }
    }

    Ok((deleted, skipped))
}

fn delete_branch(
    repo_path: &Path,
    full_name: &str,
    branch: &str,
    kind: &str,
    force: bool,
) -> Result<(u32, u32)> {
    let flag = if force { "-D" } else { "-d" };
    let result = git::run(repo_path, &["branch", flag, branch])?;
    if result.success {
        eprintln!("{}: deleted {} branch '{}'", full_name, kind, branch);
        Ok((1, 0))
    } else {
        eprintln!(
            "{}: failed to delete '{}': {}",
            full_name,
            branch,
            result.stderr.trim()
        );
        Ok((0, 1))
    }
}

fn list_merged_branches(repo_path: &Path, default_branch: &str) -> Result<Vec<String>> {
    let output = git::run(repo_path, &["branch", "--merged", default_branch])?;
    if !output.success {
        return Ok(Vec::new());
    }

    Ok(parse_branch_lines(&output.stdout, default_branch, true))
}

fn list_local_branches(repo_path: &Path, default_branch: &str) -> Result<Vec<String>> {
    let output = git::run(repo_path, &["branch", "--format=%(refname:short)"])?;
    if !output.success {
        return Ok(Vec::new());
    }

    Ok(parse_branch_lines(&output.stdout, default_branch, false))
}

fn parse_branch_lines(output: &str, default_branch: &str, strip_star: bool) -> Vec<String> {
    output
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if strip_star {
                trimmed.trim_start_matches("* ").to_string()
            } else {
                trimmed.to_string()
            }
        })
        .filter(|name| name != default_branch && !name.is_empty())
        .collect()
}

/// Detect branches whose associated PR was squash-merged on GitHub. These branches won't appear in
/// `git branch --merged` because the squash commit has a different hash.
fn find_squash_merged_branches(
    full_name: &str,
    all_local: &[String],
    already_merged: &[String],
) -> Result<Vec<String>> {
    let unmerged: Vec<&String> = all_local
        .iter()
        .filter(|b| !already_merged.contains(b))
        .collect();

    if unmerged.is_empty() {
        return Ok(Vec::new());
    }

    let merged_head_refs = fetch_merged_pr_heads(full_name)?;

    Ok(unmerged
        .into_iter()
        .filter(|b| merged_head_refs.iter().any(|h| h == *b))
        .cloned()
        .collect())
}

/// Fetch the head ref names of all merged PRs for a repo in a single `gh` call.
fn fetch_merged_pr_heads(full_name: &str) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct PrHead {
        #[serde(rename = "headRefName")]
        head_ref_name: String,
    }

    let prs: Vec<PrHead> = match gh::json(&[
        "pr",
        "list",
        "--repo",
        full_name,
        "--state",
        "merged",
        "--json",
        "headRefName",
        "--limit",
        "200",
    ]) {
        Ok(prs) => prs,
        Err(_) => return Ok(Vec::new()),
    };

    Ok(prs.into_iter().map(|pr| pr.head_ref_name).collect())
}

fn confirm_delete(full_name: &str, branch: &str) -> Result<bool> {
    eprint!(
        "{}: branch '{}' appears squash-merged on GitHub. Delete? [y/N] ",
        full_name, branch
    );
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_ascii_lowercase();

    Ok(answer == "y" || answer == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_branch_lines_excludes_default_and_strips_star() {
        let output = "* main\n  feature-a\n  feature-b\n";
        let branches = parse_branch_lines(output, "main", true);
        assert_eq!(branches, vec!["feature-a", "feature-b"]);
    }

    #[test]
    fn parse_branch_lines_handles_empty() {
        let branches = parse_branch_lines("* main\n", "main", true);
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_branch_lines_without_star_stripping() {
        let output = "main\nfeature-a\nfeature-b\n";
        let branches = parse_branch_lines(output, "main", false);
        assert_eq!(branches, vec!["feature-a", "feature-b"]);
    }
}
