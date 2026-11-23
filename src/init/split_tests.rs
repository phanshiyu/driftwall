//! Unit tests of the layer split: when a large node is cut one level further.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::*;
use crate::model::{Language, Scope};

fn many(dir: &str, count: usize) -> Vec<String> {
    (0..count).map(|n| format!("{dir}/f{n}.ts")).collect()
}

fn planned(paths: &[String]) -> InitPlan {
    let files: BTreeMap<PathBuf, Language> = paths
        .iter()
        .map(|path| (PathBuf::from(path), Language::TypeScript))
        .collect();
    plan(
        &files,
        &BTreeSet::new(),
        &BTreeMap::new(),
        &InitOptions::default(),
        &[],
        &Existing::default(),
    )
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

#[test]
fn a_large_module_with_two_layers_becomes_a_node_per_layer() {
    let mut paths = vec!["src/App.ts".to_owned(), "src/main.ts".to_owned()];
    paths.extend(many("src/components", 10));
    paths.extend(many("src/components/settings", 19));
    paths.extend(many("src/components/ui", 8));
    let p = planned(&paths);
    assert_eq!(
        tags(&p),
        pairs(&[
            ("src", "*"),
            ("src/components", "components"),
            ("src/components/settings", "components-settings"),
            ("src/components/ui", "components-ui"),
        ])
    );
    let ids: Vec<String> = p.contract.nodes.keys().map(ToString::to_string).collect();
    assert_eq!(
        ids,
        [
            "App",
            "components",
            "components-settings",
            "components-ui",
            "main"
        ]
    );
    assert!(p.tags.values().all(|tag| tag.scope == Scope::Subtree));
}

#[test]
fn a_large_directory_node_splits_the_same_way() {
    let mut paths = many("app", 10);
    paths.extend(many("app/x", 13));
    paths.extend(many("app/y", 13));
    paths.extend(many("lib", 3));
    let p = planned(&paths);
    assert_eq!(
        tags(&p),
        pairs(&[
            ("app", "app"),
            ("app/x", "app-x"),
            ("app/y", "app-y"),
            ("lib", "lib"),
        ])
    );
}

#[test]
fn a_node_below_the_file_threshold_is_not_split() {
    let mut paths = many("app", 10);
    paths.extend(many("app/x", 12));
    paths.extend(many("lib", 3));
    assert_eq!(
        tags(&planned(&paths)),
        pairs(&[("app", "app"), ("lib", "lib")])
    );
}

#[test]
fn small_child_directories_stay_in_the_node() {
    let mut paths = many("app", 30);
    paths.extend(many("app/small", 7));
    paths.extend(many("lib", 3));
    assert_eq!(
        tags(&planned(&paths)),
        pairs(&[("app", "app"), ("lib", "lib")])
    );
}

#[test]
fn a_flat_collection_of_features_is_not_split() {
    let mut paths = many("app", 5);
    for feature in ["a", "b", "c", "d", "e"] {
        paths.extend(many(&format!("app/{feature}"), 9));
    }
    paths.extend(many("lib", 3));
    assert_eq!(
        tags(&planned(&paths)),
        pairs(&[("app", "app"), ("lib", "lib")])
    );
}
