#[allow(dead_code)]
mod config;
mod dependabot;
#[allow(dead_code)]
mod gh;
#[allow(dead_code)]
mod git;
mod index;
mod ls;
#[allow(dead_code)]
mod models;
mod prune;
mod update;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "grove", about = "Manage a collection of Git repositories")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build or refresh the repository index
    Index,

    /// Fetch, pull, and report readiness for local checkouts
    Update {
        /// Update only this repo (owner/repo or repo name)
        #[arg(long)]
        repo: Option<String>,
    },

    /// Prune merged branches
    Prune {
        /// Prune only this repo (owner/repo or repo name)
        #[arg(long)]
        repo: Option<String>,
    },

    /// List audit items
    Ls {
        #[command(subcommand)]
        what: LsSubcommand,
    },

    /// Manage Dependabot PRs
    Dependabot {
        #[command(subcommand)]
        action: DependabotAction,
    },

    /// Run a gh api call against indexed repos
    Api {
        /// API endpoint template (supports {owner}, {repo}, {full_name}, {default_branch})
        endpoint: String,

        /// HTTP method
        #[arg(long, default_value = "GET")]
        method: String,

        /// Skip archived repos
        #[arg(long)]
        skip_archived: bool,

        /// Filter to a specific owner
        #[arg(long)]
        owner: Option<String>,

        /// Filter to a specific repo (owner/repo or repo name)
        #[arg(long)]
        repo: Option<String>,

        /// Only public repos
        #[arg(long)]
        public_only: bool,

        /// Only private repos
        #[arg(long)]
        private_only: bool,

        /// Print what would be done without executing
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum LsSubcommand {
    /// List open Dependabot PRs
    Prs {
        /// Write to stdout instead of state file
        #[arg(long)]
        stdout: bool,
    },
    /// List failing CI runs
    Ci {
        /// Write to stdout instead of state file
        #[arg(long)]
        stdout: bool,
    },
    /// List open issues
    Issues {
        /// Write to stdout instead of state file
        #[arg(long)]
        stdout: bool,
    },
}

#[derive(Subcommand)]
enum DependabotAction {
    /// Auto-merge open Dependabot PRs
    Merge {
        /// Filter to a specific repo (owner/repo or repo name)
        #[arg(long)]
        repo: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Index => {
            let config = config::Config::load()?;
            index::run(&config)?;
        }
        Commands::Update { repo } => {
            let config = config::Config::load()?;
            update::run(&config, repo.as_deref())?;
        }
        Commands::Prune { repo } => {
            let config = config::Config::load()?;
            prune::run(&config, repo.as_deref())?;
        }
        Commands::Ls { what } => {
            let config = config::Config::load()?;
            match what {
                LsSubcommand::Prs { stdout } => ls::run_prs(&config, stdout)?,
                LsSubcommand::Ci { stdout } => ls::run_ci(&config, stdout)?,
                LsSubcommand::Issues { stdout } => ls::run_issues(&config, stdout)?,
            }
        }
        Commands::Dependabot { action } => match action {
            DependabotAction::Merge { repo } => {
                let config = config::Config::load()?;
                dependabot::run_merge(&config, repo.as_deref())?;
            }
        },
        Commands::Api { .. } => todo!("grove api"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_parses_without_error() {
        Cli::command().debug_assert();
    }
}
