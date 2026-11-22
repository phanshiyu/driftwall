//! Rule 1 of `init`: the decisions existing `.arch` files already made.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::layout::label_of;
use crate::model::{NodeId, Ownership, Tag};

/// What the existing `.arch` files decided. `init` plans only for files they do not cover.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Existing {
    /// Every node an existing tag names, with the label the contract gives it. This includes
    /// nodes of tags whose directory holds no source file, which the linter still checks.
    pub nodes: BTreeMap<NodeId, String>,
    /// The owner of every source file an existing tag covers.
    pub owners: BTreeMap<PathBuf, NodeId>,
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
                Some(Tag::Modules) => label_of(&dir.join(name)),
                Some(Tag::Node(_)) | None => label_of(dir),
            };
            nodes.entry(id).or_insert(label);
        }
        Self {
            nodes,
            owners: ownership.owners.clone(),
        }
    }
}
