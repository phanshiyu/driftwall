//! Groups the nodes of the draft into boundaries (subgraphs), from facts only: code units and
//! the directories nodes live in. See "The draft reads as a design" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::ids::{sanitize, undrawable};
use super::units::Unit;
use crate::code::{DirTag, Scope, Tag};
use crate::contract::{Contract, NodeId};

/// A unit with this many nodes or more is grouped by directory instead of as a whole.
const MANY_NODES: usize = 8;

/// What a boundary stands for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    /// A code unit, by its directory.
    Unit(PathBuf),
    /// The nodes that live in one directory.
    Home(PathBuf),
}

impl Key {
    fn dir(&self) -> &Path {
        match self {
            Self::Unit(dir) | Self::Home(dir) => dir,
        }
    }
}

/// Sets the boundary of every node of `contract` that belongs to a group.
///
/// `owners` is the owner of each file, `tags` every tag (existing and planned) and `units` the code
/// units of the repository.
pub(super) fn assign(
    contract: &mut Contract,
    owners: &BTreeMap<PathBuf, NodeId>,
    tags: &BTreeMap<PathBuf, DirTag>,
    units: &[Unit],
) {
    let mut homes: BTreeMap<&NodeId, Vec<PathBuf>> = BTreeMap::new();
    let mut spans: BTreeMap<&NodeId, Vec<PathBuf>> = BTreeMap::new();
    for (file, id) in owners {
        if !contract.nodes.contains_key(id) {
            continue;
        }
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        spans.entry(id).or_default().push(dir);
        if let Some(home) = home_of(tags, file) {
            homes.entry(id).or_default().push(home);
        }
    }

    let unit_dirs: Vec<&Path> = units.iter().map(|unit| unit.dir.as_path()).collect();
    let unit_of: BTreeMap<&NodeId, &Path> = spans
        .iter()
        .filter_map(|(id, dirs)| {
            let span = common_ancestor(dirs)?;
            let unit = unit_dirs
                .iter()
                .filter(|dir| span.starts_with(dir))
                .max_by_key(|dir| dir.components().count())?;
            Some((*id, *unit))
        })
        .collect();
    let mut per_unit: BTreeMap<&Path, usize> = BTreeMap::new();
    for unit in unit_of.values() {
        *per_unit.entry(unit).or_default() += 1;
    }

    let mut groups: BTreeMap<Key, Vec<NodeId>> = BTreeMap::new();
    for id in contract.nodes.keys() {
        let key = match unit_of.get(id) {
            Some(unit) if per_unit.get(unit).copied().unwrap_or(0) < MANY_NODES => {
                Some(Key::Unit(unit.to_path_buf()))
            }
            _ => homes
                .get(id)
                .and_then(|dirs| common_ancestor(dirs))
                .map(Key::Home),
        };
        // why: the repository root is where everything else sits, so its group would only be the leftover.
        if let Some(key) = key.filter(|key| !key.dir().as_os_str().is_empty()) {
            groups.entry(key).or_default().push(id.clone());
        }
    }

    let total = contract.nodes.len();
    let mut taken: BTreeSet<String> = contract.nodes.keys().map(ToString::to_string).collect();
    for (key, members) in groups {
        // why: a boundary around one node, or around every node, says nothing.
        if members.len() < 2 || members.len() >= total {
            continue;
        }
        let name = free_name(&sanitize(&path_text(key.dir())), &taken);
        taken.insert(name.clone());
        for member in members {
            if let Some(info) = contract.nodes.get_mut(&member) {
                info.boundary = Some(name.clone());
            }
        }
    }
}

/// Whether Mermaid cannot parse `name` as a `subgraph` id: also `default` and `default-<x>`.
fn undrawable_boundary(name: &str) -> bool {
    undrawable(name) || name == "default" || name.starts_with("default-")
}

/// `base`, or `base-group`, `base-group-2`, ... when that is taken.
fn free_name(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) && !undrawable_boundary(base) {
        return base.to_owned();
    }
    // why: `end-group` would still start with a word Mermaid reads as syntax, so `_` joins it.
    let plain = if undrawable_boundary(&format!("{base}-group")) {
        format!("{}_group", base.replace('-', "_"))
    } else {
        format!("{base}-group")
    };
    if !taken.contains(&plain) {
        return plain;
    }
    (2..=taken.len() + 2)
        .map(|n| format!("{plain}-{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or(plain)
}

fn path_text(dir: &Path) -> String {
    dir.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// The directory a file's node lives in: the tag directory of a module of a `node: *` tag, and the
/// parent of the tag directory for any other node.
fn home_of(tags: &BTreeMap<PathBuf, DirTag>, file: &Path) -> Option<PathBuf> {
    let (dir, found) = file
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .find_map(|dir| {
            let found = tags.get(dir)?;
            (found.scope == Scope::Subtree || file.parent() == Some(dir)).then_some((dir, found))
        })?;
    Some(match found.tag {
        Tag::Modules(_) => dir.to_path_buf(),
        Tag::Node(_) => dir.parent().map(Path::to_path_buf).unwrap_or_default(),
    })
}

/// The longest directory every one of `dirs` lies in. `None` for no directory.
pub(super) fn common_ancestor(dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut all = dirs.iter();
    let first = all.next()?;
    Some(all.fold(first.clone(), |common, dir| {
        common
            .components()
            .zip(dir.components())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a)
            .collect()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NodeInfo, Prefix};
    use crate::init::units::UnitKind;

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn contract(names: &[&str]) -> Contract {
        let mut out = Contract::default();
        for name in names {
            out.nodes.insert(
                id(name),
                NodeInfo {
                    label: None,
                    boundary: None,
                    outer: Vec::new(),
                },
            );
        }
        out
    }

    fn owners(pairs: &[(&str, &str)]) -> BTreeMap<PathBuf, NodeId> {
        pairs
            .iter()
            .map(|(file, node)| (PathBuf::from(file), id(node)))
            .collect()
    }

    fn tag(dir: &str, tag: Tag) -> (PathBuf, DirTag) {
        (
            PathBuf::from(dir),
            DirTag {
                tag,
                scope: Scope::Subtree,
                peers: BTreeSet::new(),
            },
        )
    }

    fn unit(dir: &str) -> Unit {
        Unit {
            dir: PathBuf::from(dir),
            kind: UnitKind::Package,
            name: None,
        }
    }

    fn boundaries(contract: &Contract) -> Vec<(String, Option<String>)> {
        contract
            .nodes
            .iter()
            .map(|(id, info)| (id.to_string(), info.boundary.clone()))
            .collect()
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "reads as the expected boundary of a node"
    )]
    fn some(name: &str) -> Option<String> {
        Some(name.to_owned())
    }

    #[test]
    fn nodes_of_a_small_unit_share_a_boundary_named_after_it() {
        let mut c = contract(&["ui-a", "ui-b", "api", "tool"]);
        let files = owners(&[
            ("ui/src/a.ts", "ui-a"),
            ("ui/src/b.ts", "ui-b"),
            ("api/x.ts", "api"),
            ("tool/y.ts", "tool"),
        ]);
        let tags: BTreeMap<_, _> = [
            tag("ui/src", Tag::Modules(Prefix::default())),
            tag("api", Tag::Node(id("api"))),
            tag("tool", Tag::Node(id("tool"))),
        ]
        .into();
        assign(&mut c, &files, &tags, &[unit("ui"), unit("api")]);
        assert_eq!(
            boundaries(&c),
            [
                ("api".to_owned(), None),
                ("tool".to_owned(), None),
                ("ui-a".to_owned(), some("ui")),
                ("ui-b".to_owned(), some("ui")),
            ]
        );
    }

    #[test]
    fn a_boundary_with_one_node_or_around_every_node_is_not_emitted() {
        let mut c = contract(&["a", "b"]);
        let files = owners(&[("ui/a.ts", "a"), ("ui/b.ts", "b")]);
        let tags: BTreeMap<_, _> = [tag("ui", Tag::Modules(Prefix::default()))].into();
        assign(&mut c, &files, &tags, &[unit("ui")]);
        assert!(
            boundaries(&c)
                .iter()
                .all(|(_, boundary)| boundary.is_none())
        );
    }

    #[test]
    fn a_boundary_never_takes_a_node_id() {
        let mut c = contract(&["ui", "ui-a", "ui-b", "other"]);
        let files = owners(&[
            ("ui/a.ts", "ui-a"),
            ("ui/b.ts", "ui-b"),
            ("x/o.ts", "other"),
            ("x/ui.ts", "ui"),
        ]);
        let tags: BTreeMap<_, _> = [
            tag("ui", Tag::Modules(Prefix::default())),
            tag("x", Tag::Modules(Prefix::default())),
        ]
        .into();
        assign(&mut c, &files, &tags, &[unit("ui")]);
        let found: BTreeSet<Option<String>> = boundaries(&c)
            .into_iter()
            .map(|(_, boundary)| boundary)
            .collect();
        assert!(found.contains(&some("ui-group")), "{found:?}");
        assert!(!found.contains(&some("ui")), "{found:?}");
    }

    #[test]
    fn nodes_of_a_big_unit_are_grouped_by_the_directory_they_live_in() {
        let names: Vec<String> = (0..10).map(|n| format!("m{n}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut c = contract(&refs);
        let pairs: Vec<(String, String)> = names
            .iter()
            .enumerate()
            .map(|(n, name)| {
                let dir = if n < 5 { "app/src/web" } else { "app/src/core" };
                (format!("{dir}/{name}.ts"), name.clone())
            })
            .collect();
        let files: BTreeMap<PathBuf, NodeId> = pairs
            .iter()
            .map(|(file, node)| (PathBuf::from(file), id(node)))
            .collect();
        let tags: BTreeMap<_, _> = [
            tag("app/src/web", Tag::Modules(Prefix::default())),
            tag("app/src/core", Tag::Modules(Prefix::default())),
        ]
        .into();
        assign(&mut c, &files, &tags, &[unit("app")]);
        let web = boundaries(&c)
            .into_iter()
            .filter(|(_, b)| *b == some("app-src-web"))
            .count();
        let core = boundaries(&c)
            .into_iter()
            .filter(|(_, b)| *b == some("app-src-core"))
            .count();
        assert_eq!((web, core), (5, 5));
    }

    #[test]
    fn a_boundary_name_mermaid_cannot_draw_gets_a_group_spelling() {
        let taken = BTreeSet::new();
        assert_eq!(free_name("graph", &taken), "graph_group");
        assert_eq!(free_name("end", &taken), "end_group");
        assert_eq!(free_name("end-user", &taken), "end_user_group");
        assert_eq!(free_name("ui", &taken), "ui");
    }
}
