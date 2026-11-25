//! Boundary edges in the pure core: every kind on every form of boundary edge lints like the node
//! edge it stands for, and the findings of "Boundary edges" (stale, planned, redundant, via).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::code::{ImportKind, ResolvedImport};
use crate::contract::EdgeKind;
use crate::diagram;
use crate::imports::FileImports;
use crate::lint::fixtures::{at, node};
use crate::results::{Severity, Violation, ViolationKind};

const KINDS: [EdgeKind; 8] = [
    EdgeKind::Implemented,
    EdgeKind::Planned,
    EdgeKind::Test,
    EdgeKind::PlannedTest,
    EdgeKind::Type,
    EdgeKind::PlannedType,
    EdgeKind::TypeTest,
    EdgeKind::PlannedTypeTest,
];

/// Two single-node boundaries `ga` and `gb`, so a boundary edge covers exactly the pair `a`, `b`.
fn single(arrows: &str) -> Contract {
    diagram::parse(&format!(
        "flowchart LR\n  subgraph ga[\"ga\"]\n    a\n  end\n  subgraph gb[\"gb\"]\n    b\n  end\n{arrows}"
    ))
    .unwrap()
}

/// Two nodes in each of `ga` and `gb`.
fn double(arrows: &str) -> Contract {
    diagram::parse(&format!(
        "flowchart LR\n  subgraph ga[\"ga\"]\n    a1\n    a2\n  end\n  subgraph gb[\"gb\"]\n    b1\n    b2\n  end\n{arrows}"
    ))
    .unwrap()
}

fn owners(pairs: &[(&str, &str)]) -> Ownership {
    Ownership {
        owners: pairs
            .iter()
            .map(|&(file, owner)| (PathBuf::from(file), node(owner)))
            .collect(),
        ..Ownership::default()
    }
}

/// One import per `(from file, line, to file, kind, test_only)`.
fn sites(sites: &[(&str, u32, &str, ImportKind, bool)]) -> BTreeMap<PathBuf, FileImports> {
    let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
    for &(from, line, to, kind, test_only) in sites {
        map.entry(PathBuf::from(from)).or_default().resolved.push(
            ResolvedImport::new(at(from, line), PathBuf::from(to))
                .with_kind(kind)
                .with_test_only(test_only),
        );
    }
    map
}

fn lint(
    contract: &Contract,
    owned: &[(&str, &str)],
    imports: &BTreeMap<PathBuf, FileImports>,
) -> Vec<Violation> {
    reconcile(
        contract,
        &owners(owned),
        imports,
        Path::new("architecture.mmd"),
    )
    .violations
}

fn kinds(found: &[Violation]) -> Vec<(ViolationKind, Severity)> {
    found.iter().map(|v| (v.kind, v.severity)).collect()
}

/// The four sites a pair can have: runtime or type-only, in production or in tests.
const SITES: [(ImportKind, bool); 4] = [
    (ImportKind::Runtime, false),
    (ImportKind::TypeOnly, false),
    (ImportKind::Runtime, true),
    (ImportKind::TypeOnly, true),
];

#[test]
fn every_kind_on_every_form_lints_like_the_node_edge_it_stands_for() {
    let forms = [
        ("ga {arrow} gb", "boundary to boundary"),
        ("a {arrow} gb", "node to boundary"),
        ("ga {arrow} b", "boundary to node"),
    ];
    let owned = [("a/x.ts", "a"), ("b/y.ts", "b")];
    for kind in KINDS {
        for (kind_site, test_only) in SITES {
            let imports = sites(&[("a/x.ts", 3, "b/y.ts", kind_site, test_only)]);
            let expected = lint(
                &single(&format!("  a {} b\n", kind.arrow())),
                &owned,
                &imports,
            );
            for (form, what) in forms {
                let arrows = format!("  {}\n", form.replace("{arrow}", kind.arrow()));
                let found = lint(&single(&arrows), &owned, &imports);
                assert_eq!(
                    kinds(&found),
                    kinds(&expected),
                    "{what}, {kind:?}, {kind_site:?} test_only={test_only}"
                );
            }
        }
    }
}

#[test]
fn a_pair_nothing_covers_is_undeclared_exactly_as_before() {
    let contract = double("  ga --> gb\n");
    let owned = [("a1/x.ts", "a1"), ("a2/x.ts", "a2"), ("b1/y.ts", "b1")];
    let imports = sites(&[("b1/y.ts", 7, "a1/x.ts", ImportKind::Runtime, false)]);
    let found = lint(&contract, &owned, &imports);
    let undeclared: Vec<&Violation> = found
        .iter()
        .filter(|v| v.kind == ViolationKind::UndeclaredEdge)
        .collect();
    assert_eq!(undeclared.len(), 1);
    assert_eq!(
        undeclared[0].message,
        "`b1` imports `a1` (a1/x.ts) but architecture.mmd declares no edge b1 --> a1"
    );
    assert_eq!(undeclared[0].at, Some(at("b1/y.ts", 7)));
}

#[test]
fn a_boundary_edge_is_stale_only_when_none_of_its_pairs_is_realised() {
    let contract = double("  ga --> gb\n");
    let owned = [("a1/x.ts", "a1"), ("b2/y.ts", "b2")];
    assert_eq!(
        kinds(&lint(&contract, &owned, &BTreeMap::new())),
        [(ViolationKind::StaleEdge, Severity::Warning)]
    );
    let one = sites(&[("a1/x.ts", 1, "b2/y.ts", ImportKind::Runtime, false)]);
    assert_eq!(lint(&contract, &owned, &one), Vec::new());
}

#[test]
fn a_restricted_boundary_edge_is_stale_when_only_sites_it_does_not_allow_realise_it() {
    let owned = [("a1/x.ts", "a1"), ("b1/y.ts", "b1")];
    let runtime = sites(&[("a1/x.ts", 1, "b1/y.ts", ImportKind::Runtime, false)]);
    let found = lint(&double("  ga -->|type| gb\n"), &owned, &runtime);
    let stale: Vec<&Violation> = found
        .iter()
        .filter(|v| v.kind == ViolationKind::StaleEdge)
        .collect();
    assert_eq!(stale.len(), 1);
    assert_eq!(
        stale[0].message,
        "edge ga -->|type| gb is declared but no type-only import realises it"
    );
    let typed = sites(&[("a1/x.ts", 1, "b1/y.ts", ImportKind::TypeOnly, false)]);
    assert_eq!(
        lint(&double("  ga -->|type| gb\n"), &owned, &typed),
        Vec::new()
    );
}

#[test]
fn a_planned_boundary_edge_is_planned_realized_once_any_covered_pair_is_realised() {
    let owned = [("a2/x.ts", "a2"), ("b1/y.ts", "b1")];
    let contract = double("  ga -.-> gb\n");
    assert_eq!(lint(&contract, &owned, &BTreeMap::new()), Vec::new());
    let one = sites(&[("a2/x.ts", 1, "b1/y.ts", ImportKind::Runtime, false)]);
    let found = lint(&contract, &owned, &one);
    assert_eq!(
        kinds(&found),
        [(ViolationKind::PlannedRealized, Severity::Warning)]
    );
    assert_eq!(
        found[0].message,
        "planned edge ga -.-> gb is now realised by code"
    );
}

#[test]
fn a_solid_boundary_edge_realised_only_by_tests_is_a_test_only_edge() {
    let owned = [("a1/x.ts", "a1"), ("b1/y.ts", "b1"), ("b2/y.ts", "b2")];
    let imports = sites(&[
        ("a1/x.ts", 1, "b1/y.ts", ImportKind::Runtime, true),
        ("a1/x.ts", 2, "b2/y.ts", ImportKind::Runtime, true),
    ]);
    let found = lint(&double("  ga --> gb\n"), &owned, &imports);
    assert_eq!(
        kinds(&found),
        [(ViolationKind::TestOnlyEdge, Severity::Warning)]
    );
    let mixed = sites(&[
        ("a1/x.ts", 1, "b1/y.ts", ImportKind::Runtime, true),
        ("a1/x.ts", 2, "b2/y.ts", ImportKind::Runtime, false),
    ]);
    assert_eq!(lint(&double("  ga --> gb\n"), &owned, &mixed), Vec::new());
}

#[test]
fn a_solid_node_edge_beside_a_test_boundary_edge_wins_and_each_edge_is_judged_alone() {
    let owned = [("a1/x.ts", "a1"), ("b1/y.ts", "b1")];
    let imports = sites(&[("a1/x.ts", 1, "b1/y.ts", ImportKind::Runtime, false)]);
    let contract = double("  a1 --> b1\n  ga -->|test| gb\n");
    let found = lint(&contract, &owned, &imports);
    // why: the production site is allowed by the solid edge, and the test edge has no test site.
    assert_eq!(
        kinds(&found),
        [(ViolationKind::StaleEdge, Severity::Warning)]
    );
    assert_eq!(
        found[0].message,
        "edge ga -->|test| gb is declared but no test import realises it"
    );
}

#[test]
fn any_covering_edge_that_allows_the_site_is_enough() {
    let owned = [("a1/x.ts", "a1"), ("b1/y.ts", "b1")];
    let contract = double("  ga -->|test| gb\n  a1 -->|type| gb\n");
    let typed = sites(&[("a1/x.ts", 1, "b1/y.ts", ImportKind::TypeOnly, false)]);
    let found = lint(&contract, &owned, &typed);
    assert!(
        found
            .iter()
            .all(|v| v.kind != ViolationKind::UndeclaredEdge),
        "{found:?}"
    );
    let test = sites(&[("a1/x.ts", 1, "b1/y.ts", ImportKind::Runtime, true)]);
    let found = lint(&contract, &owned, &test);
    assert!(
        found
            .iter()
            .all(|v| v.kind != ViolationKind::UndeclaredEdge),
        "{found:?}"
    );
    let runtime = sites(&[("a1/x.ts", 1, "b1/y.ts", ImportKind::Runtime, false)]);
    let found = lint(&contract, &owned, &runtime);
    let undeclared = found
        .iter()
        .find(|v| v.kind == ViolationKind::UndeclaredEdge)
        .unwrap();
    assert!(
        undeclared
            .message
            .contains("declares only a type edge a1 -->|type| gb (line 11)"),
        "{}",
        undeclared.message
    );
    assert!(
        undeclared
            .message
            .contains("one more covering edge does not allow it either"),
        "{}",
        undeclared.message
    );
}

/// Whether a node edge of kind `node` is redundant under a boundary edge of kind `cover`.
fn expected_redundant(node: EdgeKind, cover: EdgeKind) -> bool {
    let wide = if !cover.is_restricted() {
        true
    } else if !node.is_restricted() {
        false
    } else {
        (!node.is_type() || cover.is_type()) && (!node.is_test() || cover.is_test())
    };
    wide && (node.is_planned() || !cover.is_planned())
}

#[test]
fn a_node_edge_is_redundant_exactly_under_a_boundary_edge_at_least_as_wide() {
    for node_kind in KINDS {
        for cover in KINDS {
            let contract = double(&format!(
                "  a1 {} b1\n  ga {} gb\n",
                node_kind.arrow(),
                cover.arrow()
            ));
            let found = lint(&contract, &[], &BTreeMap::new());
            let redundant: Vec<&Violation> = found
                .iter()
                .filter(|v| v.kind == ViolationKind::RedundantEdge)
                .collect();
            assert_eq!(
                redundant.len(),
                usize::from(expected_redundant(node_kind, cover)),
                "{node_kind:?} under {cover:?}"
            );
            for finding in redundant {
                assert_eq!(finding.severity, Severity::Warning);
                assert_eq!(finding.at, None);
                assert_eq!(
                    finding.message,
                    format!(
                        "edge a1 {} b1 is redundant: ga {} gb (line 11) already covers it",
                        node_kind.arrow(),
                        cover.arrow()
                    )
                );
            }
        }
    }
}

#[test]
fn the_first_wide_enough_cover_is_the_one_named() {
    let contract = double("  a1 --> b1\n  ga -->|type, test| gb\n  a1 --> gb\n");
    let found = lint(&contract, &[], &BTreeMap::new());
    let redundant = found
        .iter()
        .find(|v| v.kind == ViolationKind::RedundantEdge)
        .unwrap();
    assert!(
        redundant.message.contains("a1 --> gb (line 12)"),
        "{}",
        redundant.message
    );
}

#[test]
fn a_test_boundary_edge_exempts_test_sites_from_the_layer_order() {
    let src = "flowchart LR
  subgraph up[\"up\"]
    top
  end
  subgraph down[\"down\"]
    low
  end
  %% order: up > down
  down -->|test| up
";
    let contract = diagram::parse(src).unwrap();
    let owned = [("top/a.ts", "top"), ("low/a.ts", "low")];
    let test = sites(&[("low/a.ts", 1, "top/a.ts", ImportKind::Runtime, true)]);
    let found = lint(&contract, &owned, &test);
    assert!(
        found.iter().all(|v| v.kind != ViolationKind::UpwardEdge),
        "{found:?}"
    );
    let production = sites(&[("low/a.ts", 1, "top/a.ts", ImportKind::Runtime, false)]);
    let found = lint(&contract, &owned, &production);
    assert!(found.iter().any(|v| v.kind == ViolationKind::UpwardEdge));
}
