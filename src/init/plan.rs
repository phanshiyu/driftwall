//! The pure core of `init`: from the repository's files and imports to a contract plus tags.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::imports::FileImports;
use crate::model::{Contract, Edge, EdgeKind, Language, NodeId, NodeInfo};
use crate::ownership::{Tag, module_name};

/// Words the contract format rejects as node ids.
const RESERVED: [&str; 3] = ["end", "flowchart", "subgraph"];
/// Id of the node that owns files sitting directly in the repository root.
const ROOT_ID: &str = "root";
/// Label of the repository root directory.
const ROOT_LABEL: &str = ".";

/// Settings for [`plan`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InitOptions {
    /// Directory depth, counted from the repository root, at which directory nodes are cut. `None`
    /// picks it automatically, and may then choose module nodes (see `docs/DESIGN.md`, "init").
    pub depth: Option<usize>,
}

/// What `init` intends to write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitPlan {
    /// The baseline contract: one node per owning directory (or per module under `node: *`), a
    /// solid edge per observed import.
    pub contract: Contract,
    /// Repository-relative directory (empty for the root) to the tag its `.arch` gets: a node id,
    /// or `node: *` when the nodes are derived from module names.
    pub tags: BTreeMap<PathBuf, Tag>,
}

/// How the files are cut into nodes: what the tags say and which node each file ends up in.
struct Layout {
    tags: BTreeMap<PathBuf, Tag>,
    labels: BTreeMap<NodeId, String>,
    owners: BTreeMap<PathBuf, NodeId>,
}

/// Builds the baseline for `files` and the `imports` found in them.
///
/// With an explicit depth, every source file is owned by its ancestor directory at that depth, or
/// by its own directory when that is shallower. Without one, the depth is chosen from the layout
/// and, when the directory it descends to directly holds two or more modules, that directory is
/// tagged `node: *` and each module and subdirectory becomes a node instead. Imports of or from
/// files outside `files` are ignored.
#[must_use]
pub fn plan(
    files: &BTreeMap<PathBuf, Language>,
    imports: &BTreeMap<PathBuf, FileImports>,
    options: &InitOptions,
) -> InitPlan {
    let layout = options
        .depth
        .map_or_else(|| automatic(files), |depth| by_directory(files, depth));

    let mut contract = Contract::default();
    for (id, label) in layout.labels {
        contract.nodes.insert(
            id,
            NodeInfo {
                label: Some(label),
                boundary: None,
            },
        );
    }

    for (file, file_imports) in imports {
        let Some(from) = layout.owners.get(file) else {
            continue;
        };
        for import in &file_imports.resolved {
            let Some(to) = layout.owners.get(&import.target) else {
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

    InitPlan {
        contract,
        tags: layout.tags,
    }
}

/// The layout chosen from the files alone: module nodes at the fork when it directly holds two or
/// more modules, otherwise directory nodes just below the fork.
fn automatic(files: &BTreeMap<PathBuf, Language>) -> Layout {
    let source_dirs: BTreeSet<PathBuf> = files.keys().map(|file| parent_of(file)).collect();
    let fork = automatic_fork(&source_dirs);
    by_modules(files, &fork).unwrap_or_else(|| by_directory(files, component_count(&fork) + 1))
}

/// One node per owning directory: the ancestor of each file at `depth`, or its own directory.
fn by_directory(files: &BTreeMap<PathBuf, Language>, depth: usize) -> Layout {
    let owner_dir: BTreeMap<&PathBuf, PathBuf> = files
        .keys()
        .map(|file| (file, truncate(&parent_of(file), depth)))
        .collect();
    let node_dirs: BTreeSet<&PathBuf> = owner_dir.values().collect();
    let ids = node_ids(&node_dirs);
    Layout {
        labels: ids
            .iter()
            .map(|(dir, id)| (id.clone(), label_of(dir)))
            .collect(),
        owners: owner_dir
            .into_iter()
            .filter_map(|(file, dir)| Some((file.clone(), ids.get(&dir)?.clone())))
            .collect(),
        tags: ids
            .into_iter()
            .map(|(dir, id)| (dir, Tag::Node(id)))
            .collect(),
    }
}

/// One node per module of `fork`, which is tagged `node: *`, so the ids are exactly the ones the
/// linter derives. `None` when that is not worth it or not possible: fewer than two modules sit
/// directly in `fork`, or some file would derive an id the contract cannot declare, in which case
/// the caller falls back to directory nodes rather than write a baseline that does not lint.
fn by_modules(files: &BTreeMap<PathBuf, Language>, fork: &Path) -> Option<Layout> {
    let mut owners = BTreeMap::new();
    let mut direct: BTreeSet<&NodeId> = BTreeSet::new();
    for file in files.keys() {
        let name = module_name(fork, file)?;
        let id = NodeId::new(&name)
            .ok()
            .filter(|id| !RESERVED.contains(&id.as_str()))?;
        owners.insert(file.clone(), id);
    }
    for (file, id) in &owners {
        if file.parent() == Some(fork) {
            direct.insert(id);
        }
    }
    if direct.len() < 2 {
        return None;
    }
    let labels = owners
        .values()
        .map(|id| (id.clone(), label_of(&fork.join(id.as_str()))))
        .collect();
    Some(Layout {
        tags: BTreeMap::from([(fork.to_path_buf(), Tag::Modules)]),
        labels,
        owners,
    })
}

fn parent_of(file: &Path) -> PathBuf {
    file.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The first `depth` components of `dir`, or all of them when it is shallower.
fn truncate(dir: &Path, depth: usize) -> PathBuf {
    dir.components().take(depth).collect()
}

fn component_count(dir: &Path) -> usize {
    dir.components().count()
}

/// Finds the directory where the layout forks, descending from the root through pure corridors.
///
/// From the root, descend while the current directory holds no source file itself and exactly one
/// of its child directories holds sources. The stopping directory has own files or two or more
/// source-bearing branches; nodes are cut just below it, and its own files belong to it.
fn automatic_fork(source_dirs: &BTreeSet<PathBuf>) -> PathBuf {
    let mut prefix = PathBuf::new();
    loop {
        let children: BTreeSet<OsString> = source_dirs
            .iter()
            .filter_map(|dir| dir.strip_prefix(&prefix).ok()?.components().next())
            .map(Component::as_os_str)
            .map(OsString::from)
            .collect();
        let has_own_files = source_dirs.contains(&prefix);
        let mut branches = children.into_iter();
        match (has_own_files, branches.next(), branches.next()) {
            (false, Some(only), None) => prefix.push(only),
            _ => return prefix,
        }
    }
}

fn label_of(dir: &Path) -> String {
    let parts: Vec<String> = dir
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return ROOT_LABEL.to_owned();
    }
    parts
        .join("/")
        .chars()
        .map(|c| {
            if matches!(c, '"' | '\n' | '\r') {
                '\''
            } else {
                c
            }
        })
        .collect()
}

/// Assigns every node directory a unique, valid id, in sorted directory order.
fn node_ids(node_dirs: &BTreeSet<&PathBuf>) -> BTreeMap<PathBuf, NodeId> {
    let common = common_prefix(node_dirs);
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut ids = BTreeMap::new();
    for dir in node_dirs {
        let base = base_name(dir, &common);
        let chosen = (1..=taken.len() + RESERVED.len() + 1)
            .map(|attempt| {
                if attempt == 1 {
                    base.clone()
                } else {
                    format!("{base}-{attempt}")
                }
            })
            .find(|candidate| !RESERVED.contains(&candidate.as_str()) && !taken.contains(candidate))
            .unwrap_or_else(|| base.clone());
        if let Ok(id) = NodeId::new(&chosen) {
            taken.insert(chosen);
            ids.insert((*dir).clone(), id);
        }
    }
    ids
}

/// The longest directory every node directory lies in (or is).
fn common_prefix(dirs: &BTreeSet<&PathBuf>) -> PathBuf {
    let mut iter = dirs.iter();
    let Some(first) = iter.next() else {
        return PathBuf::new();
    };
    let mut prefix: Vec<Component> = first.components().collect();
    for dir in iter {
        let shared = prefix
            .iter()
            .zip(dir.components())
            .take_while(|(a, b)| *a == b)
            .count();
        prefix.truncate(shared);
    }
    prefix.into_iter().collect()
}

/// The id a directory would get before collisions: its path below `common`, sanitised.
fn base_name(dir: &Path, common: &Path) -> String {
    let below = dir.strip_prefix(common).unwrap_or(dir);
    let source = if below.as_os_str().is_empty() {
        common.file_name().map(OsString::from).unwrap_or_default()
    } else {
        below
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("-")
            .into()
    };
    if dir.as_os_str().is_empty() {
        return ROOT_ID.to_owned();
    }
    sanitize(&source.to_string_lossy())
}

/// Turns arbitrary text into `[A-Za-z][A-Za-z0-9_-]*`: other characters become `-`, repeated and
/// edge dashes are dropped, and a non-letter start gets an `n-` prefix.
fn sanitize(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        let kept = if c.is_ascii_alphanumeric() || c == '_' {
            c
        } else {
            '-'
        };
        if kept == '-' && out.ends_with('-') {
            continue;
        }
        out.push(kept);
    }
    let trimmed = out.trim_matches('-');
    match trimmed.chars().next() {
        None => "n".to_owned(),
        Some(first) if first.is_ascii_alphabetic() => trimmed.to_owned(),
        Some(_) => format!("n-{trimmed}"),
    }
}
