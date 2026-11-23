//! Rule 1 of `init`: the decisions existing `.arch` files already made.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::layout::{label_of, module_label};
use crate::model::{DirTag, NodeId, Ownership, Tag};

/// What the existing `.arch` files decided. `init` plans only for files they do not cover.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Existing {
    /// Every node an existing tag names, with the label the contract gives it. This includes
    /// nodes of tags whose directory holds no source file, which the linter still checks.
    pub nodes: BTreeMap<NodeId, String>,
    /// The owner of every source file an existing tag covers.
    pub owners: BTreeMap<PathBuf, NodeId>,
    /// What each existing `.arch` file says, by its directory, so that the owner of a file that is
    /// not source (an asset) can be asked of the existing and the planned tags together.
    pub tags: BTreeMap<PathBuf, DirTag>,
    /// The source files that are entry files declaring nothing (see `ownership::trivial_files`),
    /// which `init` folds instead of giving them a module node. Set by [`Existing::with_trivial`].
    pub trivial: BTreeSet<PathBuf>,
}

impl Existing {
    /// The decisions recorded in a scan: owners as scanned, and a node per named tag, labelled
    /// with its directory (or `<directory>/<id>` for a module of a `node: *` directory).
    #[must_use]
    pub fn of(ownership: &Ownership) -> Self {
        let mut nodes = BTreeMap::new();
        for (at, name) in &ownership.tags {
            let Ok(id) = NodeId::new(name) else {
                continue;
            };
            let dir = at.file.parent().unwrap_or_else(|| Path::new(""));
            let label = match ownership.tag_dirs.get(dir) {
                Some(DirTag {
                    tag: Tag::Modules(prefix),
                    peers,
                    ..
                }) => module_label(
                    dir,
                    name.strip_prefix(prefix.as_str()).unwrap_or(name),
                    peers,
                ),
                _ => label_of(dir),
            };
            nodes.entry(id).or_insert(label);
        }
        Self {
            nodes,
            owners: ownership.owners.clone(),
            tags: ownership.tag_dirs.clone(),
            trivial: BTreeSet::new(),
        }
    }

    /// These decisions, with `trivial` as the entry files that declare nothing.
    #[must_use]
    pub fn with_trivial(mut self, trivial: BTreeSet<PathBuf>) -> Self {
        self.trivial = trivial;
        self
    }
}
