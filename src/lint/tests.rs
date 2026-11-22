//! End-to-end tests of [`run`] against small TypeScript repositories built in temp directories.

use std::fs;
use std::io;
use std::path::Path;

use tempfile::TempDir;

use super::*;
use crate::model::{Location, Severity, Violation, ViolationKind};

/// Writes `files` (relative path, contents) into a fresh temp directory.
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

const TWO_NODES: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";

/// Two tagged directories where `a` imports `b`.
fn clean_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        (
            "a/main.ts",
            "import { b } from '../b/lib';\nexport const a = b;\n",
        ),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", "export const b = 1;\n"),
    ]
}

#[test]
fn default_options_name_architecture_mmd() {
    assert_eq!(
        LintOptions::default().contract,
        Path::new("architecture.mmd")
    );
}

#[test]
fn clean_repo_gives_an_empty_report() {
    let dir = repo(&clean_files());
    let report = run(dir.path(), &LintOptions::default()).unwrap();
    assert_eq!(report, LintReport::default());
}

#[test]
fn undeclared_edge_is_reported_at_the_import_line() {
    let mut files = clean_files();
    files.push((
        "b/other.ts",
        "export const x = 1;\nimport { a } from '../a/main';\n",
    ));
    let dir = repo(&files);
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UndeclaredEdge]);
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].at, Some(at("b/other.ts", 2)));
    assert!(found[0].message.contains("`b` imports `a`"));
}

#[test]
fn unmapped_file_is_reported_without_cascading() {
    let mut files = clean_files();
    files.push((
        "loose/x.ts",
        "import { b } from '../b/lib';\nexport const x = b;\n",
    ));
    let dir = repo(&files);
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UnmappedFile]);
    assert_eq!(found[0].at, Some(at("loose/x.ts", 1)));
}

#[test]
fn unknown_node_tag_is_reported_without_cascading() {
    let mut files = clean_files();
    files.push(("ghost/.arch", "# who?\nnode: ghost\n"));
    files.push((
        "ghost/x.ts",
        "import { b } from '../b/lib';\nexport const x = b;\n",
    ));
    let dir = repo(&files);
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UnknownNode]);
    assert_eq!(found[0].at, Some(at("ghost/.arch", 2)));
    assert!(found[0].message.contains("ghost"));
}

#[test]
fn stale_solid_edge_is_a_warning() {
    let mut files = clean_files();
    files[2] = ("a/main.ts", "export const a = 1;\n");
    let dir = repo(&files);
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::StaleEdge]);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].at, None);
}

#[test]
fn realised_planned_edge_is_a_warning_not_an_error() {
    let mut files = clean_files();
    files[0] = (
        "architecture.mmd",
        "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a -.-> b\n",
    );
    let dir = repo(&files);
    let report = run(dir.path(), &LintOptions::default()).unwrap();
    assert_eq!(
        kinds(&report.violations),
        vec![ViolationKind::PlannedRealized]
    );
    assert!(!report.failed(false));
    assert!(report.failed(true));
}

#[test]
fn unresolved_relative_import_is_a_warning_with_its_specifier() {
    let mut files = clean_files();
    files.push((
        "a/broken.ts",
        "export const y = 1;\nimport { z } from './missing';\n",
    ));
    let dir = repo(&files);
    let found = lint(&dir);
    assert_eq!(kinds(&found), vec![ViolationKind::UnresolvedImport]);
    assert_eq!(found[0].at, Some(at("a/broken.ts", 2)));
    assert!(found[0].message.contains("./missing"));
}

#[test]
fn invalid_contract_reports_the_diagram_error_with_its_line() {
    let mut files = clean_files();
    files[0] = (
        "architecture.mmd",
        "flowchart LR\n  a[\"a\"]\n  a --> ghost\n",
    );
    let dir = repo(&files);
    let error = run(dir.path(), &LintOptions::default()).unwrap_err();
    match error {
        LintError::Diagram { path, source } => {
            assert_eq!(path, Path::new("architecture.mmd"));
            assert_eq!(source.line(), Some(3));
        }
        other => panic!("expected a diagram error, got {other:?}"),
    }
}

#[test]
fn missing_contract_is_an_io_error_naming_the_path() {
    let dir = repo(&[("a/.arch", "node: a\n")]);
    let error = run(dir.path(), &LintOptions::default()).unwrap_err();
    match &error {
        LintError::Io { path, source } => {
            assert_eq!(path, Path::new("architecture.mmd"));
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("expected an io error, got {other:?}"),
    }
    assert!(error.to_string().contains("architecture.mmd"));
}

#[test]
fn custom_contract_path_is_used() {
    let mut files = clean_files();
    files[0] = ("docs/contract.mmd", TWO_NODES);
    let dir = repo(&files);
    let options = LintOptions {
        contract: "docs/contract.mmd".into(),
    };
    assert_eq!(run(dir.path(), &options).unwrap(), LintReport::default());
}

#[test]
fn invalid_arch_file_is_an_ownership_error() {
    let mut files = clean_files();
    files.push(("c/.arch", "oops\n"));
    files.push(("c/x.ts", "export {};\n"));
    let dir = repo(&files);
    let error = run(dir.path(), &LintOptions::default()).unwrap_err();
    assert!(matches!(error, LintError::Ownership(_)), "{error:?}");
}

#[test]
fn nested_arch_override_and_edge_direction_are_honoured() {
    // why: a/inner belongs to `b` by override, and the code goes a --> b while the contract has b --> a.
    let dir = repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  b --> a\n",
        ),
        ("a/.arch", "node: a\n"),
        (
            "a/main.ts",
            "import { i } from './inner/impl';\nexport const m = i;\n",
        ),
        ("a/inner/.arch", "node: b\n"),
        ("a/inner/impl.ts", "export const i = 1;\n"),
    ]);
    let found = lint(&dir);
    assert_eq!(
        kinds(&found),
        vec![ViolationKind::UndeclaredEdge, ViolationKind::StaleEdge]
    );
    assert_eq!(found[0].at, Some(at("a/main.ts", 1)));
    assert!(found[0].message.contains("`a` imports `b`"));
    assert!(found[1].message.contains("b --> a"));
}

#[test]
fn nested_override_back_to_the_same_node_is_a_same_owner_import() {
    let dir = repo(&[
        ("architecture.mmd", "flowchart LR\n  a[\"a\"]\n"),
        ("a/.arch", "node: a\n"),
        (
            "a/main.ts",
            "import { i } from './inner/impl';\nexport const m = i;\n",
        ),
        ("a/inner/.arch", "node: a\n"),
        ("a/inner/impl.ts", "export const i = 1;\n"),
    ]);
    assert_eq!(lint(&dir), Vec::new());
}

#[test]
fn observe_returns_what_run_reports_plus_what_it_read() {
    let mut files = clean_files();
    files.push((
        "b/other.ts",
        "export const x = 1;\nimport { a } from '../a/main';\n",
    ));
    let dir = repo(&files);
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    assert_eq!(
        observation.report,
        run(dir.path(), &LintOptions::default()).unwrap()
    );
    assert_eq!(observation.contract.nodes.len(), 2);
    assert_eq!(observation.ownership.owners.len(), 3);
    let edges: Vec<_> = observation
        .observed
        .keys()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    assert_eq!(edges, [("a", "b"), ("b", "a")]);
    let first: Vec<_> = observation
        .observed
        .values()
        .next()
        .unwrap()
        .iter()
        .map(|site| site.at.clone())
        .collect();
    assert_eq!(first, vec![at("a/main.ts", 1)]);
    let used: Vec<_> = observation
        .symbols
        .values()
        .map(std::collections::BTreeSet::len)
        .collect();
    assert_eq!(used, [1, 1]);
    assert_eq!(
        observation.symbols.keys().collect::<Vec<_>>(),
        observation.observed.keys().collect::<Vec<_>>()
    );
}

#[test]
fn observe_fails_like_run_on_a_missing_contract() {
    let dir = repo(&[("a/x.ts", "")]);
    assert!(matches!(
        observe(dir.path(), &LintOptions::default()),
        Err(LintError::Io { .. })
    ));
}
