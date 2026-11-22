//! End to end through [`run`]: a workspace repository where one edge is a package import by name
//! and another is an import of an asset.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::model::{Location, Severity, Violation, ViolationKind};

const CONTRACT: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a --> b\n  a --> c\n";
const INDEX: &str = "packages/a/src/index.ts";

fn files(contract: &'static str, index: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", contract),
        ("package.json", r#"{"workspaces": ["packages/*"]}"#),
        ("data.json", "{}"),
        ("packages/a/.arch", "node: a\n"),
        ("packages/a/package.json", r#"{"name": "@x/a"}"#),
        (INDEX, index),
        ("packages/b/.arch", "node: b\n"),
        (
            "packages/b/package.json",
            r#"{"name": "@x/b", "exports": {".": "./src/index.ts"}}"#,
        ),
        ("packages/b/src/index.ts", "export const b = 1;\n"),
        ("packages/c/.arch", "node: c\n"),
        ("packages/c/theme.css", "body {}\n"),
    ]
}

fn repo(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    dir
}

fn lint(dir: &TempDir) -> Vec<Violation> {
    run(dir.path(), &LintOptions::default()).unwrap().violations
}

fn kinds(violations: &[Violation]) -> Vec<ViolationKind> {
    violations.iter().map(|v| v.kind).collect()
}

fn at(file: &str, line: u32) -> Location {
    Location {
        file: file.into(),
        line,
    }
}

const BOTH: &str = "import { b } from '@x/b';\nimport '../../c/theme.css';\nexport const a = b;\n";

#[test]
fn a_package_edge_and_an_asset_edge_back_the_declared_edges() {
    let dir = repo(&files(CONTRACT, BOTH));
    assert_eq!(lint(&dir), Vec::new());
}

#[test]
fn a_workspace_package_import_is_an_edge_between_nodes() {
    let contract = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a --> c\n";
    let dir = repo(&files(contract, BOTH));
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UndeclaredEdge]);
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].at, Some(at(INDEX, 1)));
    assert!(found[0].message.contains("`a` imports `b`"));
    assert!(found[0].message.contains("packages/b/src/index.ts"));
}

#[test]
fn an_asset_import_is_an_edge_to_the_node_that_owns_the_asset() {
    let contract = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a --> b\n";
    let dir = repo(&files(contract, BOTH));
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UndeclaredEdge]);
    assert_eq!(found[0].at, Some(at(INDEX, 2)));
    assert!(found[0].message.contains("`a` imports `c`"));
    assert!(found[0].message.contains("packages/c/theme.css"));
}

#[test]
fn an_asset_edge_keeps_a_declared_edge_from_being_stale() {
    let dir = repo(&files(CONTRACT, "import '../../c/theme.css';\n"));
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::StaleEdge]);
    assert!(found[0].message.contains("a --> b"));
}

#[test]
fn an_asset_import_is_never_a_private_symbol() {
    let contract = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a --> b\n  a --> c\n  %% exports c: Theme\n";
    let dir = repo(&files(contract, BOTH));
    assert_eq!(lint(&dir), Vec::new());
}

#[test]
fn a_missing_asset_is_an_unresolved_import_and_an_unowned_asset_is_no_edge() {
    let index = "import '@x/b';\nimport '../../c/theme.css';\nimport '../../../data.json';\nimport './missing.json';\n";
    let dir = repo(&files(CONTRACT, index));
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UnresolvedImport]);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].at, Some(at(INDEX, 4)));
    assert!(found[0].message.contains("./missing.json"));
}

#[test]
fn observe_reports_asset_owners_without_counting_assets_as_files() {
    let dir = repo(&files(CONTRACT, BOTH));
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    let edges: Vec<_> = observation
        .observed
        .iter()
        .map(|(edge, sites)| (edge.from.as_str(), edge.to.as_str(), sites.clone()))
        .collect();
    assert_eq!(
        edges,
        [
            ("a", "b", vec![at(INDEX, 1)]),
            ("a", "c", vec![at(INDEX, 2)])
        ]
    );
    assert_eq!(observation.ownership.owners.len(), 2);
    assert!(observation.ownership.unmapped.is_empty());
    let owned: Vec<_> = observation
        .ownership
        .asset_owners
        .iter()
        .map(|(path, node)| (path.display().to_string(), node.to_string()))
        .collect();
    assert_eq!(
        owned,
        [
            ("packages/a/package.json".to_owned(), "a".to_owned()),
            ("packages/b/package.json".to_owned(), "b".to_owned()),
            ("packages/c/theme.css".to_owned(), "c".to_owned()),
        ]
    );
}
