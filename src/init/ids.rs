//! Ids for directory nodes: derived from the path, unique, valid, and never one that is taken.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::contract::NodeId;

/// Words the contract format rejects as node ids.
pub(super) const RESERVED: [&str; 3] = ["end", "flowchart", "subgraph"];
/// Id of the node that owns files sitting directly in the repository root.
const ROOT_ID: &str = "root";
/// What a directory node is called when its plain id is taken by a fixed node.
const DIRECTORY_SUFFIX: &str = "dir";

/// Assigns every directory a unique, valid id, in sorted directory order.
///
/// `fixed` are the ids that cannot change (module nodes and nodes named by existing tags). A
/// directory whose plain id is one of them becomes `<id>-dir`, then `<id>-dir-2`, ...; any other
/// collision, and the reserved words, get `-2`, `-3`, ...
///
/// The ids come from the path below the longest prefix the directories share, except for the
/// directories in `whole_paths` (those of units that have a prefix), which use their whole path so
/// that ids stay in step with the prefixes of unit modules. They do not count for the shared prefix.
pub(super) fn assign(
    dirs: &BTreeSet<PathBuf>,
    fixed: &BTreeSet<NodeId>,
    whole_paths: &BTreeSet<PathBuf>,
    names: &BTreeMap<PathBuf, String>,
) -> BTreeMap<PathBuf, NodeId> {
    let others: BTreeSet<PathBuf> = dirs.difference(whole_paths).cloned().collect();
    let common = common_prefix(&others);
    let fixed: BTreeSet<String> = fixed.iter().map(ToString::to_string).collect();
    let mut taken = fixed.clone();
    let mut ids = BTreeMap::new();
    for dir in dirs {
        let plain = names.get(dir).map_or_else(
            || {
                if whole_paths.contains(dir) {
                    base_name(dir, Path::new(""))
                } else {
                    base_name(dir, &common)
                }
            },
            |name| sanitize(name),
        );
        let family = if fixed.contains(&plain) {
            format!("{plain}-{DIRECTORY_SUFFIX}")
        } else {
            plain
        };
        let chosen = (1..=taken.len() + RESERVED.len() + 1)
            .map(|attempt| {
                if attempt == 1 {
                    family.clone()
                } else {
                    format!("{family}-{attempt}")
                }
            })
            .find(|candidate| {
                !RESERVED.contains(&candidate.as_str()) && !taken.contains(candidate)
            });
        if let Some(chosen) = chosen
            && let Ok(id) = NodeId::new(&chosen)
        {
            taken.insert(chosen);
            ids.insert(dir.clone(), id);
        }
    }
    ids
}

/// The longest directory every one of `dirs` lies in (or is).
fn common_prefix(dirs: &BTreeSet<PathBuf>) -> PathBuf {
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
    if dir.as_os_str().is_empty() {
        return ROOT_ID.to_owned();
    }
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
    sanitize(&source.to_string_lossy())
}

/// Turns arbitrary text into `[A-Za-z][A-Za-z0-9_-]*`: other characters become `-`, repeated and
/// edge dashes are dropped, and a non-letter start gets an `n-` prefix.
pub(super) fn sanitize(raw: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(paths: &[&str]) -> BTreeSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    fn fixed(names: &[&str]) -> BTreeSet<NodeId> {
        names.iter().map(|n| NodeId::new(n).unwrap()).collect()
    }

    fn ids(paths: &[&str], taken: &[&str]) -> Vec<String> {
        assign(
            &dirs(paths),
            &fixed(taken),
            &BTreeSet::new(),
            &BTreeMap::new(),
        )
        .values()
        .map(ToString::to_string)
        .collect()
    }

    #[test]
    fn a_directory_whose_id_is_fixed_gets_the_dir_suffix() {
        assert_eq!(ids(&["scripts", "ui"], &["ui"]), ["scripts", "ui-dir"]);
    }

    #[test]
    fn the_dir_suffix_is_numbered_when_it_is_taken_too() {
        assert_eq!(ids(&["ui"], &["ui", "ui-dir"]), ["ui-dir-2"]);
    }

    #[test]
    fn the_root_node_yields_to_a_module_called_root() {
        assert_eq!(ids(&["", "lib"], &["root"]), ["root-dir", "lib"]);
    }

    #[test]
    fn two_directories_with_the_same_sanitised_id_are_numbered() {
        assert_eq!(ids(&["x/my-pkg", "x/my.pkg"], &[]), ["my-pkg", "my-pkg-2"]);
    }
}
