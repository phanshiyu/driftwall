//! Who owns a file under a given set of tags, without touching the file system.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::module_name;
use crate::model::{DirTag, NodeId, Scope, Tag};

/// The node `file` gets from `tags`, or `None` when no tag covers it.
///
/// `tags` maps a repository-relative directory (empty for the root) to the tag its `.arch` holds.
/// A name that a `node: *` tag derives for the file and that is not a valid node id gives `None`.
///
/// The rule is the one the scan applies: the nearest tag at or above the file's directory that
/// covers the file wins, where a `scope: here` tag covers only the files directly in its
/// directory. Pure, so a tag that has not been written yet can be asked about.
#[must_use]
pub fn owner_in(tags: &BTreeMap<PathBuf, DirTag>, file: &Path) -> Option<NodeId> {
    let (dir, found) = file
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .find_map(|dir| {
            let found = tags.get(dir)?;
            (found.scope == Scope::Subtree || file.parent() == Some(dir)).then_some((dir, found))
        })?;
    match &found.tag {
        Tag::Node(id) => Some(id.clone()),
        Tag::Modules(prefix) => prefix.derive(&module_name(dir, file, &found.peers)?).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Prefix;
    use std::collections::BTreeSet;

    fn tag(dir: &str, tag: Tag, scope: Scope) -> (PathBuf, DirTag) {
        (
            PathBuf::from(dir),
            DirTag {
                tag,
                scope,
                peers: BTreeSet::new(),
            },
        )
    }

    fn node(raw: &str) -> Tag {
        Tag::Node(NodeId::new(raw).unwrap())
    }

    fn owner(tags: &BTreeMap<PathBuf, DirTag>, file: &str) -> Option<String> {
        owner_in(tags, Path::new(file)).map(|id| id.to_string())
    }

    #[test]
    fn the_nearest_tag_wins_and_here_reaches_only_its_own_files() {
        let tags = BTreeMap::from([
            tag("", node("root"), Scope::Here),
            tag("a", node("outer"), Scope::Subtree),
            tag("a/b", node("inner"), Scope::Here),
            tag(
                "ui",
                Tag::Modules(Prefix::new("ui-").unwrap()),
                Scope::Subtree,
            ),
        ]);
        assert_eq!(owner(&tags, "top.ts").as_deref(), Some("root"));
        assert_eq!(owner(&tags, "x/y.ts"), None);
        assert_eq!(owner(&tags, "a/p.ts").as_deref(), Some("outer"));
        assert_eq!(owner(&tags, "a/b/q.ts").as_deref(), Some("inner"));
        assert_eq!(owner(&tags, "a/b/c/r.ts").as_deref(), Some("outer"));
        assert_eq!(owner(&tags, "ui/store.ts").as_deref(), Some("ui-store"));
        assert_eq!(owner(&tags, "ui/lib/x.json").as_deref(), Some("ui-lib"));
    }

    #[test]
    fn a_name_that_is_no_id_has_no_owner() {
        let tags = BTreeMap::from([tag("ui", Tag::Modules(Prefix::default()), Scope::Subtree)]);
        assert_eq!(owner(&tags, "ui/.eslintrc.js"), None);
    }
}
