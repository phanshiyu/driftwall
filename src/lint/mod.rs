//! Reconciles the code graph against the contract and produces violations.
//!
//! The pure core is [`reconcile`]: three in-memory views in, a [`LintReport`] out. [`run`] is the
//! thin shell that reads the contract, scans ownership, extracts imports and calls it. [`observe`]
//! is its sibling that also hands back what it read, for commands that present the architecture
//! ([`overview`], [`node_view`]) or compare contracts ([`contract_diff`]).

mod diff;
mod observe;
mod overview;
mod reconcile;
mod rules;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::diagram::{self, DiagramError};
use crate::imports::{self, ImportsError};
use crate::model::{
    Contract, DEFAULT_CONTRACT, Edge, Language, LintReport, Location, Ownership, Symbol,
};
use crate::ownership::{self, OwnershipError};

pub use diff::{EMPTY_CONTRACT, contract_diff};
pub use observe::{observed_edges, observed_symbols};
pub use overview::{node_view, overview};
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
///
/// A variant's message names only its own context. The cause is reachable through `source()`.
#[derive(Debug, Error)]
pub enum LintError {
    /// The contract file could not be read.
    #[error("cannot read {}", path.display())]
    Io {
        /// The contract path as given in the options.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The contract file is not valid.
    #[error("{}", path.display())]
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
    /// The old side of a contract comparison is not a valid contract.
    #[error("the old contract is not valid")]
    OldContract {
        /// What is wrong with it, including the line.
        source: DiagramError,
    },
    /// The new side of a contract comparison is not a valid contract.
    #[error("the new contract is not valid")]
    NewContract {
        /// What is wrong with it, including the line.
        source: DiagramError,
    },
}

/// Everything one scan of a repository found: the inputs of the lint and its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// The parsed contract.
    pub contract: Contract,
    /// Who owns which file.
    pub ownership: Ownership,
    /// Edges code realises with their import sites, declared or not.
    pub observed: BTreeMap<Edge, Vec<Location>>,
    /// The symbols the imports behind each observed edge use.
    pub symbols: BTreeMap<Edge, BTreeSet<Symbol>>,
    /// The lint result for the same scan.
    pub report: LintReport,
}

/// Lints the repository at `root` against the contract named by `options`.
///
/// # Errors
/// Fails when the contract cannot be read or parsed, or ownership or import extraction fails.
pub fn run(root: &Path, options: &LintOptions) -> Result<LintReport, LintError> {
    observe(root, options).map(|observation| observation.report)
}

/// Scans the repository at `root` once and returns what [`run`] would report together with the
/// contract, ownership and observed edges it was computed from.
///
/// # Errors
/// The same as [`run`].
pub fn observe(root: &Path, options: &LintOptions) -> Result<Observation, LintError> {
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

    let report = reconcile(&contract, &ownership, &imports, &options.contract);
    let observed = observed_edges(&contract, &ownership, &imports);
    let symbols = observed_symbols(&contract, &ownership, &imports);
    Ok(Observation {
        contract,
        ownership,
        observed,
        symbols,
        report,
    })
}
