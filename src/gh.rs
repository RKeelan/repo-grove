use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;

pub fn run(args: &[&str]) -> Result<String> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("failed to execute gh — is the GitHub CLI installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh {} failed: {}", args.join(" "), stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("gh output was not valid UTF-8")?;
    Ok(stdout)
}

pub fn json<T: DeserializeOwned>(args: &[&str]) -> Result<T> {
    let stdout = run(args)?;
    serde_json::from_str(&stdout)
        .with_context(|| format!("failed to parse JSON from gh {}", args.join(" ")))
}

pub fn api(endpoint: &str, extra_args: &[&str]) -> Result<String> {
    let mut args = vec!["api", endpoint];
    args.extend_from_slice(extra_args);
    run(&args)
}

pub fn api_json<T: DeserializeOwned>(endpoint: &str, extra_args: &[&str]) -> Result<T> {
    let stdout = api(endpoint, extra_args)?;
    serde_json::from_str(&stdout)
        .with_context(|| format!("failed to parse JSON from gh api {endpoint}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_returns_error_on_bad_command() {
        let result = run(&["__nonexistent_subcommand__"]);
        assert!(result.is_err());
    }
}
