//! `driftwall render`: a Mermaid picture of the contract, and the Markdown blocks that keep a copy
//! of it current.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use driftwall::diagram;
use driftwall::lint;
use driftwall::results::{MarkdownSync, PictureView};

use crate::args::RenderArgs;
use crate::commands::{Run, lint_options, require_directory};
use crate::outcome::Outcome;

/// Prints the picture, or updates or checks the blocks of a Markdown file.
///
/// # Errors
/// A root that is not a directory, an unreadable or invalid contract, an unreadable or unwritable
/// Markdown file, or a malformed block.
pub fn run(args: &RenderArgs) -> Result<Run> {
    require_directory(&args.path)?;
    let options = lint_options(args.contract.as_ref());
    if let Some(file) = &args.update {
        let synced = sync(args, &options.contract, file)?;
        if !synced.stale.is_empty() {
            fs::write(file, &synced.text)
                .with_context(|| format!("cannot write {}", file.display()))?;
        }
        return Ok(Run {
            stdout: format!(
                "driftwall: {}: {} block(s) rewritten\n",
                file.display(),
                synced.stale.len()
            ),
            outcome: Outcome::Clean,
        });
    }
    if let Some(file) = &args.check {
        let synced = sync(args, &options.contract, file)?;
        if synced.stale.is_empty() {
            return Ok(Run {
                stdout: format!("driftwall: {} is current\n", file.display()),
                outcome: Outcome::Clean,
            });
        }
        let stdout = synced.stale.iter().fold(String::new(), |mut text, line| {
            let name = file.display();
            let _ = writeln!(
                text,
                "{name}:{line}: stale driftwall render block; run `driftwall render --update {name}`"
            );
            text
        });
        return Ok(Run {
            stdout,
            outcome: Outcome::Findings,
        });
    }
    let view = match (args.depth, &args.focus) {
        (Some(depth), _) => PictureView::Depth(depth),
        (None, Some(boundary)) => PictureView::Focus(boundary.clone()),
        (None, None) => PictureView::Full,
    };
    Ok(Run {
        stdout: diagram::picture(
            &lint::read_contract(&args.path, &options.contract)?,
            &view,
            args.short_labels,
            &source_name(&options.contract),
        )?,
        outcome: Outcome::Clean,
    })
}

/// Syncs the blocks of the Markdown `file` with the contract.
fn sync(args: &RenderArgs, contract: &Path, file: &Path) -> Result<MarkdownSync> {
    let markdown =
        fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    let sync = || -> Result<MarkdownSync> {
        let parsed = lint::read_contract(&args.path, contract)?;
        Ok(diagram::sync(&markdown, &parsed, &source_name(contract))?)
    };
    sync().with_context(|| format!("{}", file.display()))
}

/// The name the picture comments give the contract: its file name.
fn source_name(contract: &Path) -> String {
    contract
        .file_name()
        .unwrap_or(contract.as_os_str())
        .to_string_lossy()
        .into_owned()
}
