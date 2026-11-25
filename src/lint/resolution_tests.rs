//! End to end through [`run`]: a workspace repository where one edge is a package import by name
//! and another is an import of an asset.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::code::Location;
use crate::results::{Severity, Violation, ViolationKind};

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
        .map(|(edge, sites)| {
            (
                edge.from.as_str(),
                edge.to.as_str(),
                sites.iter().map(|site| site.at.clone()).collect::<Vec<_>>(),
            )
        })
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

const TWO_NODES: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";

type Undeclared = (String, u32, String, Option<String>, Option<String>);

fn undeclared(dir: &TempDir) -> Vec<Undeclared> {
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    observation
        .blind_spots
        .skipped
        .get(&crate::code::SkipReason::UndeclaredOwner)
        .into_iter()
        .flatten()
        .map(|reference| {
            (
                reference.at.file.display().to_string(),
                reference.at.line,
                reference.specifier.clone(),
                reference.node.as_ref().map(ToString::to_string),
                reference.message.clone(),
            )
        })
        .collect()
}

#[test]
fn an_asset_import_to_an_undeclared_owner_is_a_skipped_reference_of_the_importer() {
    let dir = repo(&files(TWO_NODES, BOTH));
    assert_eq!(
        undeclared(&dir),
        [(
            INDEX.to_owned(),
            2,
            "packages/c/theme.css".to_owned(),
            Some("a".to_owned()),
            Some(
                "asset `packages/c/theme.css` is owned by `c`, which the contract does not declare"
                    .to_owned()
            ),
        )]
    );
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UnknownNode]);
}

#[test]
fn an_owner_derived_by_a_modules_tag_that_the_contract_lacks_is_named_too() {
    let mut all = files(TWO_NODES, "import '../../shared/theme.css';\n");
    all.retain(|(path, _)| !path.starts_with("packages/c"));
    all.push(("packages/shared/.arch", "node: *\n"));
    all.push(("packages/shared/theme.css", ""));
    let dir = repo(&all);
    let rows = undeclared(&dir);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0].4.as_deref(),
        Some(
            "asset `packages/shared/theme.css` is owned by `theme`, which the contract does not declare"
        )
    );
}

#[test]
fn a_declared_owner_an_unowned_asset_a_source_target_and_an_undeclared_importer_record_nothing() {
    let dir = repo(&files(CONTRACT, BOTH));
    assert_eq!(undeclared(&dir), []);

    let unowned = repo(&files(TWO_NODES, "import '../../../data.json';\n"));
    assert_eq!(undeclared(&unowned), []);

    let mut ghost = files(TWO_NODES, "export {};\n");
    ghost.push(("packages/g/.arch", "node: ghost\n"));
    ghost.push(("packages/g/x.ts", "import '../c/theme.css';\n"));
    assert_eq!(undeclared(&repo(&ghost)), []);

    let mut source = files(TWO_NODES, "import '../../c/x';\n");
    source.push(("packages/c/x.ts", "export const x = 1;\n"));
    assert_eq!(undeclared(&repo(&source)), []);
}
