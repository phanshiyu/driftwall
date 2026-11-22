//! Unit tests of the plan's rules about units, existing tags, root files and collisions.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;
use crate::model::{Language, NodeId, Ownership, Tag};

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
    }
}

fn planned(paths: &[&str], units: &[Unit], existing: &Existing) -> InitPlan {
    plan(
        &files(paths),
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
        .map(|(dir, tag)| (dir.display().to_string(), tag.to_string()))
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
fn two_crates_are_one_node_each_covering_their_directory() {
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
    assert_eq!(tags(&p), pairs(&[("crates/a", "a"), ("crates/b", "b")]));
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
fn a_lone_crate_is_decomposed_but_a_lone_package_is_not() {
    let paths = ["src/a.rs", "src/b.rs", "scripts/x.rs", "tests/y.rs"];
    let lone_crate = planned(&paths, &[unit("", UnitKind::Crate)], &Existing::default());
    assert_eq!(
        tags(&lone_crate),
        pairs(&[("scripts", "scripts"), ("src", "*"), ("tests", "tests")])
    );
    let lone_package = planned(
        &paths,
        &[unit("src", UnitKind::Package)],
        &Existing::default(),
    );
    assert_eq!(
        tags(&lone_package),
        pairs(&[("scripts", "scripts"), ("src", "src"), ("tests", "tests")])
    );
}

#[test]
fn a_lone_crate_with_one_module_in_src_decides_nothing() {
    let p = planned(
        &["src/main.rs", "src/a/x.rs", "src/b/y.rs"],
        &[unit("", UnitKind::Crate)],
        &Existing::default(),
    );
    assert_eq!(
        tags(&p),
        pairs(&[("src", "src"), ("src/a", "a"), ("src/b", "b")])
    );
}

#[test]
fn modules_that_collide_with_an_existing_node_fall_back_to_directories() {
    let existing = Existing {
        nodes: BTreeMap::from([(id("store"), "elsewhere".to_owned())]),
        owners: BTreeMap::from([(PathBuf::from("elsewhere/z.rs"), id("store"))]),
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
    ownership
        .tag_dirs
        .insert(PathBuf::from("lib"), Tag::Node(id("lib")));
    ownership
        .tag_dirs
        .insert(PathBuf::from("src"), Tag::Modules);
    let at = |file: &str| crate::model::Location {
        file: PathBuf::from(file),
        line: 1,
    };
    ownership.tags = vec![
        (at("lib/.arch"), "lib".to_owned()),
        (at("src/.arch"), "gitlab".to_owned()),
    ];
    ownership
        .owners
        .insert(PathBuf::from("src/gitlab.rs"), id("gitlab"));
    let existing = Existing::of(&ownership);
    assert_eq!(
        existing.nodes,
        BTreeMap::from([
            (id("gitlab"), "src/gitlab".to_owned()),
            (id("lib"), "lib".to_owned())
        ])
    );
    assert_eq!(existing.owners, ownership.owners);
}
