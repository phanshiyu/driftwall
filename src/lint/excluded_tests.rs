//! End to end through [`run`] and [`observe`]: an import of an excluded file is a real dependency
//! on the node that owns it.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::model::{ImportKind, Location, SkipReason, ViolationKind};

const BOTH: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";

/// The repro of the review: `a/g.mjs` and `b/h.mjs` exist and are excluded by their headers.
fn files(contract: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", contract),
        ("a/.arch", "node: a\n"),
        ("b/.arch", "node: b\n"),
        (
            "a/x.mjs",
            "import {g} from './g.mjs'; import {h} from '../b/h.mjs';\n",
        ),
        ("a/g.mjs", "// do not edit\nexport const g = 1;\n"),
        ("b/h.mjs", "// @generated\nexport const h = 1;\n"),
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

fn kinds(dir: &TempDir) -> Vec<ViolationKind> {
    let report = run(dir.path(), &LintOptions::default()).unwrap();
    report.violations.iter().map(|v| v.kind).collect()
}

#[test]
fn imports_of_excluded_files_are_no_unresolved_warning_and_the_edge_is_there() {
    let dir = repo(&files(BOTH));
    assert_eq!(kinds(&dir), Vec::new());
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    assert_eq!(observation.blind_spots.unresolved.len(), 0);
    let edges: Vec<_> = observation
        .observed
        .iter()
        .map(|(edge, sites)| {
            (
                edge.from.as_str(),
                edge.to.as_str(),
                sites
                    .iter()
                    .map(|site| (site.at.clone(), site.kind))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        edges,
        [(
            "a",
            "b",
            vec![(
                Location {
                    file: "a/x.mjs".into(),
                    line: 1
                },
                ImportKind::Runtime
            )]
        )]
    );
    let symbols: Vec<_> = observation
        .symbols
        .values()
        .flatten()
        .map(ToString::to_string)
        .collect();
    assert_eq!(symbols, ["h"]);
}

#[test]
fn the_edge_to_an_excluded_file_must_be_declared_like_any_other() {
    let contract = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n";
    let dir = repo(&files(contract));
    assert_eq!(kinds(&dir), [ViolationKind::UndeclaredEdge]);
}

#[test]
fn excluded_files_stay_out_of_owners_and_unmapped_and_their_own_imports_are_not_read() {
    let mut all = files(BOTH);
    all.push(("c/loose.mjs", "// @generated\nimport '../a/x.mjs';\n"));
    all.push(("c/user.mjs", "// @generated\nimport '../nowhere.mjs';\n"));
    let dir = repo(&all);
    assert_eq!(kinds(&dir), Vec::new());
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    let owners: Vec<_> = observation
        .ownership
        .owners
        .keys()
        .map(|path| path.display().to_string())
        .collect();
    assert_eq!(owners, ["a/x.mjs"]);
    assert!(observation.ownership.unmapped.is_empty());
    let owned: Vec<_> = observation
        .ownership
        .excluded_owners
        .iter()
        .map(|(path, node)| (path.display().to_string(), node.to_string()))
        .collect();
    assert_eq!(
        owned,
        [
            ("a/g.mjs".to_owned(), "a".to_owned()),
            ("b/h.mjs".to_owned(), "b".to_owned())
        ]
    );
}

#[test]
fn an_excluded_target_no_tag_covers_makes_no_edge_and_no_warning() {
    let mut all = files(BOTH);
    all.retain(|(path, _)| *path != "b/.arch");
    let dir = repo(&all);
    let found = kinds(&dir);
    assert_eq!(found, [ViolationKind::StaleEdge]);
}

#[test]
fn an_excluded_target_owned_by_an_undeclared_node_is_an_undeclared_owner_reference() {
    let contract = "flowchart LR\n  a[\"a\"]\n";
    let dir = repo(&files(contract));
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    let rows: Vec<_> = observation
        .blind_spots
        .skipped
        .get(&SkipReason::UndeclaredOwner)
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
        .collect();
    assert_eq!(
        rows,
        [(
            "a/x.mjs".to_owned(),
            1,
            "b/h.mjs".to_owned(),
            Some("a".to_owned()),
            Some(
                "excluded file `b/h.mjs` is owned by `b`, which the contract does not declare"
                    .to_owned()
            )
        )]
    );
    assert!(observation.observed.is_empty());
}
