//! Test code in findings: a site written in test code is marked, and tests alone do not realise a
//! planned edge. See "Test edges" and "CLI surface" in `docs/DESIGN.md`.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::results::{Severity, Violation, ViolationKind};

/// Repo with nodes `a` and `b` and `arrow` between them (none when empty); `a` has a production
/// file and a test file, each present or not, both importing `b`.
fn repo(arrow: &str, production: bool, test: bool) -> TempDir {
    let dir = TempDir::new().unwrap();
    let edge = if arrow.is_empty() {
        String::new()
    } else {
        format!("  a {arrow} b\n")
    };
    let mut files = vec![
        (
            "architecture.mmd",
            format!("flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n{edge}"),
        ),
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
fn a_dotted_edge_used_only_by_tests_is_not_promoted_but_says_the_app_does_not_use_it() {
    let found = findings("-.->", false, true);
    assert_eq!(kinds(&found), [ViolationKind::TestOnlyEdge]);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].at, None);
    let message = &found[0].message;
    assert!(
        message.contains("planned edge a -.-> b is used only by test code")
            && message.contains("the app does not use it yet")
            && message.contains("a -.->|test| b")
            && message.contains("a -->|test| b"),
        "{message}"
    );
    let hint = found[0].hint.as_deref().unwrap();
    assert!(
        hint.contains("-.->|test|") && hint.contains("-->|test|"),
        "{hint}"
    );
}

#[test]
fn a_dotted_edge_with_a_production_site_is_still_promoted_even_when_tests_use_it_too() {
    assert_eq!(
        kinds(&findings("-.->", true, true)),
        [ViolationKind::PlannedRealized]
    );
    assert_eq!(
        kinds(&findings("-.->", true, false)),
        [ViolationKind::PlannedRealized]
    );
    assert_eq!(findings("-.->", false, false), Vec::new());
}

#[test]
fn a_planned_test_edge_realised_by_tests_keeps_planned_realized() {
    assert_eq!(
        kinds(&findings("-.->|test|", false, true)),
        [ViolationKind::PlannedRealized]
    );
}

#[test]
fn a_finding_at_a_test_site_is_marked_and_one_at_a_production_site_is_not() {
    let found = findings("", true, true);
    let sites: Vec<(&str, ViolationKind, bool)> = found
        .iter()
        .map(|v| {
            (
                v.at.as_ref().unwrap().file.to_str().unwrap(),
                v.kind,
                v.test_only,
            )
        })
        .collect();
    assert_eq!(
        sites,
        [
            ("a/x.test.ts", ViolationKind::UndeclaredEdge, true),
            ("a/x.ts", ViolationKind::UndeclaredEdge, false),
        ]
    );
}

#[test]
fn a_restricted_site_finding_in_test_code_is_marked_too() {
    let found = findings("-->|type|", true, true);
    let marked: Vec<bool> = found.iter().map(|v| v.test_only).collect();
    assert_eq!(marked, [true, false, false], "{found:?}");
}

#[test]
fn findings_without_a_site_are_never_marked() {
    let found = findings("-->", false, false);
    assert_eq!(kinds(&found), [ViolationKind::StaleEdge]);
    assert!(!found[0].test_only);
}

#[test]
fn a_planned_boundary_edge_used_only_by_tests_is_not_promoted_either() {
    let dir = repo("", false, true);
    let contract = "flowchart LR\n  subgraph x[\"X\"]\n    a\n  end\n  subgraph y[\"Y\"]\n    b\n  end\n  x -.-> y\n";
    fs::write(dir.path().join("architecture.mmd"), contract).unwrap();
    let found = run(dir.path(), &LintOptions::default()).unwrap().violations;
    assert_eq!(kinds(&found), [ViolationKind::TestOnlyEdge]);
    assert!(
        found[0].message.contains("x -.-> y"),
        "{}",
        found[0].message
    );
    assert!(found[0].message.contains("used only by test code"));
    // why: one production site anywhere under the boundary edge promotes it, as for a node edge.
    let dir = repo("", true, true);
    fs::write(dir.path().join("architecture.mmd"), contract).unwrap();
    let found = run(dir.path(), &LintOptions::default()).unwrap().violations;
    assert_eq!(kinds(&found), [ViolationKind::PlannedRealized]);
}
