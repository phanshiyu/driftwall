//! Generates a baseline contract and `.arch` tags from an existing repository.
//!
//! The pure core is [`plan`]: files and imports in, a contract and tag map out. [`run`] is the
//! shell that discovers files, extracts imports and writes the result. A repository initialised
//! this way lints clean, because the contract is the observed import graph.

mod claims;
mod existing;
mod ids;
mod layout;
mod plan;
mod units;
mod write;

#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::diagram::DiagramError;
use crate::imports::{self, ImportsError};
use crate::model::{DEFAULT_CONTRACT, Language};
use crate::ownership::{self, OwnershipError};

pub use existing::Existing;
pub use plan::{InitOptions, InitPlan, plan};
pub use units::{Unit, UnitKind, detect_units};

/// Where and what [`run`] writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// Path of the contract, relative to the repository root.
    pub contract: PathBuf,
    /// Whether to write the `.arch` tags. Without them every file is unmapped.
    pub write_tags: bool,
    /// Overwrite an existing contract or `.arch` file instead of refusing.
    pub force: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            contract: PathBuf::from(DEFAULT_CONTRACT),
            write_tags: true,
            force: false,
        }
    }
}

/// What [`run`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitSummary {
    /// Nodes in the contract.
    pub nodes: usize,
    /// Edges in the contract.
    pub edges: usize,
    /// `.arch` files written (zero without tags).
    pub tag_files: usize,
    /// The contract path, as given in the options.
    pub contract: PathBuf,
}

/// Everything that can stop [`run`]. Nothing is written when it fails before the write step.
#[derive(Debug, Error)]
pub enum InitError {
    /// A file could not be written or inspected.
    #[error("cannot write {}", path.display())]
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// Scanning ownership failed.
    #[error(transparent)]
    Ownership(#[from] OwnershipError),
    /// Extracting imports failed.
    #[error(transparent)]
    Imports(#[from] ImportsError),
    /// Files that would be overwritten already exist.
    #[error("refusing to overwrite existing files (use --force): {}", list(paths))]
    Conflict {
        /// Every existing path `init` would write, sorted.
        paths: Vec<PathBuf>,
    },
    /// The repository has no supported source files, so there is nothing to baseline.
    #[error("no supported source files found")]
    NoSources,
    /// The rendered contract does not parse.
    #[error("generated contract is invalid")]
    Diagram(#[from] DiagramError),
    /// The rendered contract parses to something other than the plan.
    #[error("generated contract does not round-trip through the diagram parser")]
    RoundTrip,
}

fn list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Baselines the repository at `root`: writes the contract and, unless disabled, the tags.
///
/// All conflicts are checked before anything is written, so a refusal leaves `root` untouched.
///
/// # Errors
/// Fails with [`InitError::NoSources`] on a repository without source files,
/// [`InitError::Conflict`] when it would clobber existing files, and the scan, extraction and IO
/// errors otherwise.
pub fn run(
    root: &Path,
    options: &InitOptions,
    write: &WriteOptions,
) -> Result<InitSummary, InitError> {
    let ownership = ownership::scan(root)?;
    // why: tagged files are known files too: they resolve as import targets and get edges.
    let files: BTreeMap<PathBuf, Language> = ownership
        .owners
        .keys()
        .chain(&ownership.unmapped)
        .filter_map(|path| Some((path.clone(), ownership::language_of(path)?)))
        .collect();
    if files.is_empty() {
        return Err(InitError::NoSources);
    }
    let imports = imports::collect(root, &files)?;
    let units = detect_units(root, &files);
    let baseline = plan(&files, &imports, options, &units, &Existing::of(&ownership));
    write::apply(root, &baseline, write)
}
