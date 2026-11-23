//! `init --map`: the tags `init` would write, and the existing ones, as `%% map` rules.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::InitError;
use crate::model::{DirTag, MapGlob, MapRule, MapTarget, Scope, Tag};

/// One rule per tag, sorted by directory. A tag on `d` is the glob `d/` (the whole subtree), or
/// `d/*` with `scope: here`; at the root they are `./` and `*`. A `node: <prefix>*` tag on `d` is
/// the module form on `d/`.
///
/// # Errors
/// [`InitError::MapUnsupported`] for a `node: *` tag with `scope: here`, which has no map form.
pub fn rules_of(tags: &BTreeMap<PathBuf, DirTag>) -> Result<Vec<MapRule>, InitError> {
    tags.iter()
        .map(|(dir, found)| {
            let path = dir.display().to_string();
            let unsupported = || InitError::MapUnsupported { dir: dir.clone() };
            let (text, target) = match (&found.tag, found.scope) {
                (Tag::Node(id), Scope::Subtree) => (subtree(&path), MapTarget::Node(id.clone())),
                (Tag::Node(id), Scope::Here) => (direct(&path), MapTarget::Node(id.clone())),
                (Tag::Modules(prefix), Scope::Subtree) => {
                    (subtree(&path), MapTarget::Modules(prefix.clone()))
                }
                (Tag::Modules(_), Scope::Here) => return Err(unsupported()),
            };
            let glob = MapGlob::parse(&text).map_err(|_| unsupported())?;
            Ok(MapRule {
                line: 0,
                target,
                globs: vec![glob],
            })
        })
        .collect()
}

fn subtree(dir: &str) -> String {
    if dir.is_empty() {
        "./".to_owned()
    } else {
        format!("{dir}/")
    }
}

fn direct(dir: &str) -> String {
    if dir.is_empty() {
        "*".to_owned()
    } else {
        format!("{dir}/*")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::model::{NodeId, Prefix};

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

    fn node(id: &str) -> Tag {
        Tag::Node(NodeId::new(id).unwrap())
    }

    fn shown(tags: &[(PathBuf, DirTag)]) -> Vec<String> {
        let map: BTreeMap<_, _> = tags.iter().cloned().collect();
        rules_of(&map)
            .unwrap()
            .iter()
            .map(|rule| format!("{}: {}", rule.target, rule.globs[0]))
            .collect()
    }

    #[test]
    fn subtree_here_module_and_root_tags_have_their_globs() {
        let tags = [
            tag("", node("root"), Scope::Here),
            tag("src/ui", node("ui"), Scope::Subtree),
            tag("lib", node("lib"), Scope::Here),
            tag(
                "src",
                Tag::Modules(Prefix::new("m-").unwrap()),
                Scope::Subtree,
            ),
        ];
        assert_eq!(
            shown(&tags),
            ["root: *", "lib: lib/*", "m-*: src/", "ui: src/ui/"]
        );
        assert_eq!(shown(&[tag("", node("all"), Scope::Subtree)]), ["all: ./"]);
    }

    #[test]
    fn a_module_tag_with_here_scope_has_no_map_form() {
        let map = BTreeMap::from([tag("src", Tag::Modules(Prefix::default()), Scope::Here)]);
        assert!(matches!(
            rules_of(&map),
            Err(InitError::MapUnsupported { .. })
        ));
    }
}
