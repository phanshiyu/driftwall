//! End to end through [`run`]: a test edge allows test-only imports and nothing else.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::model::{Severity, Violation, ViolationKind};

fn contract(arrow: &str) -> String {
    format!("flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a {arrow} b\n")
}

/// Repo with nodes `a` and `b`; `a` has a production file and a test file, each present or not.
fn repo(arrow: &str, production: bool, test: bool) -> TempDir {
    let dir = TempDir::new().unwrap();
    let mut files = vec![
        ("architecture.mmd", contract(arrow)),
        ("a/.arch", "node: a\n".to_owned()),
        ("b/.arch", "node: b\n".to_owned()),
        ("b/y.ts", "export const y = 1;\n".to_owned()),
    ];
    if production {
        files.push(("a/x.ts", "import { y } from '../b/y';\n".to_owned()));
    }
    if test {
        files.push(("a/x.test.ts", "import { y } from '../b/y';\n".to_owned()));
    }
    for (path, contents) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    dir
}

fn findings(arrow: &str, production: bool, test: bool) -> Vec<Violation> {
    let dir = repo(arrow, production, test);
    run(dir.path(), &LintOptions::default()).unwrap().violations
}

fn kinds(found: &[Violation]) -> Vec<ViolationKind> {
    found.iter().map(|v| v.kind).collect()
}

#[test]
fn a_test_edge_allows_test_only_imports() {
    assert_eq!(findings("-->|test|", false, true), Vec::new());
}

#[test]
fn a_production_import_on_a_test_edge_is_an_undeclared_edge_that_says_so() {
    let found = findings("-->|test|", true, true);
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].at.as_ref().unwrap().file.to_str(), Some("a/x.ts"));
    assert!(
        found[0].message.contains("allows test imports only"),
        "{}",
        found[0].message
    );
}

#[test]
fn production_sites_alone_leave_the_test_edge_stale_and_each_site_an_error() {
    let found = findings("-->|test|", true, false);
    assert_eq!(
        kinds(&found),
        [ViolationKind::UndeclaredEdge, ViolationKind::StaleEdge]
    );
    assert!(
        found[1].message.contains("a -->|test| b"),
        "{}",
        found[1].message
    );
}

#[test]
fn a_test_edge_with_no_site_is_stale_like_a_solid_one() {
    let found = findings("-->|test|", false, false);
    assert_eq!(kinds(&found), [ViolationKind::StaleEdge]);
    assert_eq!(found[0].severity, Severity::Warning);
}

#[test]
fn a_solid_edge_realised_only_by_tests_is_a_warning_that_names_the_fix() {
    let found = findings("-->", false, true);
    assert_eq!(kinds(&found), [ViolationKind::TestOnlyEdge]);
    assert_eq!(found[0].severity, Severity::Warning);
    assert!(
        found[0].message.contains("a -->|test| b"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_solid_edge_with_a_production_site_is_clean_and_with_none_is_stale() {
    assert_eq!(findings("-->", true, true), Vec::new());
    assert_eq!(findings("-->", true, false), Vec::new());
    assert_eq!(
        kinds(&findings("-->", false, false)),
        [ViolationKind::StaleEdge]
    );
}

#[test]
fn a_dotted_test_edge_is_tolerated_unused_and_promoted_when_tests_use_it() {
    assert_eq!(findings("-.->|test|", false, false), Vec::new());
    let found = findings("-.->|test|", false, true);
    assert_eq!(kinds(&found), [ViolationKind::PlannedRealized]);
    assert!(
        found[0].message.contains("-.->|test|"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_production_import_on_a_dotted_test_edge_is_an_error_and_does_not_promote_it() {
    let found = findings("-.->|test|", true, false);
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
}
