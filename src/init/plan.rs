//! The pure core of `init`: from the repository's files and imports to a contract plus tags.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::claims::claim;
use super::existing::Existing;
use super::ids;
use super::layout::{Draft, Owner, label_of};
use super::units::{Unit, python_units};
use crate::imports::FileImports;
use crate::model::{Contract, Edge, EdgeKind, Language, NodeId, NodeInfo};
use crate::ownership::Tag;

/// Settings for [`plan`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InitOptions {
    /// Directory depth, counted from the repository root, at which directory nodes are cut. `None`
    /// picks it automatically, and may then choose code units and module nodes (see
    /// `docs/DESIGN.md`, "init").
    pub depth: Option<usize>,
}

/// What `init` intends to write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitPlan {
    /// The baseline contract: the nodes of the existing tags and of the new ones, and a solid edge
    /// per observed import.
    pub contract: Contract,
    /// Repository-relative directory (empty for the root) to the tag its new `.arch` gets: a node
    /// id, or `node: *` when the nodes are derived from module names. Existing tags are not here.
    pub tags: BTreeMap<PathBuf, Tag>,
}

/// Builds the baseline for `files` and the `imports` found in them.
///
/// Files that `existing` covers keep their owner. For the others, with an explicit depth, every
/// file is owned by its ancestor directory at that depth, or by its own directory when that is
/// shallower. Without one, the code `units` decide first, then the depth is chosen from the layout
/// and, when the directory it descends to directly holds two or more modules, that directory is
/// tagged `node: *` and each module and subdirectory becomes a node instead. Imports of or from
/// files outside `files` are ignored.
#[must_use]
pub fn plan(
    files: &BTreeMap<PathBuf, Language>,
    imports: &BTreeMap<PathBuf, FileImports>,
    options: &InitOptions,
    units: &[Unit],
    existing: &Existing,
) -> InitPlan {
    let fresh: Vec<&Path> = files
        .keys()
        .filter(|file| !existing.owners.contains_key(*file))
        .map(PathBuf::as_path)
        .collect();
    let mut draft = Draft::new(existing.nodes.keys().cloned().collect());
    if let Some(depth) = options.depth {
        draft.cut_directories(&fresh, depth);
    } else {
        let every_unit: Vec<Unit> = units
            .iter()
            .cloned()
            .chain(python_units(files.keys()))
            .collect();
        let rest = claim(&mut draft, &fresh, &every_unit);
        if !rest.is_empty() {
            draft.cut_automatically(&rest);
        }
    }

    let directory_ids = ids::assign(&draft.directories, &draft.fixed);
    let mut owners = existing.owners.clone();
    for (file, owner) in &draft.owners {
        let id = match owner {
            Owner::Directory(dir) => directory_ids.get(dir).cloned(),
            Owner::Module(id) => Some(id.clone()),
        };
        if let Some(id) = id {
            owners.insert(file.clone(), id);
        }
    }

    let mut labels = existing.nodes.clone();
    labels.extend(draft.modules.clone());
    labels.extend(
        directory_ids
            .iter()
            .map(|(dir, id)| (id.clone(), label_of(dir))),
    );
    let mut contract = Contract::default();
    for (id, label) in labels {
        contract.nodes.insert(
            id,
            NodeInfo {
                label: Some(label),
                boundary: None,
            },
        );
    }
    add_edges(&mut contract, imports, &owners);

    let mut tags: BTreeMap<PathBuf, Tag> = directory_ids
        .into_iter()
        .map(|(dir, id)| (dir, Tag::Node(id)))
        .collect();
    tags.extend(draft.module_dirs.into_iter().map(|dir| (dir, Tag::Modules)));
    InitPlan { contract, tags }
}

/// One solid edge per pair of different owners with an import between them.
fn add_edges(
    contract: &mut Contract,
    imports: &BTreeMap<PathBuf, FileImports>,
    owners: &BTreeMap<PathBuf, NodeId>,
) {
    for (file, file_imports) in imports {
        let Some(from) = owners.get(file) else {
            continue;
        };
        for import in &file_imports.resolved {
            let Some(to) = owners.get(&import.target) else {
                continue;
            };
            if from != to {
                contract.edges.insert(
                    Edge {
                        from: from.clone(),
                        to: to.clone(),
                    },
                    EdgeKind::Implemented,
                );
            }
        }
    }
}
