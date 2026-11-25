//! `driftwall render`: a Mermaid picture of the contract, and the Markdown blocks that keep a copy
//! of it current.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use driftwall::lint;
use driftwall::model::{MarkdownSync, PictureView};

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
        stdout: lint::render_picture(&args.path, &options.contract, &view)?,
        outcome: Outcome::Clean,
    })
}

/// Syncs the blocks of the Markdown `file` with the contract.
fn sync(args: &RenderArgs, contract: &Path, file: &Path) -> Result<MarkdownSync> {
    let markdown =
        fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    lint::sync_markdown(&args.path, contract, &markdown)
        .with_context(|| format!("{}", file.display()))
}
