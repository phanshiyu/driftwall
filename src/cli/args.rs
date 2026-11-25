//! Command line arguments.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Repository root used when `PATH` is omitted, shared by every command that takes one.
const DEFAULT_PATH: &str = ".";

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
    /// Write a baseline contract and `.arch` tags for an existing repository.
    ///
    /// The result lints clean. Refuses to overwrite existing files unless `--force`.
    Init(InitArgs),
    /// Show what a file is and which node owns it, with the .arch file that decided it.
    ///
    /// The file must exist. Exits 1 when a source file is unmapped.
    Where(WhereArgs),
    /// Print the whole architecture: nodes, directories, edges, how code realises them.
    ///
    /// Read-only and informational: exits 0 unless the tool fails.
    Brief(BriefArgs),
    /// Print one node: its directories, files, and every edge entering or leaving it.
    ///
    /// Includes edges code uses that the contract does not declare. Read-only.
    Neighbors(NeighborsArgs),
    /// Compare the contract with the one at a git ref, for human review.
    ///
    /// Exits 0 whatever changed, unless `--exit-code` asks for 1 on any change.
    Diff(DiffArgs),
}

/// Output formats of the commands that present the architecture or a contract change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ViewFormat {
    /// Human and agent readable text.
    Text,
    /// Stable JSON for agents.
    Json,
}

/// Arguments of `driftwall brief`.
#[derive(Debug, Args)]
pub struct BriefArgs {
    /// Repository root to describe.
    #[arg(value_name = "PATH", default_value = DEFAULT_PATH)]
    pub path: PathBuf,
    /// Contract file, relative to PATH [default: architecture.mmd].
    #[arg(long, value_name = "FILE")]
    pub contract: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = ViewFormat::Text)]
    pub format: ViewFormat,
}

/// Arguments of `driftwall neighbors`.
#[derive(Debug, Args)]
pub struct NeighborsArgs {
    /// Node id to describe.
    #[arg(value_name = "NODE")]
    pub node: String,
    /// Repository root.
    #[arg(value_name = "PATH", default_value = DEFAULT_PATH)]
    pub path: PathBuf,
    /// Contract file, relative to PATH [default: architecture.mmd].
    #[arg(long, value_name = "FILE")]
    pub contract: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = ViewFormat::Text)]
    pub format: ViewFormat,
    /// Text: list the source files of the node under the `files:` line. JSON always has them.
    #[arg(long)]
    pub files: bool,
    /// Text: list every import site of every edge, not just the first three. JSON always does.
    #[arg(long)]
    pub all_sites: bool,
}

/// Arguments of `driftwall diff`.
#[derive(Debug, Args)]
pub struct DiffArgs {
    /// Git ref holding the contract to compare against, for example `main` or `HEAD~1`.
    #[arg(long, value_name = "REF")]
    pub base: String,
    /// Repository root.
    #[arg(value_name = "PATH", default_value = DEFAULT_PATH)]
    pub path: PathBuf,
    /// Contract file, relative to PATH [default: architecture.mmd].
    #[arg(long, value_name = "FILE")]
    pub contract: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = ViewFormat::Text)]
    pub format: ViewFormat,
    /// Exit 1 when the contract, a rule, a boundary or a file's owner changed, or debt was added.
    #[arg(long)]
    pub exit_code: bool,
    /// Debt baseline, relative to PATH: also report entries added since REF.
    #[arg(long, value_name = "FILE")]
    pub baseline: Option<PathBuf>,
}

/// Arguments of `driftwall where`.
#[derive(Debug, Args)]
pub struct WhereArgs {
    /// File to look up, relative to the root or absolute inside it.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
    /// Repository root.
    #[arg(long, value_name = "DIR", default_value = ".")]
    pub root: PathBuf,
    /// Contract file, relative to the root [default: architecture.mmd]. Must be valid when given.
    #[arg(long, value_name = "FILE")]
    pub contract: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = WhereFormat::Text)]
    pub format: WhereFormat,
}

/// Output formats of `driftwall where`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WhereFormat {
    /// `<file>: <what it is and who owns it>`, for example `<file>: <node> (via <arch file>)`.
    Text,
    /// An object with `file`, `node`, `arch_file`, `kind` and, when there is one, `detail`; for a
    /// source file that uses packages also `packages`.
    Json,
}

/// Arguments of `driftwall lint`.
#[derive(Debug, Args)]
pub struct LintArgs {
    /// Repository root to lint.
    #[arg(value_name = "PATH", default_value = DEFAULT_PATH)]
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
    /// Accept the findings a debt baseline records, relative to PATH. Fails only on new debt.
    #[arg(long, value_name = "FILE", conflicts_with = "write_baseline")]
    pub baseline: Option<PathBuf>,
    /// Write a debt baseline of the current findings, relative to PATH, and exit 0.
    #[arg(long, value_name = "FILE")]
    pub write_baseline: Option<PathBuf>,
}

/// What `lint` does with a debt baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineMode {
    /// No baseline: report every finding.
    None,
    /// Accept the findings the file records, relative to the root.
    Apply(PathBuf),
    /// Record today's findings in the file, relative to the root.
    Write(PathBuf),
}

impl LintArgs {
    /// The baseline flags as one mode. Clap rejects both flags together, so at most one is set.
    #[must_use]
    pub fn baseline_mode(&self) -> BaselineMode {
        match (&self.write_baseline, &self.baseline) {
            (Some(file), _) => BaselineMode::Write(file.clone()),
            (None, Some(file)) => BaselineMode::Apply(file.clone()),
            (None, None) => BaselineMode::None,
        }
    }
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

/// Arguments of `driftwall init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Repository root to baseline.
    #[arg(value_name = "PATH", default_value = DEFAULT_PATH)]
    pub path: PathBuf,
    /// Directory depth at which nodes are cut [default: chosen from the layout].
    #[arg(long, value_name = "N")]
    pub depth: Option<usize>,
    /// Contract file to write, relative to PATH [default: architecture.mmd].
    #[arg(long, value_name = "FILE")]
    pub contract: Option<PathBuf>,
    /// Write the contract only, without `.arch` files.
    #[arg(long)]
    pub no_tags: bool,
    /// Write the mapping as `%% map` lines in the contract instead of `.arch` files.
    #[arg(long, conflicts_with = "no_tags")]
    pub map: bool,
    /// Overwrite an existing contract or `.arch` file.
    #[arg(long)]
    pub force: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn mode(flags: &[&str]) -> Result<BaselineMode, clap::Error> {
        let line = ["driftwall", "lint"]
            .into_iter()
            .chain(flags.iter().copied());
        Cli::try_parse_from(line).map(|cli| match cli.command {
            Command::Lint(args) => args.baseline_mode(),
            other => panic!("not lint: {other:?}"),
        })
    }

    #[test]
    fn no_flag_is_no_baseline() {
        assert_eq!(mode(&[]).unwrap(), BaselineMode::None);
    }

    #[test]
    fn each_flag_names_its_mode() {
        assert_eq!(
            mode(&["--baseline", "debt.txt"]).unwrap(),
            BaselineMode::Apply(PathBuf::from("debt.txt"))
        );
        assert_eq!(
            mode(&["--write-baseline", "debt.txt"]).unwrap(),
            BaselineMode::Write(PathBuf::from("debt.txt"))
        );
    }

    #[test]
    fn both_flags_are_a_usage_error() {
        let error = mode(&["--baseline", "a", "--write-baseline", "b"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}
