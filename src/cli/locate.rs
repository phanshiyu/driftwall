//! `driftwall where`: what a file is, which node owns it, and which rule decided it.

use anyhow::Result;
use driftwall::report;
use driftwall::views;

use crate::args::{WhereArgs, WhereFormat};
use crate::commands::{Run, require_directory};
use crate::outcome::Outcome;

/// Locates `args.file` under `args.root`.
///
/// # Errors
/// A root that is not a directory, a contract given with `--contract` that is unreadable or
/// invalid, a path outside the root or that does not exist, or an unreadable or invalid `.arch`.
pub fn run(args: &WhereArgs) -> Result<Run> {
    require_directory(&args.root)?;
    let answer = views::locate(&args.root, &args.file, args.contract.as_deref())?;
    let stdout = match args.format {
        WhereFormat::Text => report::render_where_text(&answer),
        WhereFormat::Json => report::render_where_json(&answer)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::of_where(&answer.located),
    })
}
