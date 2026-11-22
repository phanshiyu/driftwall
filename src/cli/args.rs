//! Command line arguments.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Architecture drift linter: a Mermaid diagram is the contract, your code must obey it.
///
/// Exit codes: 0 clean, 1 violations, 2 tool error.
#[derive(Debug, Parser)]
#[command(name = "driftwall", version, arg_required_else_help = true)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the code against the architecture contract.
    Lint(LintArgs),
}

/// Arguments of `driftwall lint`.
#[derive(Debug, Args)]
pub struct LintArgs {
    /// Repository root to lint.
    #[arg(value_name = "PATH", default_value = ".")]
    pub path: PathBuf,
    /// Contract file, relative to PATH [default: architecture.mmd].
    #[arg(long, value_name = "FILE")]
    pub contract: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = LintFormat::Text)]
    pub format: LintFormat,
    /// Fail on warnings too.
    #[arg(long)]
    pub strict: bool,
}

/// Output formats of `driftwall lint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LintFormat {
    /// Human readable text.
    Text,
    /// Stable JSON for agents.
    Json,
    /// SARIF 2.1.0 for code scanning.
    Sarif,
}
