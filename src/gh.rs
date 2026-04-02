use std::process::Command;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

pub struct GhOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(args: &[&str]) -> Result<GhOutput> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("failed to execute gh — is the GitHub CLI installed?")?;

    let stdout = String::from_utf8(output.stdout).context("gh output was not valid UTF-8")?;
    let stderr = String::from_utf8(output.stderr).context("gh stderr was not valid UTF-8")?;

    Ok(GhOutput {
        success: output.status.success(),
        stdout,
        stderr,
    })
}

pub fn json<T: DeserializeOwned>(args: &[&str]) -> Result<T> {
    let output = run(args)?;
    anyhow::ensure!(
        output.success,
        "gh {} failed: {}",
        args.join(" "),
        output.stderr.trim()
    );
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("failed to parse JSON from gh {}", args.join(" ")))
}

pub fn api(endpoint: &str, extra_args: &[&str]) -> Result<GhOutput> {
    let mut args = vec!["api", endpoint];
    args.extend_from_slice(extra_args);
    run(&args)
}

pub fn api_json<T: DeserializeOwned>(endpoint: &str, extra_args: &[&str]) -> Result<T> {
    let output = api(endpoint, extra_args)?;
    anyhow::ensure!(
        output.success,
        "gh api {} failed: {}",
        endpoint,
        output.stderr.trim()
    );
    serde_json::from_str(&output.stdout)
        .with_context(|| format!("failed to parse JSON from gh api {endpoint}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_returns_ok_with_failure_status_on_bad_command() {
        let output = run(&["__nonexistent_subcommand__"]).unwrap();
        assert!(!output.success);
    }
}
