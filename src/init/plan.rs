//! The pure core of `init`: from the repository's files and imports to a contract plus tags.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::imports::FileImports;
use crate::model::{Contract, Edge, EdgeKind, Language, NodeId, NodeInfo};

/// Words the contract format rejects as node ids.
const RESERVED: [&str; 3] = ["end", "flowchart", "subgraph"];
/// Id of the node that owns files sitting directly in the repository root.
const ROOT_ID: &str = "root";
/// Label of the repository root directory.
const ROOT_LABEL: &str = ".";

/// Settings for [`plan`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InitOptions {
    /// Directory depth, counted from the repository root, at which nodes are cut. `None` picks it
    /// automatically (see `docs/DESIGN.md`, section "init").
    pub depth: Option<usize>,
}

/// What `init` intends to write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitPlan {
    /// The baseline contract: one node per owning directory, a solid edge per observed import.
    pub contract: Contract,
    /// Repository-relative directory (empty for the root) to the node its `.arch` tags it with.
    pub tags: BTreeMap<PathBuf, NodeId>,
}

/// Builds the baseline for `files` and the `imports` found in them.
///
/// Every source file is owned by its ancestor directory at the chosen depth, or by its own
/// directory when that is shallower. Imports of or from files outside `files` are ignored.
#[must_use]
pub fn plan(
    files: &BTreeMap<PathBuf, Language>,
    imports: &BTreeMap<PathBuf, FileImports>,
    options: &InitOptions,
) -> InitPlan {
    let source_dirs: BTreeSet<PathBuf> = files.keys().map(|file| parent_of(file)).collect();
    let depth = options
        .depth
        .unwrap_or_else(|| automatic_depth(&source_dirs));

    let owner_dir: BTreeMap<&PathBuf, PathBuf> = files
        .keys()
        .map(|file| (file, truncate(&parent_of(file), depth)))
        .collect();
    let node_dirs: BTreeSet<&PathBuf> = owner_dir.values().collect();
    let ids = node_ids(&node_dirs);

    let mut contract = Contract::default();
    for (dir, id) in &ids {
        contract.nodes.insert(
            id.clone(),
            NodeInfo {
                label: Some(label_of(dir)),
                boundary: None,
            },
        );
    }

    let owner_of = |file: &PathBuf| owner_dir.get(file).and_then(|dir| ids.get(dir));
    for (file, file_imports) in imports {
        let Some(from) = owner_of(file) else {
            continue;
        };
        for import in &file_imports.resolved {
            let Some(to) = owner_of(&import.target) else {
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
        tags: ids,
    }
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

/// Chooses the depth that cuts nodes just below the first directory that is not a pure corridor.
///
/// From the root, descend while the current directory holds no source file itself and exactly one
/// of its child directories holds sources. The stopping directory has own files or two or more
/// source-bearing branches; nodes are its children, and its own files belong to it.
fn automatic_depth(source_dirs: &BTreeSet<PathBuf>) -> usize {
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
            _ => return component_count(&prefix) + 1,
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
