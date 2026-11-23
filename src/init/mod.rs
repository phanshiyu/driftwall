//! Generates a baseline contract and `.arch` tags from an existing repository.
//!
//! The pure core is [`plan`]: files and imports in, a contract and tag map out. [`run`] is the
//! shell that discovers files, extracts imports and writes the result. A repository initialised
//! this way lints clean, because the contract is the observed import graph.

mod boundaries;
mod claims;
mod describe;
mod existing;
mod ids;
mod layout;
mod maps;
mod plan;
mod present;
mod split;
mod units;
mod write;

#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod scope_tests;
#[cfg(test)]
mod split_tests;
#[cfg(test)]
mod swift_tests;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::diagram::DiagramError;
use crate::imports::{self, ImportsError};
use crate::model::{BlindSpots, BlindSummary, DEFAULT_CONTRACT, Language, SkipReason};
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
    /// Write the mapping as `%% map` lines in the contract instead of `.arch` files. Wins over
    /// `write_tags`.
    pub write_map: bool,
    /// Overwrite an existing contract or `.arch` file instead of refusing.
    pub force: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            contract: PathBuf::from(DEFAULT_CONTRACT),
            write_tags: true,
            write_map: false,
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
    /// `%% map` lines written (zero without `--map`).
    pub map_lines: usize,
    /// The contract path, as given in the options.
    pub contract: PathBuf,
    /// What the scan could not see, as counts: the same census `lint` reports.
    pub blind_spots: BlindSummary,
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
    /// A tag has no `%% map` form.
    #[error("the tag in {}/.arch has no `%% map` form: `node: *` with `scope: here`", dir.display())]
    MapUnsupported {
        /// The directory of the tag.
        dir: PathBuf,
    },
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
        .filter_map(|path| Some((path.clone(), ownership::language_in(root, path)?)))
        .collect();
    if files.is_empty() {
        return Err(InitError::NoSources);
    }
    // why: assets reach import resolution as in `lint`, so edges into JSON or CSS files match.
    let assets: BTreeSet<PathBuf> = ownership
        .asset_owners
        .keys()
        .chain(&ownership.asset_unmapped)
        .cloned()
        .collect();
    let excluded: BTreeSet<PathBuf> = ownership.excluded.values().flatten().cloned().collect();
    // why: a folded entry file is not source to plan, but an import can still name it.
    let known_unread: BTreeSet<PathBuf> = excluded.union(&ownership.folded).cloned().collect();
    let mut imports = imports::collect_with_assets(
        root,
        &files,
        &assets,
        &ownership.asset_unmapped,
        &known_unread,
    )?;
    let units = detect_units(root, &files);
    let trivial = ownership::trivial_files(root, files.keys().map(PathBuf::as_path));
    let existing = Existing::of(&ownership).with_trivial(trivial);
    let mut baseline = plan(
        &files, &assets, &excluded, &imports, options, &units, &existing,
    );
    // why: an embed of an asset records an edge only when a tag covers it, and the tags are known only now, so the imports are read again with the assets the planned tags leave unowned.
    let mut all_tags = existing.tags.clone();
    all_tags.extend(baseline.tags.clone());
    let unowned: BTreeSet<PathBuf> = assets
        .iter()
        .filter(|asset| ownership::owner_in(&all_tags, asset).is_none())
        .cloned()
        .collect();
    let embeds_skipped = imports
        .values()
        .flat_map(|found| &found.skipped)
        .any(|skip| skip.reason == SkipReason::EmbedNonSource);
    if embeds_skipped && unowned != ownership.asset_unmapped {
        imports = imports::collect_with_assets(root, &files, &assets, &unowned, &known_unread)?;
        baseline = plan(
            &files, &assets, &excluded, &imports, options, &units, &existing,
        );
    }
    let every_unit: Vec<Unit> = units
        .iter()
        .cloned()
        .chain(units::python_units(files.keys()))
        .collect();
    let mut all_tags = existing.tags;
    all_tags.extend(baseline.tags.clone());
    for (id, text) in describe::describe(root, &files, &baseline.owners, &all_tags, &every_unit) {
        if let Some(info) = baseline.contract.nodes.get_mut(&id) {
            info.label = Some(format!("{id}: {text}"));
        }
    }
    boundaries::assign(
        &mut baseline.contract,
        &baseline.owners,
        &all_tags,
        &every_unit,
    );
    let mut summary = write::apply(root, &baseline, &all_tags, write)?;
    // why: init has no owners or line counts to give, and the note shows neither.
    summary.blind_spots = BlindSpots::build(
        &ownership,
        |_| None,
        |_| None,
        imports.values().flat_map(|found| &found.skipped),
        imports.values().flat_map(|found| &found.unresolved),
    )
    .summary();
    Ok(summary)
}
