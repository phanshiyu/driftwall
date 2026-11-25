//! The one place that picks how a repository's files get their owner: `.arch` tag files, or the
//! contract's `%% map` lines ("One mechanism per repository" in `docs/DESIGN.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::OwnershipError;
use super::locate::{locate, locate_mapped};
use super::lookup::owner_in;
use super::mapping::{MapContext, Mapper, scan_mapped};
use crate::model::{DirTag, Located, MapRule, NodeId, Ownership};

/// How the files of a repository get their owner.
#[derive(Debug, Clone, Copy)]
pub enum Mechanism<'a> {
    /// The contract has no `%% map` line, or there is no contract: the `.arch` tag files decide.
    Tags,
    /// The contract's `%% map` lines decide, and no `.arch` file is read.
    Maps(MapContext<'a>),
}

impl<'a> Mechanism<'a> {
    /// The mechanism of the contract at `contract` (relative to the root, for messages) whose
    /// `%% map` lines are `rules`: maps when there is at least one, tags otherwise.
    #[must_use]
    pub const fn of(contract: &'a Path, rules: &'a [MapRule]) -> Self {
        if rules.is_empty() {
            Self::Tags
        } else {
            Self::Maps(MapContext { contract, rules })
        }
    }

    /// Walks the repository at `root` and gives every file its owner.
    ///
    /// # Errors
    /// A file cannot be read, a `.arch` file is malformed, or two globs tie.
    pub fn scan(self, root: &Path) -> Result<Ownership, OwnershipError> {
        match self {
            Self::Tags => super::scan(root),
            Self::Maps(context) => scan_mapped(root, context),
        }
    }

    /// Where `file` (relative to `root`) belongs and why. A directory is answered as a file
    /// directly in it.
    ///
    /// # Errors
    /// The same as [`Mechanism::scan`], for the files that decide.
    pub fn locate(self, root: &Path, file: &Path) -> Result<Located, OwnershipError> {
        match self {
            Self::Tags => locate(root, file),
            Self::Maps(context) => locate_mapped(root, file, context),
        }
    }

    /// The owner of any path, existing or not, ready to answer many: the tags in `dirs`, or the
    /// `%% map` lines.
    #[must_use]
    pub fn owners(self) -> OwnerRule<'a> {
        match self {
            Self::Tags => OwnerRule::Tags,
            Self::Maps(context) => OwnerRule::Maps(Mapper::new(context)),
        }
    }
}

/// The owner rule of a [`Mechanism`], built once to answer many paths.
pub enum OwnerRule<'a> {
    /// The nearest `.arch` tag decides.
    Tags,
    /// The most specific `%% map` glob decides.
    Maps(Mapper<'a>),
}

impl OwnerRule<'_> {
    /// The node that owns `path`, given the `.arch` tags `dirs` a scan found (a map may defer to
    /// them for module targets).
    #[must_use]
    pub fn owner(&self, path: &Path, dirs: &BTreeMap<PathBuf, DirTag>) -> Option<NodeId> {
        match self {
            Self::Tags => owner_in(dirs, path),
            Self::Maps(mapper) => mapper.owner(path, dirs),
        }
    }
}
