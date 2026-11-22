//! Reconciles the code graph against the contract and produces violations.
//!
//! The pure core is [`reconcile`]: three in-memory views in, a [`LintReport`] out. [`run`] is the
//! thin shell that reads the contract, scans ownership, extracts imports and calls it.

mod reconcile;
mod rules;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::diagram::{self, DiagramError};
use crate::imports::{self, ImportsError};
use crate::model::{DEFAULT_CONTRACT, Language, LintReport};
use crate::ownership::{self, OwnershipError};

pub use reconcile::reconcile;

/// Settings for [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintOptions {
    /// Path of the contract, relative to the repository root.
    pub contract: PathBuf,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            contract: PathBuf::from(DEFAULT_CONTRACT),
        }
    }
}

/// Everything that can stop a lint from producing a report. A report with violations is not one.
#[derive(Debug, Error)]
pub enum LintError {
    /// The contract file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// The contract path as given in the options.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The contract file is not valid.
    #[error("{}: {source}", path.display())]
    Diagram {
        /// The contract path as given in the options.
        path: PathBuf,
        /// What is wrong with it, including the line.
        source: DiagramError,
    },
    /// Scanning ownership failed.
    #[error(transparent)]
    Ownership(#[from] OwnershipError),
    /// Extracting imports failed.
    #[error(transparent)]
    Imports(#[from] ImportsError),
}

/// Lints the repository at `root` against the contract named by `options`.
///
/// # Errors
/// Fails when the contract cannot be read or parsed, or ownership or import extraction fails.
pub fn run(root: &Path, options: &LintOptions) -> Result<LintReport, LintError> {
    let source =
        fs::read_to_string(root.join(&options.contract)).map_err(|source| LintError::Io {
            path: options.contract.clone(),
            source,
        })?;
    let contract = diagram::parse(&source).map_err(|source| LintError::Diagram {
        path: options.contract.clone(),
        source,
    })?;
    let ownership = ownership::scan(root)?;

    // why: files of unsupported languages are skipped here, so only extractable sources are read.
    let files: BTreeMap<PathBuf, Language> = ownership
        .owners
        .keys()
        .chain(&ownership.unmapped)
        .filter_map(|path| Some((path.clone(), ownership::language_of(path)?)))
        .collect();
    let imports = imports::collect(root, &files)?;

    Ok(reconcile(
        &contract,
        &ownership,
        &imports,
        &options.contract,
    ))
}
