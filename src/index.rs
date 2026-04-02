use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::config::Config;
use crate::gh;
use crate::git;
use crate::models::{Index, Owner, OwnerKind, Repo};

#[derive(Deserialize)]
struct GhOrg {
    login: String,
}

#[derive(Deserialize)]
struct GhRepo {
    name: String,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    #[serde(rename = "isArchived")]
    is_archived: bool,
    #[serde(rename = "defaultBranchRef")]
    default_branch_ref: Option<GhBranchRef>,
}

#[derive(Deserialize)]
struct GhBranchRef {
    name: String,
}

pub fn run(config: &Config) -> Result<()> {
    config.ensure_data_dir()?;

    let previous_index = load(config).ok();

    let owners = discover_owners(config, &previous_index)?;
    eprintln!("Found {} owner(s)", owners.len());

    let mut repos = Vec::new();
    for owner in &owners {
        let owner_repos = fetch_repos_for_owner(owner, config)?;
        eprintln!("  {}: {} repo(s)", owner.owner, owner_repos.len());
        repos.extend(owner_repos);
    }

    repos.sort_by(|a, b| a.full_name.cmp(&b.full_name));

    let index = Index {
        generated_at: chrono::Utc::now().to_rfc3339(),
        src_root: config.src_root.clone(),
        owners,
        repos,
    };

    let json = serde_json::to_string_pretty(&index).context("failed to serialise index")?;
    std::fs::write(&config.index_path, &json)
        .with_context(|| format!("failed to write index to {}", config.index_path.display()))?;

    let cloned = index.repos.iter().filter(|r| r.path.is_some()).count();
    eprintln!(
        "Wrote index ({} repos, {} cloned) to {}",
        index.repos.len(),
        cloned,
        config.index_path.display()
    );
    Ok(())
}

pub fn load(config: &Config) -> Result<Index> {
    let content = std::fs::read_to_string(&config.index_path).with_context(|| {
        format!(
            "failed to read index from {} — have you run `grove index`?",
            config.index_path.display()
        )
    })?;
    serde_json::from_str(&content).context("failed to parse index JSON")
}

fn discover_owners(config: &Config, previous: &Option<Index>) -> Result<Vec<Owner>> {
    let previous_dirs: HashMap<String, String> = previous
        .as_ref()
        .map(|idx| {
            idx.owners
                .iter()
                .map(|o| (o.owner.clone(), o.dir.clone()))
                .collect()
        })
        .unwrap_or_default();

    let resolve_dir = |name: &str| -> String {
        previous_dirs
            .get(name)
            .cloned()
            .unwrap_or_else(|| resolve_owner_dir(&config.src_root, name))
    };

    let personal = &config.personal_owner;
    let orgs = fetch_admin_orgs()?;

    let mut owners: Vec<Owner> = std::iter::once((personal.clone(), OwnerKind::User))
        .chain(
            orgs.into_iter()
                .filter(|org| org != personal)
                .map(|org| (org, OwnerKind::Org)),
        )
        .map(|(name, kind)| Owner {
            dir: resolve_dir(&name),
            owner: name,
            kind,
        })
        .collect();

    owners.sort_by(|a, b| a.owner.to_lowercase().cmp(&b.owner.to_lowercase()));
    Ok(owners)
}

fn fetch_admin_orgs() -> Result<Vec<String>> {
    let output = gh::api("user/memberships/orgs", &["--paginate"])?;
    if output.stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    if !output.success {
        bail!("gh api user/memberships/orgs failed: {}", output.stderr);
    }

    #[derive(Deserialize)]
    struct Membership {
        role: String,
        organization: GhOrg,
    }

    let memberships: Vec<Membership> =
        serde_json::from_str(&output.stdout).context("failed to parse org memberships")?;

    Ok(memberships
        .into_iter()
        .filter(|m| m.role == "admin")
        .map(|m| m.organization.login)
        .collect())
}

fn fetch_repos_for_owner(owner: &Owner, config: &Config) -> Result<Vec<Repo>> {
    let gh_repos: Vec<GhRepo> = gh::json(&[
        "repo",
        "list",
        &owner.owner,
        "--limit",
        "1000",
        "--json",
        "name,nameWithOwner,isPrivate,isArchived,defaultBranchRef",
    ])?;

    // Build remote-URL map lazily: only scan the owner directory if any repo
    // fails name-based probing.
    let mut remote_map: Option<HashMap<String, PathBuf>> = None;

    let repos = gh_repos
        .into_iter()
        .map(|r| {
            let full_name = &r.name_with_owner;

            // Tier 1: name-based probing (exact, lowercase, kebab-case)
            let path =
                resolve_repo_path_by_name(&config.src_root, &owner.dir, &r.name).or_else(|| {
                    // Fallback: scan owner dir for git remotes
                    let map = remote_map.get_or_insert_with(|| {
                        scan_owner_dir_remotes(&config.src_root, &owner.dir, &owner.owner)
                    });
                    map.get(full_name).cloned()
                });

            Repo {
                owner: owner.owner.clone(),
                repo: r.name,
                full_name: r.name_with_owner,
                owner_dir: owner.dir.clone(),
                path,
                is_private: r.is_private,
                is_archived: r.is_archived,
                default_branch: r.default_branch_ref.map(|b| b.name),
            }
        })
        .collect();

    Ok(repos)
}

/// Convert an owner name to a directory name: lowercase, non-alphanumeric to hyphens.
fn owner_to_dir(owner: &str) -> String {
    let mut result = String::with_capacity(owner.len());
    for ch in owner.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push('-');
        }
    }
    result.trim_matches('-').to_string()
}

/// Probe the filesystem for the owner directory: exact name, then normalized lowercase.
fn resolve_owner_dir(src_root: &std::path::Path, owner: &str) -> String {
    if src_root.join(owner).is_dir() {
        return owner.to_string();
    }
    let normalized = owner_to_dir(owner);
    if normalized != owner && src_root.join(&normalized).is_dir() {
        return normalized;
    }
    normalized
}

/// Probe the filesystem for the repo by name: exact, lowercase, then kebab-case.
fn resolve_repo_path_by_name(src_root: &Path, owner_dir: &str, repo_name: &str) -> Option<PathBuf> {
    let base = src_root.join(owner_dir);

    let candidates = [
        repo_name.to_string(),
        repo_name.to_ascii_lowercase(),
        repo_to_kebab(repo_name),
    ];

    for candidate in &candidates {
        let path = base.join(candidate);
        if path.join(".git").exists() {
            return Some(path);
        }
    }

    None
}

/// Scan an owner's directory for git repos and map their remote URLs to paths.
/// Only examines immediate subdirectories (does not recurse past `.git`).
fn scan_owner_dir_remotes(
    src_root: &Path,
    owner_dir: &str,
    expected_owner: &str,
) -> HashMap<String, PathBuf> {
    let base = src_root.join(owner_dir);
    let mut map = HashMap::new();

    let entries = match std::fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(_) => return map,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.join(".git").exists() {
            continue;
        }
        if let Some(full_name) = remote_full_name(&path) {
            if let Some(remote_owner) = full_name.split('/').next() {
                if !remote_owner.eq_ignore_ascii_case(expected_owner) {
                    eprintln!(
                        "  warning: {} has remote owner '{}', expected '{}'",
                        path.display(),
                        remote_owner,
                        expected_owner
                    );
                }
            }
            map.insert(full_name, path);
        }
    }

    map
}

/// Extract `owner/repo` from a git repo's origin remote URL.
fn remote_full_name(repo_path: &Path) -> Option<String> {
    let output = git::run(repo_path, &["remote", "get-url", "origin"]).ok()?;
    if !output.success {
        return None;
    }
    parse_github_remote(output.stdout.trim())
}

/// Parse `owner/repo` from a GitHub URL (HTTPS or SSH).
fn parse_github_remote(url: &str) -> Option<String> {
    // SSH: git@github.com:owner/repo.git
    if let Some(path) = url.strip_prefix("git@github.com:") {
        let path = path.strip_suffix(".git").unwrap_or(path);
        return Some(path.to_string());
    }
    // HTTPS: https://github.com/owner/repo.git
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        let path = rest.strip_suffix(".git").unwrap_or(rest);
        return Some(path.to_string());
    }
    None
}

/// Convert a repo name to kebab-case: split on CamelCase boundaries, lowercase,
/// normalise non-alphanumeric chars to hyphens.
fn repo_to_kebab(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    let mut prev = None::<char>;

    for ch in name.chars() {
        if let Some(p) = prev {
            if ch.is_ascii_uppercase() && (p.is_ascii_lowercase() || p.is_ascii_digit()) {
                result.push('-');
            }
        }
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push('-');
        }
        prev = Some(ch);
    }

    result
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn owner_to_dir_lowercases() {
        assert_eq!(owner_to_dir("RKeelan"), "rkeelan");
    }

    #[test]
    fn owner_to_dir_normalises_special_chars() {
        assert_eq!(owner_to_dir("My.Org"), "my-org");
        assert_eq!(owner_to_dir("--leading--"), "leading");
    }

    #[test]
    fn repo_to_kebab_converts_camel_case() {
        assert_eq!(repo_to_kebab("MyProject"), "my-project");
        assert_eq!(repo_to_kebab("repoGrove"), "repo-grove");
    }

    #[test]
    fn repo_to_kebab_handles_already_kebab() {
        assert_eq!(repo_to_kebab("repo-grove"), "repo-grove");
    }

    #[test]
    fn repo_to_kebab_handles_numbers() {
        assert_eq!(repo_to_kebab("thing2Do"), "thing2-do");
    }

    #[test]
    fn repo_to_kebab_collapses_hyphens() {
        assert_eq!(repo_to_kebab("a__b--c"), "a-b-c");
    }

    #[test]
    fn resolve_repo_path_by_name_returns_none_when_not_cloned() {
        let tmp = PathBuf::from("/nonexistent");
        assert!(resolve_repo_path_by_name(&tmp, "owner", "MyRepo").is_none());
    }

    #[test]
    fn resolve_repo_path_by_name_finds_exact_match() {
        let tmp = std::env::temp_dir().join("grove-test-exact");
        let repo_git = tmp.join("owner").join("MyRepo").join(".git");
        std::fs::create_dir_all(&repo_git).unwrap();

        let result = resolve_repo_path_by_name(&tmp, "owner", "MyRepo").unwrap();
        assert!(result.ends_with("MyRepo"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_repo_path_by_name_finds_lowercase() {
        let tmp = std::env::temp_dir().join("grove-test-lower");
        let repo_git = tmp.join("owner").join("myrepo").join(".git");
        std::fs::create_dir_all(&repo_git).unwrap();

        let result = resolve_repo_path_by_name(&tmp, "owner", "MyRepo").unwrap();
        // On case-insensitive filesystems, the exact-match probe may hit the
        // lowercase dir first, so accept either leaf name.
        let leaf = result.file_name().unwrap().to_string_lossy();
        assert!(
            leaf == "myrepo" || leaf == "MyRepo",
            "expected myrepo or MyRepo, got {leaf}"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_repo_path_by_name_finds_kebab() {
        let tmp = std::env::temp_dir().join("grove-test-kebab");
        let repo_git = tmp.join("owner").join("my-repo").join(".git");
        std::fs::create_dir_all(&repo_git).unwrap();

        let result = resolve_repo_path_by_name(&tmp, "owner", "MyRepo").unwrap();
        assert!(result.ends_with("my-repo"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn parse_github_remote_https() {
        assert_eq!(
            parse_github_remote("https://github.com/CompactFrameFormat/cff-c.git"),
            Some("CompactFrameFormat/cff-c".to_string())
        );
    }

    #[test]
    fn parse_github_remote_https_no_suffix() {
        assert_eq!(
            parse_github_remote("https://github.com/RKeelan/repo-grove"),
            Some("RKeelan/repo-grove".to_string())
        );
    }

    #[test]
    fn parse_github_remote_ssh() {
        assert_eq!(
            parse_github_remote("git@github.com:RKeelan/repo-grove.git"),
            Some("RKeelan/repo-grove".to_string())
        );
    }

    #[test]
    fn parse_github_remote_non_github() {
        assert!(parse_github_remote("https://gitlab.com/user/repo.git").is_none());
    }
}
