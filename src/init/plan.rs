//! The pure core of `init`: from the repository's files and imports to a contract plus tags.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::claims::claim;
use super::existing::Existing;
use super::ids;
use super::layout::{Draft, Owner, label_of};
use super::units::{Unit, python_units};
use crate::imports::FileImports;
use crate::model::{Contract, DirTag, Edge, EdgeKind, Language, NodeId, NodeInfo, Scope, Tag};
use crate::ownership::owner_in;

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
    /// id, or `node: *` (with its prefix) when the nodes are derived from module names, and the
    /// scope. Existing tags are not here.
    pub tags: BTreeMap<PathBuf, DirTag>,
}

/// Builds the baseline for `files` and the `imports` found in them.
///
/// Files that `existing` covers keep their owner. For the others, with an explicit depth, every
/// file is owned by its ancestor directory at that depth, or by its own directory when that is
/// shallower. Without one, the code `units` decide first, then the depth is chosen from the layout
/// and, when the directory it descends to directly holds two or more modules, that directory is
/// tagged `node: *` and each module and subdirectory becomes a node instead. Imports of or from
/// files outside `files` and `assets` are ignored.
///
/// `assets` are the walked files that are not source, and `excluded` the source files the scan
/// left out, which imports resolve to like assets. Either has the owner the tags give it (the
/// existing ones and the ones planned here, see [`owner_in`]), and an import of one whose owner is
/// another declared node is an edge, exactly as `lint` counts it. The owner of every asset, and of
/// every excluded file that something imports, is declared as a node when no source names it (a
/// module of a `node: *` directory that holds nothing else): the baseline never makes `lint` record
/// an `undeclared-owner` reference of its own.
#[must_use]
pub fn plan(
    files: &BTreeMap<PathBuf, Language>,
    assets: &BTreeSet<PathBuf>,
    excluded: &BTreeSet<PathBuf>,
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
    let mut draft = Draft::new(
        existing.nodes.keys().cloned().collect(),
        files.keys().cloned().collect(),
    );
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
        draft.split_layers();
    }

    let directory_ids = ids::assign(
        &draft.directories,
        &draft.fixed,
        &draft.whole_paths,
        &draft.names,
    );
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

    let mut tags: BTreeMap<PathBuf, DirTag> = BTreeMap::new();
    for (dir, id) in &directory_ids {
        let scope = scope_of(dir, &draft);
        let tag = Tag::Node(id.clone());
        let peers = BTreeSet::new();
        tags.insert(dir.clone(), DirTag { tag, scope, peers });
    }
    for (dir, region) in std::mem::take(&mut draft.module_dirs) {
        let tag = Tag::Modules(region.prefix);
        tags.insert(
            dir,
            DirTag {
                tag,
                scope: Scope::Subtree,
                peers: region.peers,
            },
        );
    }

    let mut all_tags = existing.tags.clone();
    all_tags.extend(tags.clone());
    // why: an excluded file is real code only when something imports it, so only those get a declared owner.
    let imported: BTreeSet<&PathBuf> = imports
        .values()
        .flat_map(|found| &found.resolved)
        .map(|import| &import.target)
        .collect();
    let imported_excluded = excluded.iter().filter(|file| imported.contains(file));
    for asset in assets.iter().chain(imported_excluded) {
        if let Some(owner) = owner_in(&all_tags, asset) {
            // why: an owner no source file names must still be declared, or `lint` records the import as `undeclared-owner`.
            contract
                .nodes
                .entry(owner.clone())
                .or_insert_with(|| NodeInfo {
                    label: Some(asset_label(&all_tags, asset)),
                    boundary: None,
                });
            owners.insert(asset.clone(), owner);
        }
    }
    add_edges(&mut contract, imports, &owners);
    InitPlan { contract, tags }
}

/// The label of a node that only an asset names: the directory of the tag that owns `asset`, or
/// the subdirectory below it, or the asset file itself, when a `node: *` tag derived the node from
/// it. Every label names a path that exists.
fn asset_label(tags: &BTreeMap<PathBuf, DirTag>, asset: &Path) -> String {
    let tagged = asset
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .find(|dir| tags.contains_key(*dir))
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let below = asset.strip_prefix(&tagged).unwrap_or(asset);
    let first = below
        .components()
        .next()
        .map(std::path::Component::as_os_str);
    match (tags.get(&tagged).map(|found| &found.tag), first) {
        (Some(Tag::Modules(_)), Some(first)) if below.components().count() > 1 => {
            label_of(&tagged.join(first))
        }
        (Some(Tag::Modules(_)), _) => label_of(asset),
        _ => label_of(&tagged),
    }
}

/// The scope of the directory node at `dir`: only a group (the repository root, or the files
/// directly in a unit directory) is limited to its own directory, and only when every file the
/// draft gives it sits directly in it.
fn scope_of(dir: &Path, draft: &Draft) -> Scope {
    let is_group = dir.as_os_str().is_empty() || draft.groups.contains(dir);
    let all_direct = draft
        .owners
        .iter()
        .filter(|(_, owner)| matches!(owner, Owner::Directory(found) if found == dir))
        .all(|(file, _)| file.parent() == Some(dir));
    if is_group && all_direct {
        Scope::Here
    } else {
        Scope::Subtree
    }
}

/// One solid edge per pair of different owners with an import between them. Both owners must be
/// declared in the contract, as `lint` requires.
fn add_edges(
    contract: &mut Contract,
    imports: &BTreeMap<PathBuf, FileImports>,
    owners: &BTreeMap<PathBuf, NodeId>,
) {
    let declared = |id: &&NodeId| contract.nodes.contains_key(*id);
    let mut found = BTreeSet::new();
    for (file, file_imports) in imports {
        let Some(from) = owners.get(file).filter(declared) else {
            continue;
        };
        for import in &file_imports.resolved {
            let Some(to) = owners.get(&import.target).filter(declared) else {
                continue;
            };
            if from != to {
                found.insert(Edge {
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
    }
    for edge in found {
        contract.edges.insert(edge, EdgeKind::Implemented);
    }
}
