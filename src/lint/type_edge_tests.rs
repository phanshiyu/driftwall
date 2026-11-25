//! End to end through [`run`]: a type edge allows type-only imports, and `type, test` also tests.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::results::{Severity, Violation, ViolationKind};

const RUNTIME: &str = "import { y } from '../b/y';\n";
const TYPES: &str = "import type { Y } from '../b/y';\n";

/// Repo with nodes `a` and `b`, an edge `arrow`, and the given sites in `a`, as (file, source).
fn findings_in(arrow: &str, sites: &[(&str, &str)]) -> Vec<Violation> {
    let dir = TempDir::new().unwrap();
    let mut files = vec![
        (
            "architecture.mmd",
            format!("flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a {arrow} b\n"),
        ),
        ("a/.arch", "node: a\n".to_owned()),
        ("b/.arch", "node: b\n".to_owned()),
        ("b/y.ts", "export const y = 1;\n".to_owned()),
    ];
    files.extend(sites.iter().map(|&(f, s)| (f, s.to_owned())));
    for (path, contents) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    run(dir.path(), &LintOptions::default()).unwrap().violations
}

fn kinds(found: &[Violation]) -> Vec<ViolationKind> {
    found.iter().map(|v| v.kind).collect()
}

#[test]
fn a_type_edge_allows_type_only_imports() {
    assert_eq!(findings_in("-->|type|", &[("a/x.ts", TYPES)]), Vec::new());
    let jsdoc = "/** @type {import('../b/y').Y} */\nexport const v = 1;\n";
    assert_eq!(findings_in("-->|type|", &[("a/x.js", jsdoc)]), Vec::new());
}

#[test]
fn a_type_only_test_site_is_allowed_on_a_type_edge_and_on_a_test_edge() {
    let site = [("a/x.test.ts", TYPES)];
    assert_eq!(findings_in("-->|type|", &site), Vec::new());
    assert_eq!(findings_in("-->|test|", &site), Vec::new());
    assert_eq!(findings_in("-->|type, test|", &site), Vec::new());
}

#[test]
fn a_runtime_import_on_a_type_edge_is_an_undeclared_edge_that_says_so() {
    let found = findings_in("-->|type|", &[("a/x.ts", RUNTIME), ("a/z.ts", TYPES)]);
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].at.as_ref().unwrap().file.to_str(), Some("a/x.ts"));
    assert!(
        found[0].message.contains("allows type-only imports only"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_runtime_test_site_is_not_allowed_on_a_type_edge_but_is_on_type_test() {
    let sites = [("a/x.test.ts", RUNTIME), ("a/z.ts", TYPES)];
    let found = findings_in("-->|type|", &sites);
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
    assert_eq!(findings_in("-->|type, test|", &sites), Vec::new());
}

#[test]
fn a_production_runtime_site_on_a_type_test_edge_is_an_error_naming_both() {
    let found = findings_in("-->|type, test|", &[("a/x.ts", RUNTIME), ("a/z.ts", TYPES)]);
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
    assert!(
        found[0]
            .message
            .contains("allows type-only and test imports only"),
        "{}",
        found[0].message
    );
}

#[test]
fn runtime_sites_alone_leave_a_type_edge_stale_and_each_site_an_error() {
    let found = findings_in("-->|type|", &[("a/x.ts", RUNTIME)]);
    assert_eq!(
        kinds(&found),
        [ViolationKind::UndeclaredEdge, ViolationKind::StaleEdge]
    );
    assert_eq!(found[1].severity, Severity::Warning);
    assert!(
        found[1].message.contains("a -->|type| b"),
        "{}",
        found[1].message
    );
}

#[test]
fn a_type_edge_with_no_site_is_stale_and_a_dotted_one_is_tolerated() {
    assert_eq!(
        kinds(&findings_in("-->|type|", &[])),
        [ViolationKind::StaleEdge]
    );
    assert_eq!(
        kinds(&findings_in("-->|type, test|", &[])),
        [ViolationKind::StaleEdge]
    );
    assert_eq!(findings_in("-.->|type|", &[]), Vec::new());
}

#[test]
fn a_dotted_type_edge_is_promoted_by_a_type_only_site_not_by_a_runtime_one() {
    let found = findings_in("-.->|type|", &[("a/x.ts", TYPES)]);
    assert_eq!(kinds(&found), [ViolationKind::PlannedRealized]);
    assert!(
        found[0].message.contains("-.->|type|"),
        "{}",
        found[0].message
    );
    let found = findings_in("-.->|type|", &[("a/x.ts", RUNTIME)]);
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
}

#[test]
fn a_solid_edge_realised_only_by_type_imports_is_clean() {
    assert_eq!(findings_in("-->", &[("a/x.ts", TYPES)]), Vec::new());
}

#[test]
fn python_type_checking_imports_are_type_only() {
    let types = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from b.y import Y\n";
    let runtime = "from b.y import Y\n";
    let dir = |source: &str, arrow: &str| {
        let dir = TempDir::new().unwrap();
        for (path, contents) in [
            (
                "architecture.mmd",
                format!("flowchart LR\n  a\n  b\n  a {arrow} b\n"),
            ),
            ("a/.arch", "node: a\n".to_owned()),
            ("b/.arch", "node: b\n".to_owned()),
            ("b/__init__.py", String::new()),
            ("b/y.py", "class Y: ...\n".to_owned()),
            ("a/x.py", source.to_owned()),
        ] {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        run(dir.path(), &LintOptions::default()).unwrap().violations
    };
    assert_eq!(dir(types, "-->|type|"), Vec::new());
    assert_eq!(
        kinds(&dir(runtime, "-->|type|")),
        [ViolationKind::UndeclaredEdge, ViolationKind::StaleEdge]
    );
}
