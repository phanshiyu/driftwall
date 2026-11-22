//! Unit tests of the plan's rules about units, existing tags, root files and collisions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::*;
use crate::model::{DirTag, Language, NodeId, Ownership, Prefix, Scope, Tag};

fn files(paths: &[&str]) -> BTreeMap<PathBuf, Language> {
    paths
        .iter()
        .map(|path| (PathBuf::from(path), Language::Rust))
        .collect()
}

fn unit(dir: &str, kind: UnitKind) -> Unit {
    Unit {
        dir: PathBuf::from(dir),
        kind,
        name: None,
    }
}

fn planned(paths: &[&str], units: &[Unit], existing: &Existing) -> InitPlan {
    plan(
        &files(paths),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &InitOptions::default(),
        units,
        existing,
    )
}

fn id(raw: &str) -> NodeId {
    NodeId::new(raw).unwrap()
}

fn tags(plan: &InitPlan) -> Vec<(String, String)> {
    plan.tags
        .iter()
        .map(|(dir, found)| (dir.display().to_string(), found.tag.to_string()))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(dir, tag)| ((*dir).to_owned(), (*tag).to_owned()))
        .collect()
}

fn node_ids(plan: &InitPlan) -> Vec<String> {
    plan.contract
        .nodes
        .keys()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn every_crate_is_decomposed_and_one_without_two_modules_is_one_node() {
    let crates = [
        unit("crates/a", UnitKind::Crate),
        unit("crates/b", UnitKind::Crate),
    ];
    let p = planned(
        &[
            "crates/a/src/lib.rs",
            "crates/a/src/x.rs",
            "crates/a/tests/it.rs",
            "crates/b/src/lib.rs",
        ],
        &crates,
        &Existing::default(),
    );
    assert_eq!(
        tags(&p),
        pairs(&[
            ("crates/a/src", "crates-a-*"),
            ("crates/a/tests", "crates-a-tests"),
            ("crates/b", "crates-b")
        ])
    );
    assert_eq!(
        node_ids(&p),
        ["crates-a-lib", "crates-a-tests", "crates-a-x", "crates-b"]
    );
}

#[test]
fn a_root_package_next_to_its_members_covers_only_its_src() {
    let crates = [unit("", UnitKind::Crate), unit("crates/a", UnitKind::Crate)];
    let p = planned(
        &["src/main.rs", "build.rs", "crates/a/src/lib.rs"],
        &crates,
        &Existing::default(),
    );
    assert_eq!(
        tags(&p),
        pairs(&[("", "root"), ("crates/a", "crates-a"), ("src", "src")])
    );
}

#[test]
fn a_lone_unit_is_decomposed_whatever_its_kind() {
    let paths = ["src/a.rs", "src/b.rs", "scripts/x.rs", "tests/y.rs"];
    let expected = pairs(&[("scripts", "scripts"), ("src", "*"), ("tests", "tests")]);
    for lone in [
        unit("", UnitKind::Crate),
        unit("", UnitKind::Package),
        unit("src", UnitKind::PythonPackage),
    ] {
        let p = planned(&paths, std::slice::from_ref(&lone), &Existing::default());
        assert_eq!(tags(&p), expected, "{lone:?}");
        assert_eq!(node_ids(&p), ["a", "b", "scripts", "tests"], "{lone:?}");
    }
}

#[test]
fn a_units_source_directory_counts_subdirectories_as_modules() {
    let p = planned(
        &["src/main.rs", "src/a/x.rs", "src/b/y.rs"],
        &[unit("", UnitKind::Crate)],
        &Existing::default(),
    );
    assert_eq!(tags(&p), pairs(&[("src", "*")]));
    assert_eq!(node_ids(&p), ["a", "b", "main"]);
}

#[test]
fn a_lone_crate_with_one_module_in_src_decides_nothing() {
    let p = planned(
        &["src/a/x.rs", "src/a/y.rs", "scripts/z.rs"],
        &[unit("", UnitKind::Crate)],
        &Existing::default(),
    );
    assert_eq!(tags(&p), pairs(&[("scripts", "scripts"), ("src", "src")]));
}

#[test]
fn modules_that_collide_with_an_existing_node_fall_back_to_directories() {
    let existing = Existing {
        nodes: BTreeMap::from([(id("store"), "elsewhere".to_owned())]),
        owners: BTreeMap::from([(PathBuf::from("elsewhere/z.rs"), id("store"))]),
        ..Existing::default()
    };
    let p = planned(
        &["elsewhere/z.rs", "src/main.rs", "src/store.rs"],
        &[unit("", UnitKind::Crate)],
        &existing,
    );
    assert_eq!(tags(&p), pairs(&[("src", "src")]));
    assert_eq!(node_ids(&p), ["src", "store"]);
}

#[test]
fn a_second_module_region_with_a_taken_id_gets_directory_nodes() {
    let p = planned(
        &["src/a.rs", "src/b.rs", "tools/a.rs", "tools/c.rs"],
        &[unit("", UnitKind::Crate)],
        &Existing::default(),
    );
    assert_eq!(tags(&p), pairs(&[("src", "*"), ("tools", "tools")]));
    assert_eq!(node_ids(&p), ["a", "b", "tools"]);
}

#[test]
fn existing_owners_are_not_planned_again_and_their_nodes_are_declared() {
    let existing = Existing {
        nodes: BTreeMap::from([
            (id("core"), "src".to_owned()),
            (id("spare"), "spare".to_owned()),
        ]),
        owners: BTreeMap::from([(PathBuf::from("src/a.rs"), id("core"))]),
        ..Existing::default()
    };
    let p = planned(&["src/a.rs", "other/b.rs"], &[], &existing);
    assert_eq!(tags(&p), pairs(&[("other", "other")]));
    assert_eq!(node_ids(&p), ["core", "other", "spare"]);
}

#[test]
fn nothing_fresh_plans_no_tags() {
    let existing = Existing {
        nodes: BTreeMap::from([(id("core"), "src".to_owned())]),
        owners: BTreeMap::from([(PathBuf::from("src/a.rs"), id("core"))]),
        ..Existing::default()
    };
    let p = planned(&["src/a.rs"], &[], &existing);
    assert!(p.tags.is_empty());
    assert_eq!(node_ids(&p), ["core"]);
}

#[test]
fn root_files_next_to_source_directories_form_one_node() {
    let p = planned(
        &["a.rs", "b.rs", "c.rs", "src/x.rs", "lib/y.rs"],
        &[],
        &Existing::default(),
    );
    assert_eq!(
        tags(&p),
        pairs(&[("", "root"), ("lib", "lib"), ("src", "src")])
    );
}

#[test]
fn existing_of_labels_nodes_by_their_tag_directory() {
    let mut ownership = Ownership::default();
    let dir_tag = |tag| DirTag {
        tag,
        scope: Scope::Subtree,
    };
    ownership
        .tag_dirs
        .insert(PathBuf::from("lib"), dir_tag(Tag::Node(id("lib"))));
    ownership.tag_dirs.insert(
        PathBuf::from("src"),
        dir_tag(Tag::Modules(Prefix::default())),
    );
    ownership.tag_dirs.insert(
        PathBuf::from("ui"),
        dir_tag(Tag::Modules(Prefix::new("ui-").unwrap())),
    );
    let at = |file: &str| crate::model::Location {
        file: PathBuf::from(file),
        line: 1,
    };
    ownership.tags = vec![
        (at("lib/.arch"), "lib".to_owned()),
        (at("src/.arch"), "gitlab".to_owned()),
        (at("ui/.arch"), "ui-store".to_owned()),
    ];
    ownership
        .owners
        .insert(PathBuf::from("src/gitlab.rs"), id("gitlab"));
    let existing = Existing::of(&ownership);
    assert_eq!(
        existing.nodes,
        BTreeMap::from([
            (id("gitlab"), "src/gitlab".to_owned()),
            (id("lib"), "lib".to_owned()),
            (id("ui-store"), "ui/store".to_owned()),
        ])
    );
    assert_eq!(existing.owners, ownership.owners);
}
