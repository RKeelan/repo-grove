use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// --- Index ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub generated_at: String,
    pub src_root: PathBuf,
    pub owners: Vec<Owner>,
    pub repos: Vec<Repo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OwnerKind {
    User,
    Org,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Owner {
    pub owner: String,
    pub dir: String,
    pub kind: OwnerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub owner: String,
    pub repo: String,
    pub full_name: String,
    pub owner_dir: String,
    pub path: PathBuf,
    pub is_private: bool,
    pub is_archived: bool,
    pub default_branch: Option<String>,
}

// --- Readiness (check) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessMode {
    Missing,
    Dirty,
    Pristine,
    PristinePullFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoReadiness {
    pub timestamp: String,
    pub repo_path: String,
    pub repo_full_name: String,
    pub ok: bool,
    pub mode: ReadinessMode,
    pub work_path: Option<String>,
    pub default_branch: Option<String>,
    pub message: String,
    pub dirty_status: Option<String>,
}

// --- Audit items (ls) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependabotPr {
    pub item_type: String,
    pub repo: String,
    pub repo_path: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub head_ref: String,
    pub base_ref: String,
    pub is_draft: bool,
    pub updated_at: String,
    pub repo_ready: Option<RepoReadiness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailingCi {
    pub item_type: String,
    pub repo: String,
    pub repo_path: String,
    pub default_branch: String,
    pub run_id: u64,
    pub workflow_name: String,
    pub status: String,
    pub conclusion: String,
    pub url: String,
    pub created_at: String,
    pub updated_at: String,
    pub head_sha: String,
    pub repo_ready: Option<RepoReadiness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenIssue {
    pub item_type: String,
    pub repo: String,
    pub repo_path: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_kind_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&OwnerKind::User).unwrap(), "\"user\"");
        assert_eq!(serde_json::to_string(&OwnerKind::Org).unwrap(), "\"org\"");
    }

    #[test]
    fn readiness_mode_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ReadinessMode::PristinePullFailed).unwrap(),
            "\"pristine_pull_failed\""
        );
    }

    #[test]
    fn index_round_trips_through_json() {
        let index = Index {
            generated_at: "2026-04-01T00:00:00Z".to_string(),
            src_root: PathBuf::from("/home/user/src"),
            owners: vec![Owner {
                owner: "RKeelan".to_string(),
                dir: "RKeelan".to_string(),
                kind: OwnerKind::User,
            }],
            repos: vec![Repo {
                owner: "RKeelan".to_string(),
                repo: "repo-grove".to_string(),
                full_name: "RKeelan/repo-grove".to_string(),
                owner_dir: "RKeelan".to_string(),
                path: PathBuf::from("/home/user/src/RKeelan/repo-grove"),
                is_private: false,
                is_archived: false,
                default_branch: Some("main".to_string()),
            }],
        };
        let json = serde_json::to_string(&index).unwrap();
        let deserialized: Index = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.repos.len(), 1);
        assert_eq!(deserialized.repos[0].full_name, "RKeelan/repo-grove");
    }
}
