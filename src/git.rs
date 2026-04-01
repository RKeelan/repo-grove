use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
}

pub fn run(repo_path: &Path, args: &[&str]) -> Result<GitOutput> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .context("failed to execute git — is git installed?")?;

    let stdout = String::from_utf8(output.stdout).context("git output was not valid UTF-8")?;
    let stderr = String::from_utf8(output.stderr).context("git stderr was not valid UTF-8")?;

    if !output.status.success() {
        bail!(
            "git -C {} {} failed: {}",
            repo_path.display(),
            args.join(" "),
            stderr.trim()
        );
    }

    Ok(GitOutput { stdout, stderr })
}

pub fn run_status(repo_path: &Path, args: &[&str]) -> Result<(bool, GitOutput)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .context("failed to execute git")?;

    let stdout = String::from_utf8(output.stdout).context("git output was not valid UTF-8")?;
    let stderr = String::from_utf8(output.stderr).context("git stderr was not valid UTF-8")?;

    Ok((output.status.success(), GitOutput { stdout, stderr }))
}
