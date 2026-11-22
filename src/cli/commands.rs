//! Runs a parsed command and returns what to print and how it ended.
//!
//! Nothing here prints or exits: [`Run`] carries the result back to `main`.

use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use driftwall::lint::{self, LintOptions};
use driftwall::report;

use crate::args::{Command, LintArgs, LintFormat};
use crate::outcome::Outcome;

/// What a command produced.
#[derive(Debug)]
pub struct Run {
    /// Text for stdout, printed exactly as given.
    pub stdout: String,
    /// How the command ended.
    pub outcome: Outcome,
}

/// Runs `command`.
///
/// # Errors
/// Any tool error: unreadable or invalid contract, IO failure, a root that is not a directory.
pub fn execute(command: &Command) -> Result<Run> {
    match command {
        Command::Lint(args) => run_lint(args),
    }
}

fn run_lint(args: &LintArgs) -> Result<Run> {
    require_directory(&args.path)?;
    let mut options = LintOptions::default();
    if let Some(contract) = &args.contract {
        options.contract.clone_from(contract);
    }
    let lint_report = lint::run(&args.path, &options)?;
    let stdout = match args.format {
        LintFormat::Text => report::render_text(&lint_report),
        LintFormat::Json => report::render_json(&lint_report)?,
        LintFormat::Sarif => report::render_sarif(&lint_report)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::of_lint(&lint_report, args.strict),
    })
}

/// Fails unless `path` exists and is a directory.
fn require_directory(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).map_err(|error| anyhow!("cannot access {}: {error}", path.display()))?;
    if !metadata.is_dir() {
        bail!("{} is not a directory", path.display());
    }
    Ok(())
}
