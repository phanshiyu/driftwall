//! Unit tests of the scope of planned tags, unit prefixes and the owners of assets.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::*;
use crate::imports::FileImports;
use crate::model::{Location, ResolvedImport, Scope};

fn files(paths: &[&str]) -> BTreeMap<PathBuf, Language> {
    paths
        .iter()
        .map(|path| (PathBuf::from(path), Language::TypeScript))
        .collect()
}

fn import(from: &str, to: &str) -> (PathBuf, FileImports) {
    let at = Location {
        file: PathBuf::from(from),
        line: 1,
    };
    (
        PathBuf::from(from),
        FileImports {
            resolved: vec![ResolvedImport::new(at, PathBuf::from(to))],
            unresolved: Vec::new(),
            skipped: Vec::new(),
        },
    )
}

fn plan_of(paths: &[&str], depth: Option<usize>) -> InitPlan {
    plan(
        &files(paths),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &InitOptions { depth },
        &[],
        &Existing::default(),
    )
}

fn scopes(plan: &InitPlan) -> Vec<(String, Scope)> {
    plan.tags
        .iter()
        .map(|(dir, found)| (dir.display().to_string(), found.scope))
        .collect()
}

fn tags(plan: &InitPlan) -> Vec<(String, String)> {
    plan.tags
        .iter()
        .map(|(dir, found)| (dir.display().to_string(), found.tag.to_string()))
        .collect()
}

fn node_ids(plan: &InitPlan) -> Vec<String> {
    plan.contract
        .nodes
        .keys()
        .map(ToString::to_string)
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(dir, tag)| ((*dir).to_owned(), (*tag).to_owned()))
        .collect()
}

#[test]
fn the_root_group_is_limited_to_its_own_files_and_other_tags_are_not() {
    let p = plan_of(&["a.ts", "src/x.ts", "lib/y.ts"], None);
    assert_eq!(
        scopes(&p),
        [
            (String::new(), Scope::Here),
            ("lib".to_owned(), Scope::Subtree),
            ("src".to_owned(), Scope::Subtree),
        ]
    );
}

#[test]
fn a_root_node_that_owns_files_below_the_root_is_not_limited() {
    let paths = ["a.ts", "src/x.ts", "src/deep/y.ts"];
    assert_eq!(
        scopes(&plan_of(&paths, Some(0))),
        [(String::new(), Scope::Subtree)]
    );
    assert_eq!(
        scopes(&plan_of(&paths, Some(1))),
        [
            (String::new(), Scope::Here),
            ("src".to_owned(), Scope::Subtree)
        ]
    );
}

#[test]
fn a_unit_group_is_limited_and_a_unit_prefix_is_its_directory_path() {
    let package = |dir: &str| Unit {
        dir: PathBuf::from(dir),
        kind: UnitKind::Package,
        name: None,
    };
    let units = [package(""), package("apps/web ui")];
    let paths = [
        "src/a.ts",
        "src/b.ts",
        "apps/web ui/src/a.ts",
        "apps/web ui/src/c.ts",
        "apps/web ui/vite.config.ts",
        "apps/web ui/test/t.ts",
    ];
    let p = plan(
        &files(&paths),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &InitOptions::default(),
        &units,
        &Existing::default(),
    );
    assert_eq!(
        tags(&p),
        pairs(&[
            ("apps/web ui", "apps-web-ui"),
            ("apps/web ui/src", "apps-web-ui-src"),
            ("apps/web ui/test", "apps-web-ui-test"),
            ("src", "*"),
        ])
    );
    assert_eq!(
        node_ids(&p),
        [
            "a",
            "apps-web-ui",
            "apps-web-ui-src",
            "apps-web-ui-test",
            "b"
        ]
    );
    let limited: Vec<Scope> = scopes(&p).into_iter().map(|(_, scope)| scope).collect();
    assert_eq!(
        limited,
        [Scope::Here, Scope::Here, Scope::Subtree, Scope::Subtree]
    );
}

#[test]
fn an_import_of_an_asset_is_an_edge_to_the_node_the_tags_give_the_asset() {
    let all = files(&["src/a/x.ts", "src/b/y.ts"]);
    let assets: BTreeSet<PathBuf> = ["src/b/data.json", "loose/data.json", "src/c/only.json"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let imports: BTreeMap<PathBuf, FileImports> = [
        import("src/a/x.ts", "src/b/data.json"),
        import("src/b/y.ts", "loose/data.json"),
        import("src/b/y.ts", "src/c/only.json"),
    ]
    .into_iter()
    .collect();
    let p = plan(
        &all,
        &assets,
        &imports,
        &InitOptions::default(),
        &[],
        &Existing::default(),
    );
    let edges: Vec<String> = p
        .contract
        .edges
        .keys()
        .map(|edge| format!("{} {}", edge.from, edge.to))
        .collect();
    assert_eq!(node_ids(&p), ["a", "b"]);
    assert_eq!(edges, ["a b"]);
}
