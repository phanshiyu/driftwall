//! Nested boundaries and `%% isolated` in the pure core: breaches, stale isolation, depth 3 and the
//! rules of "Boundary edges" over nested members.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::code::{ImportKind, ResolvedImport};
use crate::diagram;
use crate::lint::fixtures::{at, node};
use crate::reading::imports::FileImports;
use crate::results::{Severity, Violation, ViolationKind};

/// `features` holds `billing` (with `invoices` inside it, three levels deep) and `search`; `shared`
/// is beside it. Every node `n` owns the file `n/x.ts`.
fn contract(extra: &str) -> Contract {
    diagram::parse(&format!(
        "flowchart LR
  subgraph features
    subgraph billing
      bill-ui
      subgraph invoices
        invoice-svc
      end
    end
    subgraph search
      search-svc
    end
  end
  subgraph shared
    util
  end
{extra}"
    ))
    .unwrap()
}

const NODES: [&str; 4] = ["bill-ui", "invoice-svc", "search-svc", "util"];

fn owners() -> Ownership {
    Ownership {
        owners: NODES
            .iter()
            .map(|n| (PathBuf::from(format!("{n}/x.ts")), node(n)))
            .collect(),
        ..Ownership::default()
    }
}

/// Imports as `(from node, to node, test-only)`.
fn lint(contract: &Contract, sites: &[(&str, &str, bool)]) -> Vec<Violation> {
    let mut imports: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
    for &(from, to, test_only) in sites {
        let file = format!("{from}/x.ts");
        imports
            .entry(PathBuf::from(&file))
            .or_default()
            .resolved
            .push(
                ResolvedImport::new(at(&file, 3), PathBuf::from(format!("{to}/x.ts")))
                    .with_kind(ImportKind::Runtime)
                    .with_test_only(test_only),
            );
    }
    reconcile(contract, &owners(), &imports, Path::new("architecture.mmd")).violations
}

fn of_kind(found: &[Violation], kind: ViolationKind) -> Vec<&Violation> {
    found.iter().filter(|v| v.kind == kind).collect()
}

#[test]
fn a_depth_three_import_into_another_child_is_a_breach_naming_both_children() {
    let c = contract("  %% isolated: features\n");
    let found = lint(&c, &[("invoice-svc", "search-svc", false)]);
    let breaches = of_kind(&found, ViolationKind::IsolationBreach);
    assert_eq!(breaches.len(), 1);
    assert_eq!(breaches[0].severity, Severity::Error);
    let message = &breaches[0].message;
    assert!(message.contains("`features`") && message.contains("(in `billing`)"));
    assert!(message.contains("(in `search`"), "{message}");
}

#[test]
fn the_breach_is_not_excused_by_a_covering_arrow_because_none_can_be_declared() {
    let src = "flowchart LR\n  subgraph f\n    subgraph a\n      x\n    end\n    subgraph b\n      y\n    end\n  end\n  %% isolated: f\n  x --> y\n";
    assert!(diagram::parse(src).is_err());
}

#[test]
fn imports_inside_one_child_at_any_depth_and_out_of_the_boundary_are_fine() {
    let c = contract("  %% isolated: features\n  bill-ui --> invoice-svc\n  features --> shared\n");
    let found = lint(
        &c,
        &[
            ("bill-ui", "invoice-svc", false),
            ("invoice-svc", "util", false),
            ("search-svc", "util", false),
        ],
    );
    assert_eq!(found, Vec::new(), "{found:#?}");
}

#[test]
fn test_only_and_type_only_sites_are_breaches_too() {
    let c = contract("  %% isolated: features\n");
    let found = lint(
        &c,
        &[
            ("bill-ui", "search-svc", true),
            ("search-svc", "invoice-svc", false),
        ],
    );
    let breaches = of_kind(&found, ViolationKind::IsolationBreach);
    assert_eq!(breaches.len(), 2);
    assert!(breaches.iter().any(|v| v.test_only));
}

#[test]
fn without_the_rule_the_same_import_is_only_undeclared() {
    let found = lint(&contract(""), &[("invoice-svc", "search-svc", false)]);
    assert_eq!(of_kind(&found, ViolationKind::IsolationBreach).len(), 0);
    assert_eq!(of_kind(&found, ViolationKind::UndeclaredEdge).len(), 1);
}

#[test]
fn an_isolated_boundary_with_fewer_than_two_children_is_a_stale_isolation_warning() {
    let c = contract("  %% isolated: invoices, shared\n");
    let found = lint(&c, &[]);
    let stale = of_kind(&found, ViolationKind::StaleIsolation);
    assert_eq!(stale.len(), 2);
    assert!(
        stale
            .iter()
            .all(|v| v.severity == Severity::Warning && v.at.is_none())
    );
    assert!(
        stale[0].message.contains("one child"),
        "{}",
        stale[0].message
    );
    let two = lint(&contract("  %% isolated: billing\n"), &[]);
    assert!(
        of_kind(&two, ViolationKind::StaleIsolation).is_empty(),
        "{two:#?}"
    );
}

#[test]
fn nested_isolation_keeps_the_children_of_the_inner_boundary_apart_too() {
    let c = contract("  %% isolated: features, billing\n");
    let found = lint(&c, &[("bill-ui", "invoice-svc", false)]);
    let breaches = of_kind(&found, ViolationKind::IsolationBreach);
    assert_eq!(breaches.len(), 1);
    assert!(breaches[0].message.contains("boundary `billing`"));
}

#[test]
fn nested_members_get_the_via_and_redundant_findings_of_the_boundary_edge() {
    let c = contract("  features --> shared\n  invoice-svc --> util\n  billing --> shared\n");
    let found = lint(&c, &[("invoice-svc", "util", false)]);
    let redundant = of_kind(&found, ViolationKind::RedundantEdge);
    assert_eq!(redundant.len(), 1);
    assert!(redundant[0].message.contains("invoice-svc --> util"));
    // why: `billing --> shared` is itself covered by `features --> shared`, which is not reported.
    assert!(redundant.iter().all(|v| !v.message.contains("billing -->")));
}

#[test]
fn an_upward_import_is_found_through_nested_membership() {
    let c = contract("  %% order: features > shared\n  features --> shared\n");
    let found = lint(&c, &[("util", "invoice-svc", false)]);
    assert_eq!(of_kind(&found, ViolationKind::UpwardEdge).len(), 1);
    let ok = lint(&c, &[("invoice-svc", "util", false)]);
    assert_eq!(of_kind(&ok, ViolationKind::UpwardEdge).len(), 0);
}
