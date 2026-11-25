//! Open layer order ("Open order" in `docs/DESIGN.md`): what lint says with the pure core.

use std::fmt::Write as _;
use std::path::Path;

use super::fixtures::{at, imports, ownership};
use super::*;
use crate::diagram::parse;
use crate::model::{Violation, ViolationKind};

const LAYERS: [&str; 7] = [
    "app", "features", "shared", "core", "domain", "infra", "base",
];

/// Seven layers `app` ... `base`, one node each (`n_app` ...), the open chain on line 25, and the
/// `extra` lines after it. `f2` is a second node of `features` and `c2` one of `core`.
fn contract(extra: &str) -> Contract {
    let mut text = String::from("flowchart LR\n");
    for layer in LAYERS {
        let _ = write!(text, "  subgraph {layer}[\"{layer}\"]\n    n_{layer}\n");
        if layer == "features" {
            text.push_str("    f2\n");
        }
        if layer == "core" {
            text.push_str("    c2\n");
        }
        text.push_str("  end\n");
    }
    let _ = writeln!(text, "  %% order open: {}", LAYERS.join(" > "));
    text.push_str(extra);
    parse(&text).unwrap()
}

fn dir_of(name: &str) -> String {
    name.to_owned()
}

/// The findings of imports `(from node, to node)`, one per pair, at line 3 of the importer.
fn findings(contract: &Contract, pairs: &[(&str, &str)]) -> Vec<Violation> {
    let owners: Vec<(String, String)> = contract
        .nodes
        .keys()
        .map(|id| (format!("{}/a.ts", dir_of(id.as_str())), id.to_string()))
        .collect();
    let owners: Vec<(&str, &str)> = owners
        .iter()
        .map(|(f, o)| (f.as_str(), o.as_str()))
        .collect();
    let sites: Vec<(String, String)> = pairs
        .iter()
        .map(|(from, to)| (format!("{from}/a.ts"), format!("{to}/a.ts")))
        .collect();
    let sites: Vec<(&str, u32, &str)> = sites
        .iter()
        .map(|(f, t)| (f.as_str(), 3, t.as_str()))
        .collect();
    reconcile(
        contract,
        &ownership(&owners, &[]),
        &imports(&sites),
        Path::new("c.mmd"),
    )
    .violations
}

fn kinds(found: &[Violation]) -> Vec<ViolationKind> {
    found.iter().map(|v| v.kind).collect()
}

#[test]
fn a_downward_import_across_three_layers_needs_no_arrow() {
    let contract = contract("");
    assert_eq!(kinds(&findings(&contract, &[("n_app", "n_core")])), []);
    assert_eq!(kinds(&findings(&contract, &[("n_features", "n_base")])), []);
}

#[test]
fn an_upward_import_is_reported_and_not_excused() {
    let found = findings(&contract(""), &[("n_core", "n_app")]);
    assert_eq!(kinds(&found), [ViolationKind::UpwardEdge]);
}

#[test]
fn imports_inside_one_layer_still_need_an_arrow() {
    let found = findings(&contract(""), &[("n_features", "f2")]);
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
}

#[test]
fn one_arrow_narrows_its_pair_of_layers_only() {
    let contract = contract("  n_features --> n_core\n");
    let found = findings(
        &contract,
        &[
            ("n_features", "n_core"),
            ("f2", "c2"),
            ("f2", "n_shared"),
            ("n_features", "n_domain"),
            ("n_app", "c2"),
        ],
    );
    assert_eq!(kinds(&found), [ViolationKind::UndeclaredEdge]);
    assert_eq!(found[0].at, Some(at("f2/a.ts", 3)));
    let message = &found[0].message;
    assert!(message.contains("declares no edge f2 --> c2"), "{message}");
    assert!(
        message.contains("`%% order open` on line 25 limits `features` to the edges it declares towards `core` (1 declared) and this pair is not among them"),
        "{message}"
    );
}

#[test]
fn a_test_type_dotted_or_boundary_edge_narrows_too() {
    for extra in [
        "  n_features -->|test| n_core\n",
        "  n_features -->|type| n_core\n",
        "  n_features -.-> n_core\n",
        "  n_features --> core\n",
    ] {
        let found = findings(&contract(extra), &[("f2", "c2")]);
        assert!(
            found
                .iter()
                .any(|v| v.kind == ViolationKind::UndeclaredEdge),
            "{extra}: {found:?}"
        );
    }
}

#[test]
fn a_declared_arrow_the_allowance_would_cover_is_not_redundant() {
    let found = findings(
        &contract("  n_features --> n_core\n"),
        &[("n_features", "n_core")],
    );
    assert_eq!(kinds(&found), []);
}

#[test]
fn a_declared_narrowing_arrow_nothing_uses_is_stale() {
    let found = findings(&contract("  n_features --> n_core\n"), &[]);
    assert_eq!(kinds(&found), [ViolationKind::StaleEdge]);
}

#[test]
fn a_plain_chain_grants_no_allowance() {
    let mut contract = contract("");
    contract.rules.order[0].open = false;
    assert_eq!(
        kinds(&findings(&contract, &[("n_app", "n_core")])),
        [ViolationKind::UndeclaredEdge]
    );
}

#[test]
fn a_boundary_in_an_open_and_a_plain_chain_gets_the_allowance_from_the_open_one() {
    let mut text = String::from("flowchart LR\n");
    for layer in ["a", "b", "c"] {
        let _ = write!(
            text,
            "  subgraph {layer}[\"{layer}\"]\n    n_{layer}\n  end\n"
        );
    }
    text.push_str("  %% order open: a > b\n  %% order: b > c\n");
    let contract = parse(&text).unwrap();
    assert_eq!(kinds(&findings(&contract, &[("n_a", "n_b")])), []);
    assert_eq!(
        kinds(&findings(&contract, &[("n_b", "n_c")])),
        [ViolationKind::UndeclaredEdge]
    );
}

#[test]
fn the_allowance_makes_no_cycle() {
    // why: only declared edges count, so an open chain with an acyclic arrow set is acyclic.
    let _ = contract("  n_features --> n_core\n  %% no-cycles\n");
}
