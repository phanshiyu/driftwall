//! `driftwall diff`: what changed in the contract since a git ref.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use driftwall::lint::{self, EMPTY_CONTRACT};
use driftwall::model::{Baseline, DEFAULT_CONTRACT, DebtAdded, Moved};
use driftwall::report;
use tempfile::TempDir;

use crate::args::{DiffArgs, ViewFormat};
use crate::commands::{Run, parse_baseline, read_baseline, require_directory};
use crate::git;
use crate::outcome::Outcome;

/// Compares the contract at `args.base` with the one in the working tree.
///
/// A contract that does not exist at the ref counts as the empty contract, so everything in the
/// working-tree contract shows as added.
///
/// # Errors
/// A root that is not a directory, an unreadable working-tree contract, a git failure other
/// than the contract being absent at the ref, or a contract that does not parse.
pub fn run(args: &DiffArgs) -> Result<Run> {
    require_directory(&args.path)?;
    let contract = args
        .contract
        .clone()
        .unwrap_or_else(|| DEFAULT_CONTRACT.into());
    let new_src = fs::read_to_string(args.path.join(&contract))
        .with_context(|| format!("cannot read {}", contract.display()))?;
    let old_src = git::contract_at(&args.path, &args.base, &contract)?;
    let mut diff = lint::contract_diff(old_src.as_deref().unwrap_or(EMPTY_CONTRACT), &new_src)?;
    diff.moved = moved_files(args, &contract, old_src.is_some())?;
    let Some(file) = &args.baseline else {
        let stdout = match args.format {
            ViewFormat::Text => report::render_diff_text(&diff),
            ViewFormat::Json => report::render_diff_json(&diff)?,
        };
        return Ok(Run {
            stdout,
            outcome: Outcome::of_diff(&diff, 0, args.exit_code),
        });
    };
    let debt = debt_added(args, file)?;
    let stdout = match args.format {
        ViewFormat::Text => report::render_diff_text_with_debt(&diff, &debt),
        ViewFormat::Json => report::render_diff_json_with_debt(&diff, &debt)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::of_diff(&diff, debt.len(), args.exit_code),
    })
}

/// The source files whose owner at the base ref differs from the owner now. `declared` is whether
/// the contract exists at the ref; without it the old tree is owned by its `.arch` files alone.
fn moved_files(args: &DiffArgs, contract: &Path, declared: bool) -> Result<Vec<Moved>> {
    // why: the owner at the ref is whatever a lint of that tree would say, so the tree is scanned.
    let old_tree = TempDir::new().context("cannot create a temporary directory")?;
    git::export_tree(&args.path, &args.base, old_tree.path())?;
    let scratch = TempDir::new().context("cannot create a temporary directory")?;
    let renames = git::renames(&args.path, &args.base, scratch.path())?;
    let old = lint::owners(old_tree.path(), contract, declared)?;
    let new = lint::owners(&args.path, contract, true)?;
    Ok(lint::moved(&old, &new, &renames))
}

/// The entries of the working-tree baseline `file` that are new or larger than at the base ref.
/// A baseline absent at the ref counts as empty, so every entry is added.
fn debt_added(args: &DiffArgs, file: &Path) -> Result<Vec<DebtAdded>> {
    let new = read_baseline(&args.path.join(file), file)?;
    let old = match git::contract_at(&args.path, &args.base, file)? {
        Some(text) => parse_baseline(
            &text,
            &PathBuf::from(format!("{}:{}", args.base, file.display())),
        )?,
        None => Baseline::default(),
    };
    Ok(new.added_since(&old))
}
