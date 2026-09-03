use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "burrow",
    version,
    about = "A disposable hole for your agent to work in."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run a command inside a fresh worktree and report what it changed.
    Run(RunArgs),
    /// List active burrows.
    Ls,
    /// Show the full receipt for a burrow.
    Show(ShowArgs),
    /// Commit a burrow's changes and land them in the main repo.
    Promote(PromoteArgs),
    /// Remove one or more burrows.
    Rm(RmArgs),
    /// Check that burrow can operate in this environment.
    Doctor,
}

#[derive(Args)]
pub struct RunArgs {
    /// Git ref to branch the burrow from. Defaults to HEAD.
    #[arg(long)]
    pub from: Option<String>,

    /// Maximum number of changed files before the run is blocked.
    #[arg(long)]
    pub max_files: Option<usize>,

    /// Maximum number of added bytes before the run is blocked.
    #[arg(long)]
    pub max_bytes: Option<u64>,

    /// Extra denylist glob. May be given multiple times. Adds to the defaults.
    #[arg(long = "deny")]
    pub deny: Vec<String>,

    /// Wall-clock timeout in seconds. No timeout by default.
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Always keep the worktree, regardless of verdict.
    #[arg(long, conflicts_with = "discard")]
    pub keep: bool,

    /// Always remove the worktree, regardless of verdict.
    #[arg(long)]
    pub discard: bool,

    /// Print the receipt as JSON instead of the human report.
    #[arg(long)]
    pub json: bool,

    /// The command to run, e.g. `-- claude -p "fix the bug"`.
    #[arg(last = true, required = true, num_args = 1..)]
    pub command: Vec<String>,
}

#[derive(Args)]
pub struct ShowArgs {
    pub id: String,
}

#[derive(Args)]
pub struct PromoteArgs {
    pub id: String,

    /// Branch name to create in the main repo. Defaults to burrow/<id>.
    #[arg(long)]
    pub branch: Option<String>,

    /// Write a patch file instead of creating a branch.
    #[arg(long)]
    pub patch: Option<PathBuf>,

    /// Promote a blocked burrow anyway, or overwrite an existing branch.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct RmArgs {
    pub id: Option<String>,

    /// Remove every burrow that is not currently running.
    #[arg(long)]
    pub all: bool,

    /// Remove burrows whose process died without cleaning up after itself.
    #[arg(long)]
    pub stale: bool,
}
