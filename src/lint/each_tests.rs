//! `%% each` edge templates in the pure core: what lint says about their expansions, see "Edge
//! templates" in `docs/DESIGN.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::diagram;
use crate::imports::FileImports;
use crate::lint::fixtures::at;
use crate::model::{ImportKind, ResolvedImport, Severity, Violation, ViolationKind};

/// Two features, `billing` (controller, service, view) and `search` (controller, service), and
/// `shared` beside `features`. The `extra` lines start at line 16.
fn contract(extra: &str) -> Contract {
    diagram::parse(&format!(
        "flowchart LR
  subgraph features
    subgraph billing
      billing-controller
      billing-service
      billing-view
    end
    subgraph search
      search-controller
      search-service
    end
  end
  subgraph shared
    util
  end
{extra}"
    ))
    .unwrap()
}

fn owners(contract: &Contract) -> Ownership {
    Ownership {
        owners: contract
            .nodes
            .keys()
            .map(|n| (PathBuf::from(format!("{n}/x.ts")), n.clone()))
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
    reconcile(
        contract,
        &owners(contract),
        &imports,
        Path::new("architecture.mmd"),
    )
    .violations
}

fn of_kind(found: &[Violation], kind: ViolationKind) -> Vec<&Violation> {
    found.iter().filter(|v| v.kind == kind).collect()
}

const SERVICE: &str = "  %% each features: controller --> service\n";

#[test]
fn an_expansion_allows_its_pair_in_every_child_and_nothing_else() {
    let c = contract(SERVICE);
    let found = lint(
        &c,
        &[
            ("billing-controller", "billing-service", false),
            ("search-controller", "search-service", false),
        ],
    );
    assert_eq!(found, Vec::new(), "{found:#?}");
    let found = lint(
        &c,
        &[
            ("billing-service", "billing-controller", false),
            ("billing-controller", "search-service", false),
        ],
    );
    assert_eq!(of_kind(&found, ViolationKind::UndeclaredEdge).len(), 2);
}

#[test]
fn a_template_arrow_is_stale_only_when_no_child_realises_it_and_is_reported_once() {
    let c = contract(SERVICE);
    let found = lint(&c, &[("billing-controller", "billing-service", false)]);
    assert_eq!(found, Vec::new(), "one child is enough: {found:#?}");
    let found = lint(&c, &[]);
    let stale = of_kind(&found, ViolationKind::StaleEdge);
    assert_eq!(stale.len(), 1, "{found:#?}");
    assert_eq!(stale[0].severity, Severity::Warning);
    assert!(
        stale[0].message.contains("`%% each features` on line 16"),
        "{}",
        stale[0].message
    );
    assert!(
        stale[0].message.contains("controller --> service"),
        "{}",
        stale[0].message
    );
}

#[test]
fn a_planned_arrow_realised_in_a_child_is_one_planned_realized() {
    let c = contract("  %% each features: controller -.-> service\n");
    let found = lint(
        &c,
        &[
            ("billing-controller", "billing-service", false),
            ("search-controller", "search-service", false),
        ],
    );
    assert_eq!(
        of_kind(&found, ViolationKind::PlannedRealized).len(),
        1,
        "{found:#?}"
    );
}

#[test]
fn a_test_arrow_refuses_production_imports_and_names_the_template() {
    let c = contract("  %% each features: controller -->|test| service\n");
    let found = lint(&c, &[("billing-controller", "billing-service", true)]);
    assert_eq!(found, Vec::new(), "{found:#?}");
    let found = lint(&c, &[("billing-controller", "billing-service", false)]);
    let undeclared = of_kind(&found, ViolationKind::UndeclaredEdge);
    assert_eq!(undeclared.len(), 1);
    assert!(
        undeclared[0]
            .message
            .contains("each features: controller -->|test| service (line 16)"),
        "{}",
        undeclared[0].message
    );
}

#[test]
fn an_explicit_edge_an_expansion_covers_is_redundant_and_names_the_template_line() {
    let c = contract(&format!(
        "{SERVICE}  billing-controller --> billing-service\n"
    ));
    let found = lint(&c, &[("billing-controller", "billing-service", false)]);
    let redundant = of_kind(&found, ViolationKind::RedundantEdge);
    assert_eq!(redundant.len(), 1, "{found:#?}");
    assert_eq!(redundant[0].severity, Severity::Warning);
    assert!(
        redundant[0]
            .message
            .contains("each features: controller --> service (line 16) already covers it"),
        "{}",
        redundant[0].message
    );
}

#[test]
fn an_explicit_edge_in_the_other_direction_is_not_redundant() {
    let c = contract(&format!(
        "{SERVICE}  billing-service --> billing-controller\n"
    ));
    let found = lint(&c, &[("billing-service", "billing-controller", false)]);
    assert_eq!(
        of_kind(&found, ViolationKind::RedundantEdge).len(),
        0,
        "{found:#?}"
    );
}

#[test]
fn expansions_never_trip_isolation() {
    // why: an expansion joins two nodes of one child, so `%% isolated` has nothing to refuse.
    let c = contract(&format!("  %% isolated: features\n{SERVICE}"));
    let found = lint(
        &c,
        &[
            ("billing-controller", "billing-service", false),
            ("search-controller", "search-service", false),
        ],
    );
    assert_eq!(found, Vec::new(), "{found:#?}");
    let found = lint(&c, &[("billing-controller", "search-service", false)]);
    assert_eq!(
        of_kind(&found, ViolationKind::IsolationBreach).len(),
        1,
        "{found:#?}"
    );
}

#[test]
fn expansions_narrow_no_pair_of_layers_of_an_open_order() {
    // why: the expansions are inside `features`, so `features` keeps its whole allowance to `shared`.
    let c = contract(&format!("  %% order open: features > shared\n{SERVICE}"));
    assert_eq!(c.arrows_between("features", "shared"), 0);
    let found = lint(
        &c,
        &[
            ("billing-controller", "billing-service", false),
            ("billing-view", "util", false),
            ("search-service", "util", false),
        ],
    );
    assert_eq!(found, Vec::new(), "{found:#?}");
}

#[test]
fn an_explicit_edge_to_the_lower_layer_narrows_where_the_template_does_not() {
    let c = contract(&format!(
        "  %% order open: features > shared\n{SERVICE}  billing-view --> util\n"
    ));
    assert_eq!(c.arrows_between("features", "shared"), 1);
}

#[test]
fn diff_reports_a_template_as_its_line_and_not_its_expansion() {
    let text = |extra: &str| {
        format!(
            "flowchart LR\n  subgraph f\n    subgraph a\n      a-x\n      a-y\n    end\n  end\n{extra}"
        )
    };
    let diff = contract_diff(&text(""), &text("  %% each f: x --> y\n")).unwrap();
    assert_eq!(diff.rules_added, vec!["%% each f: x --> y".to_owned()]);
    assert!(diff.added_edges.is_empty() && diff.added_boundary_edges.is_empty());
    let diff = contract_diff(&text(""), &text("  a-x --> a-y\n")).unwrap();
    assert!(!diff.is_empty());
}
