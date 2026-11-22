//! Runs a parsed command and returns what to print and how it ended.
//!
//! Nothing here prints or exits: [`Run`] carries the result back to `main`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use driftwall::init::{self, InitOptions, InitSummary, WriteOptions};
use driftwall::lint::{self, LintOptions};
use driftwall::model::BlindSummary;
use driftwall::report;

use crate::args::{Command, InitArgs, LintArgs, LintFormat};
use crate::outcome::Outcome;
use crate::{diff, locate, views};

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
        Command::Init(args) => run_init(args),
        Command::Where(args) => locate::run(args),
        Command::Brief(args) => views::brief(args),
        Command::Neighbors(args) => views::neighbors(args),
        Command::Diff(args) => diff::run(args),
    }
}

/// The lint options for an optional `--contract` argument.
pub fn lint_options(contract: Option<&PathBuf>) -> LintOptions {
    let mut options = LintOptions::default();
    if let Some(contract) = contract {
        options.contract.clone_from(contract);
    }
    options
}

fn run_lint(args: &LintArgs) -> Result<Run> {
    require_directory(&args.path)?;
    let options = lint_options(args.contract.as_ref());
    let observation = lint::observe(&args.path, &options)?;
    let lint_report = &observation.report;
    let blind = observation.blind_spots.summary();
    let stdout = match args.format {
        LintFormat::Text => with_note(report::render_text(lint_report), &blind),
        LintFormat::Json => report::render_json(lint_report, &blind)?,
        LintFormat::Sarif => report::render_sarif(lint_report)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::of_lint(lint_report, args.strict),
    })
}

fn run_init(args: &InitArgs) -> Result<Run> {
    require_directory(&args.path)?;
    let options = InitOptions { depth: args.depth };
    let mut write = WriteOptions {
        write_tags: !args.no_tags,
        force: args.force,
        ..WriteOptions::default()
    };
    if let Some(contract) = &args.contract {
        write.contract.clone_from(contract);
    }
    let summary = init::run(&args.path, &options, &write)?;
    Ok(Run {
        stdout: with_note(render_init_summary(&summary), &summary.blind_spots),
        outcome: Outcome::Clean,
    })
}

/// `text` followed by the blind spots note line, when there is one.
fn with_note(mut text: String, blind: &BlindSummary) -> String {
    if let Some(note) = report::render_blind_note(blind) {
        text.push_str(&note);
        text.push('\n');
    }
    text
}

/// `driftwall: wrote <contract> (<n> nodes, <m> edges) and <k> .arch files`.
fn render_init_summary(summary: &InitSummary) -> String {
    let tags = match summary.tag_files {
        0 => "no .arch files".to_owned(),
        count => format!("{count} .arch files"),
    };
    format!(
        "driftwall: wrote {} ({} nodes, {} edges) and {tags}\n",
        summary.contract.display(),
        summary.nodes,
        summary.edges,
    )
}

/// Fails unless `path` exists and is a directory.
///
/// # Errors
/// The path cannot be inspected or is not a directory.
pub fn require_directory(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).map_err(|error| anyhow!("cannot access {}: {error}", path.display()))?;
    if !metadata.is_dir() {
        bail!("{} is not a directory", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(nodes: usize, edges: usize, tag_files: usize) -> InitSummary {
        InitSummary {
            nodes,
            edges,
            tag_files,
            contract: "architecture.mmd".into(),
            blind_spots: BlindSummary::default(),
        }
    }

    #[test]
    fn init_summary_names_contract_counts_and_tags() {
        assert_eq!(
            render_init_summary(&summary(3, 2, 3)),
            "driftwall: wrote architecture.mmd (3 nodes, 2 edges) and 3 .arch files\n"
        );
        assert_eq!(
            render_init_summary(&summary(1, 0, 0)),
            "driftwall: wrote architecture.mmd (1 nodes, 0 edges) and no .arch files\n"
        );
    }
}
