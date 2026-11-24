//! Nested boundaries in the contract: membership, rendering, and the contract errors of
//! "Nested boundaries", "Layer order", "No cycles" and "Isolated boundaries".

use super::*;
use crate::model::{Edge, Endpoint, NodeId, children};

/// `features` holds `billing` (two nodes, one of them in `invoices`, three levels deep), `search`
/// (one node) and `shell` written directly in it; `shared` is beside it.
const FIXTURE: &str = "flowchart LR
  subgraph features[\"Features\"]
    subgraph billing[\"Billing\"]
      bill-ui
      subgraph invoices[\"Invoices\"]
        invoice-svc
      end
    end
    subgraph search[\"Search\"]
      search-svc
    end
    shell
  end
  subgraph shared[\"Shared\"]
    util
  end
";

fn with(extra: &str) -> String {
    format!("{FIXTURE}{extra}")
}

fn error(src: &str) -> DiagramError {
    parse(src).unwrap_err()
}

fn id(raw: &str) -> NodeId {
    NodeId::new(raw).unwrap()
}

fn pair(from: &str, to: &str) -> Edge {
    Edge {
        from: id(from),
        to: id(to),
    }
}

#[test]
fn membership_is_transitive_to_any_depth() {
    let contract = parse(FIXTURE).unwrap();
    let nodes = &contract.nodes;
    let members = |boundary: &str| -> Vec<String> {
        Endpoint::Boundary(boundary.to_owned())
            .members(nodes)
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    assert_eq!(
        members("features"),
        ["bill-ui", "invoice-svc", "search-svc", "shell"]
    );
    assert_eq!(members("billing"), ["bill-ui", "invoice-svc"]);
    assert_eq!(members("invoices"), ["invoice-svc"]);
    assert_eq!(members("shared"), ["util"]);
    let deep = &nodes[&id("invoice-svc")];
    assert_eq!(deep.boundary.as_deref(), Some("invoices"));
    assert_eq!(deep.outer, ["features", "billing"]);
}

#[test]
fn the_children_of_a_boundary_are_its_child_boundaries_and_its_own_nodes() {
    let contract = parse(FIXTURE).unwrap();
    assert_eq!(
        children(&contract.nodes, "features"),
        ["billing", "search", "shell"]
    );
    assert_eq!(
        children(&contract.nodes, "billing"),
        ["bill-ui", "invoices"]
    );
    assert_eq!(children(&contract.nodes, "invoices"), ["invoice-svc"]);
    assert_eq!(children(&contract.nodes, "shared"), ["util"]);
}

#[test]
fn nested_contracts_render_as_nested_subgraphs_and_round_trip() {
    let contract = parse(&with("  features --> shared\n  billing --> search\n")).unwrap();
    let rendered = render(&contract);
    assert!(rendered.contains("  subgraph features[\"features\"]\n    subgraph billing[\"billing\"]\n      subgraph invoices[\"invoices\"]\n        invoice-svc\n"));
    let again = parse(&rendered).unwrap();
    assert_eq!(again.nodes, contract.nodes);
    assert_eq!(again.boundary_edges.len(), 2);
    assert_eq!(render(&again), rendered);
}

#[test]
fn ids_are_one_namespace_at_every_level() {
    let node_as_inner =
        "flowchart LR\n  subgraph a\n    b\n    subgraph b\n      c\n    end\n  end\n";
    assert!(matches!(
        error(node_as_inner),
        DiagramError::IdClash {
            line: 4,
            first_is_boundary: false,
            ..
        }
    ));
    let boundary_twice = "flowchart LR\n  subgraph a\n    subgraph a\n      c\n    end\n  end\n";
    assert!(matches!(
        error(boundary_twice),
        DiagramError::DuplicateBoundary { line: 3, .. }
    ));
    let node_in_two =
        "flowchart LR\n  subgraph a\n    n\n    subgraph b\n      n\n    end\n  end\n";
    assert!(matches!(
        error(node_in_two),
        DiagramError::DuplicateNode { line: 5, .. }
    ));
}

#[test]
fn malformed_nesting_is_a_contract_error_with_its_line() {
    assert_eq!(
        error("flowchart LR\n  subgraph a\n    n\n  end\n  end\n"),
        DiagramError::StrayEnd { line: 5 }
    );
    assert_eq!(
        error("flowchart LR\n  subgraph a\n    subgraph b\n      n\n    end\n"),
        DiagramError::UnclosedSubgraph {
            line: 2,
            id: "a".to_owned()
        }
    );
}

#[test]
fn a_boundary_edge_covers_every_node_at_any_depth_and_an_inner_one_only_its_own() {
    let contract = parse(&with("  features --> shared\n  billing --> shared\n")).unwrap();
    let nodes = &contract.nodes;
    assert_eq!(contract.boundary_edges[0].pairs(nodes).len(), 4);
    assert_eq!(contract.boundary_edges[1].pairs(nodes).len(), 2);
    assert!(
        contract.boundary_edges[1]
            .pairs(nodes)
            .contains(&pair("invoice-svc", "util"))
    );
    assert!(
        !contract.boundary_edges[1]
            .pairs(nodes)
            .contains(&pair("search-svc", "util"))
    );
}

#[test]
fn a_boundary_edge_between_nested_ends_covers_distinct_pairs() {
    // why: `features --> billing` is every member of `features` using every member of `billing`.
    let contract = parse(&with("  features --> billing\n")).unwrap();
    let pairs = contract.boundary_edges[0].pairs(&contract.nodes);
    // why: 4 features members x 2 billing members, minus the 2 pairs of a node with itself.
    assert_eq!(pairs.len(), 6);
    assert!(pairs.contains(&pair("bill-ui", "invoice-svc")));
    assert!(pairs.contains(&pair("shell", "bill-ui")));
    assert!(parse(&with("  billing --> features\n")).is_ok());
    assert!(parse(&with("  features --> features\n")).is_ok());
    assert!(parse(&with("  invoice-svc --> features\n")).is_ok());
}

#[test]
fn an_edge_to_a_boundary_with_no_node_at_any_depth_is_an_error() {
    let src = "flowchart LR\n  subgraph a\n    subgraph b\n    end\n  end\n  n\n  n --> a\n";
    assert!(matches!(
        error(src),
        DiagramError::EmptyBoundary { line: 7, .. }
    ));
}

#[test]
fn order_lists_an_outer_boundary_as_one_layer_and_rejects_listing_both() {
    let ok = parse(&with("  %% order: features > shared\n")).unwrap();
    let order = &ok.rules.order[0];
    assert_eq!(
        order
            .place(&ok.nodes[&id("invoice-svc")])
            .map(|(boundary, rank)| (boundary.to_owned(), rank)),
        Some(("features".to_owned(), 0))
    );
    assert_eq!(ok.rules.layer(&ok.nodes[&id("util")]).unwrap().rank, 1);
    let layer = ok.rules.layer(&ok.nodes[&id("invoice-svc")]).unwrap();
    assert_eq!(
        (layer.boundary.as_str(), layer.rank, layer.of),
        ("features", 0, 2)
    );
    for chain in [
        "features > billing",
        "billing > features",
        "shared > features > invoices",
    ] {
        match error(&with(&format!("  %% order: {chain}\n"))) {
            DiagramError::MalformedRule { rule, reason, .. } => {
                assert_eq!(rule, "order");
                assert!(reason.contains("contains"), "{chain}: {reason}");
            }
            other => panic!("{chain}: {other:?}"),
        }
    }
}

#[test]
fn two_chains_may_list_a_boundary_and_one_nested_in_it() {
    let contract = parse(&with(
        "  %% order: features > shared\n  %% order: billing > search\n",
    ))
    .unwrap();
    assert_eq!(contract.rules.order.len(), 2);
}

#[test]
fn an_inner_boundary_orders_only_what_a_chain_lists() {
    let contract = parse(&with("  %% order: billing > search > shared\n")).unwrap();
    let order = &contract.rules.order[0];
    assert_eq!(order.place(&contract.nodes[&id("shell")]), None);
    assert_eq!(
        order.place(&contract.nodes[&id("invoice-svc")]),
        Some(("billing", 0))
    );
    // why: `search-svc` is below `billing`, so a declared arrow upward is the error.
    assert!(matches!(
        error(&with(
            "  %% order: billing > search > shared\n  search --> billing\n"
        )),
        DiagramError::UpwardArrow { .. }
    ));
}

#[test]
fn an_outer_boundary_edge_points_upward_through_its_members() {
    assert!(matches!(
        error(&with(
            "  %% order: features > shared\n  shared --> billing\n"
        )),
        DiagramError::UpwardArrow { .. }
    ));
    assert!(
        parse(&with(
            "  %% order: features > shared\n  billing --> shared\n"
        ))
        .is_ok()
    );
}

#[test]
fn no_cycles_boundaries_checks_every_level_and_skips_nested_pairs() {
    let ok = "  %% no-cycles: boundaries\n  bill-ui --> shell\n  shell --> invoice-svc\n  billing --> features\n";
    // why: a boundary never has an edge to one around it or inside it.
    assert!(parse(&with(ok)).is_ok());
    let cycle = with(
        "  %% no-cycles: boundaries\n  bill-ui --> search-svc\n  search-svc --> invoice-svc\n",
    );
    match error(&cycle) {
        DiagramError::DeclaredCycle { what, cycle, .. } => {
            assert_eq!(what, "boundary");
            assert_eq!(cycle, "billing --> search --> billing");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_cycles_boundaries_finds_a_cycle_between_inner_boundaries_across_outer_ones() {
    let src = "flowchart LR
  subgraph a
    subgraph a1
      x1
    end
  end
  subgraph b
    subgraph b1
      y1
    end
  end
  %% no-cycles: boundaries
  x1 --> y1
  y1 --> x1
";
    match error(src) {
        DiagramError::DeclaredCycle { cycle, .. } => {
            assert_eq!(cycle, "a --> b --> a");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn isolated_parses_a_list_and_renders_back() {
    let contract = parse(&with("  %% isolated: features, billing\n")).unwrap();
    assert_eq!(contract.rules.isolated.len(), 2);
    assert_eq!(contract.rules.isolated["features"], 17);
    let again = parse(&render(&contract)).unwrap();
    assert_eq!(
        again.rules.isolated.keys().collect::<Vec<_>>(),
        ["billing", "features"]
    );
    assert_eq!(contract.rules.summary(&contract.nodes).isolated.len(), 2);
}

#[test]
fn a_malformed_isolated_line_is_a_contract_error() {
    for line in [
        "%% isolated:",
        "%% isolated: shell",
        "%% isolated: nope",
        "%% isolated: features, features",
        "%% isolated: features\n  %% isolated: features",
        "%% isolated: a b",
        "%% isolated: features,",
    ] {
        let found = error(&with(&format!("  {line}\n")));
        assert!(
            matches!(
                found,
                DiagramError::MalformedRule { .. } | DiagramError::InvalidId { .. }
            ),
            "{line}: {found:?}"
        );
    }
}

#[test]
fn an_arrow_between_two_children_is_a_contract_error_of_any_kind() {
    for arrow in [
        "bill-ui --> search-svc",
        "bill-ui -.-> search-svc",
        "bill-ui -->|test| search-svc",
        "bill-ui -->|type| search-svc",
        "invoice-svc --> shell",
        "billing --> search",
        "billing --> shell",
        "bill-ui --> search",
        "billing -.-> search",
        "features --> features",
        "features --> billing",
        "billing --> features",
    ] {
        match error(&with(&format!("  %% isolated: features\n  {arrow}\n"))) {
            DiagramError::IsolatedArrow {
                line,
                boundary,
                isolated_line,
                ..
            } => {
                assert_eq!(
                    (line, boundary.as_str(), isolated_line),
                    (18, "features", 17),
                    "{arrow}"
                );
            }
            other => panic!("{arrow}: {other:?}"),
        }
    }
}

#[test]
fn an_arrow_inside_one_child_or_out_of_the_isolated_boundary_is_fine() {
    for arrow in [
        "bill-ui --> invoice-svc",
        "billing --> billing",
        "billing --> invoices",
        "invoices --> bill-ui",
        "features --> shared",
        "shared --> features",
        "shell --> util",
        "billing --> util",
    ] {
        let src = with(&format!("  %% isolated: features\n  {arrow}\n"));
        assert!(parse(&src).is_ok(), "{arrow}: {:?}", parse(&src).err());
    }
}

#[test]
fn the_first_arrow_by_line_is_the_one_reported() {
    let src = with("  %% isolated: features\n  billing --> search\n  bill-ui --> shell\n");
    assert!(matches!(
        error(&src),
        DiagramError::IsolatedArrow { line: 18, .. }
    ));
}

#[test]
fn nested_isolation_isolates_the_children_of_each_listed_boundary() {
    // why: `billing` is isolated too, so its two children may not use each other either.
    let src = with("  %% isolated: features, billing\n  bill-ui --> invoice-svc\n");
    match error(&src) {
        DiagramError::IsolatedArrow {
            boundary, apart, ..
        } => {
            assert_eq!(boundary, "billing");
            assert_eq!(*apart, ("bill-ui".to_owned(), "invoices".to_owned()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_isolated_boundary_names_its_children_in_the_summary() {
    let contract = parse(&with("  %% isolated: features\n")).unwrap();
    let summary = contract.rules.summary(&contract.nodes);
    assert_eq!(summary.isolated[0].boundary, "features");
    assert_eq!(summary.isolated[0].children, ["billing", "search", "shell"]);
}
