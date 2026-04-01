use std::path::PathBuf;

use anyhow::{Context, Result};

pub struct Config {
    pub src_root: PathBuf,
    pub data_dir: PathBuf,
    pub personal_owner: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let src_root = match std::env::var("SRC_ROOT") {
            Ok(val) => PathBuf::from(val),
            Err(_) => dirs::home_dir()
                .context("could not determine home directory")?
                .join("src"),
        };

        let data_dir = match std::env::var("STATE_ROOT") {
            Ok(val) => PathBuf::from(val),
            Err(_) => dirs::data_dir()
                .context("could not determine data directory")?
                .join("repo-grove"),
        };

        let personal_owner =
            std::env::var("GITHUB_PERSONAL_OWNER").unwrap_or_else(|_| "RKeelan".to_string());

        Ok(Self {
            src_root,
            data_dir,
            personal_owner,
        })
    }

    pub fn index_path(&self) -> PathBuf {
        match std::env::var("REPO_INDEX_PATH") {
            Ok(val) => PathBuf::from(val),
            Err(_) => self.data_dir.join("repo-index.json"),
        }
    }

    pub fn dependabot_prs_path(&self) -> PathBuf {
        self.data_dir.join("dependabot-prs.jsonl")
    }

    pub fn failing_ci_path(&self) -> PathBuf {
        self.data_dir.join("failing-ci.jsonl")
    }

    pub fn open_issues_path(&self) -> PathBuf {
        self.data_dir.join("open-issues.jsonl")
    }

    pub fn ensure_data_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir).with_context(|| {
            format!(
                "failed to create data directory: {}",
                self.data_dir.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_file_paths_use_expected_names() {
        let config = Config {
            src_root: PathBuf::from("/src"),
            data_dir: PathBuf::from("/data"),
            personal_owner: "test".to_string(),
        };
        assert!(config.index_path().ends_with("repo-index.json"));
        assert!(config
            .dependabot_prs_path()
            .ends_with("dependabot-prs.jsonl"));
        assert!(config.failing_ci_path().ends_with("failing-ci.jsonl"));
        assert!(config.open_issues_path().ends_with("open-issues.jsonl"));
    }
}
