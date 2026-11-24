//! Boundary edges in the contract: what parses, what renders and the contract errors of
//! "Boundary edges", "Layer order" and "No cycles".

use super::*;
use crate::model::{Edge, EdgeKind, Endpoint, NodeId};

/// Six feature nodes in `features`, four nodes in `core`, one in `ui`, and `solo` in none.
const FIXTURE: &str = "flowchart LR
  subgraph ui[\"UI\"]
    shell
  end
  subgraph features[\"Features\"]
    f1
    f2
    f3
    f4
    f5
    f6
  end
  subgraph core[\"Core\"]
    c1
    c2
    c3
    c4
  end
  solo
";

fn with(extra: &str) -> String {
    format!("{FIXTURE}{extra}")
}

fn error(src: &str) -> DiagramError {
    parse(src).unwrap_err()
}

fn node(raw: &str) -> Endpoint {
    Endpoint::Node(NodeId::new(raw).unwrap())
}

fn sub(raw: &str) -> Endpoint {
    Endpoint::Boundary(raw.to_owned())
}

#[test]
fn an_edge_may_join_two_boundaries_a_node_and_a_boundary_or_the_reverse() {
    let contract = parse(&with(
        "  features --> core\n  solo --> core\n  features --> solo\n",
    ))
    .unwrap();
    assert!(contract.edges.is_empty());
    let ends: Vec<_> = contract
        .boundary_edges
        .iter()
        .map(|declared| (declared.from.clone(), declared.to.clone(), declared.line))
        .collect();
    assert_eq!(
        ends,
        [
            (sub("features"), sub("core"), 20),
            (node("solo"), sub("core"), 21),
            (sub("features"), node("solo"), 22),
        ]
    );
}

#[test]
fn every_arrow_and_label_works_on_a_boundary_edge() {
    let kinds = [
        ("-->", EdgeKind::Implemented),
        ("-.->", EdgeKind::Planned),
        ("-->|test|", EdgeKind::Test),
        ("-.->|test|", EdgeKind::PlannedTest),
        ("-->|type|", EdgeKind::Type),
        ("-.->|type|", EdgeKind::PlannedType),
        ("-->|type, test|", EdgeKind::TypeTest),
        ("-.->|test, type|", EdgeKind::PlannedTypeTest),
        ("-->|display text|", EdgeKind::Implemented),
    ];
    for (arrow, kind) in kinds {
        let contract = parse(&with(&format!("  features {arrow} core\n"))).unwrap();
        assert_eq!(contract.boundary_edges[0].kind, kind, "{arrow}");
    }
}

#[test]
fn an_edge_between_two_nodes_stays_a_node_edge() {
    let contract = parse(&with("  f1 --> c1\n")).unwrap();
    assert_eq!(contract.edges.len(), 1);
    assert_eq!(contract.boundary_edges, Vec::new());
}

#[test]
fn a_boundary_may_be_declared_after_the_edge_that_names_it() {
    let contract =
        parse("flowchart LR\n  a --> g\n  subgraph g[\"G\"]\n    b\n  end\n  a\n").unwrap();
    assert_eq!(contract.boundary_edges.len(), 1);
}

#[test]
fn a_boundary_to_itself_is_allowed_and_a_node_to_itself_is_not() {
    let contract = parse(&with("  features --> features\n")).unwrap();
    assert_eq!(contract.boundary_edges[0].pairs(&contract.nodes).len(), 30);
    assert!(matches!(
        error(&with("  f1 --> f1\n")),
        DiagramError::SelfEdge { line: 20, .. }
    ));
}

#[test]
fn a_node_may_point_at_its_own_boundary() {
    let contract = parse(&with("  f1 --> features\n")).unwrap();
    assert_eq!(contract.boundary_edges[0].pairs(&contract.nodes).len(), 5);
}

#[test]
fn an_undeclared_endpoint_is_still_an_error_naming_the_line() {
    assert_eq!(
        error(&with("  features --> ghost\n")),
        DiagramError::UndeclaredNode {
            line: 20,
            id: NodeId::new("ghost").unwrap()
        }
    );
}

#[test]
fn a_boundary_with_no_node_cannot_be_an_endpoint() {
    let src = "flowchart LR\n  subgraph empty[\"E\"]\n  end\n  a\n  a --> empty\n";
    assert_eq!(
        error(src),
        DiagramError::EmptyBoundary {
            line: 5,
            id: "empty".to_owned()
        }
    );
    let src = "flowchart LR\n  subgraph empty[\"E\"]\n  end\n  empty --> empty\n";
    assert!(matches!(error(src), DiagramError::EmptyBoundary { .. }));
}

#[test]
fn the_same_ends_twice_are_a_duplicate_whatever_the_kind() {
    for second in ["-->", "-.->", "-->|test|"] {
        let err = error(&with(&format!(
            "  features --> core\n  features {second} core\n"
        )));
        assert_eq!(
            err,
            DiagramError::DuplicateEdge {
                line: 21,
                from: sub("features"),
                to: sub("core"),
                first_line: 20
            },
            "{second}"
        );
    }
}

#[test]
fn overlapping_edges_with_different_ends_are_allowed() {
    let src = with("  features --> core\n  f1 --> core\n  f1 --> c1\n  core --> features\n");
    let contract = parse(&src).unwrap();
    assert_eq!(contract.boundary_edges.len(), 3);
    assert_eq!(contract.edges.len(), 1);
}

#[test]
fn boundary_edges_survive_a_render_round_trip_and_render_the_same_every_time() {
    let src = with(
        "  f1 --> c1\n  solo -.-> core\n  features -->|type, test| core\n  core --> features\n  features --> features\n",
    );
    let contract = parse(&src).unwrap();
    let rendered = render(&contract);
    assert!(
        rendered.contains("  features -->|type, test| core\n"),
        "{rendered}"
    );
    let again = parse(&rendered).unwrap();
    let ends = |contract: &crate::model::Contract| -> Vec<_> {
        contract
            .boundary_edges
            .iter()
            .map(crate::model::BoundaryEdge::text)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    assert_eq!(ends(&contract), ends(&again));
    assert_eq!(again.edges, contract.edges);
    assert_eq!(render(&again), rendered);
}

#[test]
fn a_boundary_edge_pointing_up_the_order_is_a_contract_error_at_its_line() {
    let src = with("  %% order: ui > features > core\n  core --> features\n");
    let err = error(&src);
    let DiagramError::UpwardArrow {
        line,
        from,
        to,
        order_line,
        relation,
    } = &err
    else {
        panic!("{err:?}");
    };
    assert_eq!((*line, *order_line), (21, 20));
    assert_eq!((from.as_str(), to.as_str()), ("core", "features"));
    assert_eq!(relation, "`core` is below `features`");
}

#[test]
fn a_node_and_a_boundary_edge_are_judged_on_their_pairs() {
    for edge in ["c1 --> features", "features --> shell", "c2 --> ui"] {
        let src = with(&format!("  %% order: ui > features > core\n  {edge}\n"));
        assert!(
            matches!(error(&src), DiagramError::UpwardArrow { line: 21, .. }),
            "{edge}"
        );
    }
}

#[test]
fn downward_sideways_and_unlisted_boundary_edges_are_fine() {
    let src = with(
        "  %% order: features > core\n  features --> core\n  features --> features\n  ui --> core\n  core --> ui\n  solo --> features\n  core --> solo\n",
    );
    assert!(parse(&src).is_ok());
}

#[test]
fn an_upward_boundary_test_edge_is_allowed_and_so_is_a_dotted_one_that_points_down() {
    let src =
        with("  %% order: ui > features > core\n  core -->|test| features\n  features -.-> core\n");
    assert!(parse(&src).is_ok());
    let src = with("  %% order: ui > features > core\n  core -.-> features\n");
    assert!(matches!(error(&src), DiagramError::UpwardArrow { .. }));
    let src = with("  %% order: ui > features > core\n  core -->|type| features\n");
    assert!(matches!(error(&src), DiagramError::UpwardArrow { .. }));
}

#[test]
fn the_earliest_upward_edge_of_either_kind_is_the_one_reported() {
    let src = with(
        "  %% order: ui > features > core\n  features --> core\n  core --> features\n  c1 --> f1\n",
    );
    assert!(matches!(
        error(&src),
        DiagramError::UpwardArrow { line: 22, .. }
    ));
    let src = with("  %% order: ui > features > core\n  c1 --> f1\n  core --> features\n");
    assert!(matches!(
        error(&src),
        DiagramError::UpwardArrow { line: 21, .. }
    ));
}

#[test]
fn no_cycles_expands_boundary_edges_to_node_pairs() {
    let src = with("  %% no-cycles\n  features --> core\n  c1 --> f1\n");
    let err = error(&src);
    let DiagramError::DeclaredCycle { line, what, cycle } = &err else {
        panic!("{err:?}");
    };
    assert_eq!((*line, what.as_str()), (20, "node"));
    assert_eq!(
        cycle,
        "c1 --> f1 (line 22), f1 --> c1 (via features --> core, line 21)"
    );
}

#[test]
fn a_cycle_between_two_boundary_edges_names_both_lines() {
    let src = with("  %% no-cycles\n  features --> core\n  core --> features\n");
    let err = error(&src);
    let DiagramError::DeclaredCycle { cycle, .. } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(
        cycle,
        "c1 --> f1 (via core --> features, line 22), f1 --> c1 (via features --> core, line 21)"
    );
}

#[test]
fn a_node_edge_is_named_before_a_boundary_edge_that_covers_the_same_pair() {
    let src = with("  %% no-cycles\n  features --> core\n  f1 --> c1\n  c1 --> f1\n");
    let err = error(&src);
    let DiagramError::DeclaredCycle { cycle, .. } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(cycle, "c1 --> f1 (line 23), f1 --> c1 (line 22)");
}

#[test]
fn test_boundary_edges_are_not_counted_for_cycles() {
    let src = with("  %% no-cycles\n  features --> core\n  core -->|test| features\n");
    assert!(parse(&src).is_ok());
    let src = with("  %% no-cycles\n  features --> core\n  core -->|type, test| features\n");
    assert!(matches!(error(&src), DiagramError::DeclaredCycle { .. }));
}

#[test]
fn a_boundary_to_itself_is_a_node_cycle_but_not_a_boundary_cycle() {
    let src = with("  %% no-cycles\n  features --> features\n");
    assert!(matches!(error(&src), DiagramError::DeclaredCycle { .. }));
    let src = with("  %% no-cycles: boundaries\n  features --> features\n");
    assert!(parse(&src).is_ok());
}

#[test]
fn the_boundary_graph_counts_boundary_edges() {
    let src = with("  %% no-cycles: boundaries\n  features --> core\n  c1 --> f1\n");
    let err = error(&src);
    let DiagramError::DeclaredCycle { what, cycle, .. } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(
        (what.as_str(), cycle.as_str()),
        ("boundary", "core --> features --> core")
    );
}

#[test]
fn a_node_edge_has_no_covering_boundary_edge_in_the_contract_value() {
    let contract = parse(&with("  f1 --> c1\n  features --> core\n")).unwrap();
    let pair = Edge {
        from: NodeId::new("f1").unwrap(),
        to: NodeId::new("c1").unwrap(),
    };
    assert_eq!(contract.covers(&pair).len(), 2);
}
