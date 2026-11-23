//! The rule directives of "Contract rules": what parses, and every contract error.

use super::*;
use crate::model::NodeId;

const LAYERS: &str = "flowchart LR
  subgraph entry[\"Entry\"]
    cli
  end
  subgraph engine[\"Engine\"]
    lint
  end
  subgraph shared[\"Shared\"]
    model
  end
  solo
";

fn with(extra: &str) -> String {
    format!("{LAYERS}{extra}")
}

fn error(src: &str) -> DiagramError {
    parse(src).unwrap_err()
}

#[test]
fn an_order_line_lists_boundaries_top_first() {
    let contract = parse(&with("  %% order: entry > engine > shared\n")).unwrap();
    let order = contract.rules.order.unwrap();
    assert_eq!(order.boundaries, ["entry", "engine", "shared"]);
    assert_eq!(order.line, 12);
    assert_eq!(order.rank("engine"), Some(1));
    assert_eq!(order.rank("nope"), None);
}

#[test]
fn a_contract_without_rule_lines_has_no_rules() {
    assert!(parse(LAYERS).unwrap().rules.is_empty());
    // why: a comment that merely starts like a directive stays plain.
    let plain = with("%% orders of magnitude\n%% order is not a directive without a colon\n");
    assert!(parse(&plain).unwrap().rules.is_empty());
}

#[test]
fn a_malformed_order_is_a_contract_error_at_its_line() {
    for bad in [
        "%% order:\n",
        "%% order: entry\n",
        "%% order: entry >\n",
        "%% order: entry > > shared\n",
        "%% order: entry > entry\n",
        "%% order: entry > ghost\n",
        "%% order: entry > lint\n",
        "%% order: entry > not an id\n",
    ] {
        let err = error(&with(bad));
        assert_eq!(err.line(), Some(12), "{bad}: {err}");
    }
}

#[test]
fn an_order_that_names_a_node_says_it_is_not_a_boundary() {
    let err = error(&with("%% order: entry > lint\n"));
    assert!(
        err.to_string().contains("`lint` is a node, not a boundary"),
        "{err}"
    );
    let err = error(&with("%% order: entry > ghost\n"));
    assert!(
        err.to_string()
            .contains("`ghost` is not a declared boundary"),
        "{err}"
    );
}

#[test]
fn a_second_order_line_is_a_contract_error_naming_the_first() {
    let err = error(&with(
        "%% order: entry > shared\n%% order: shared > entry\n",
    ));
    assert!(
        matches!(
            &err,
            DiagramError::DuplicateRule {
                line: 13,
                first_line: 12,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn an_upward_arrow_is_a_contract_error_at_the_arrow_naming_the_order() {
    let src = with("  %% order: entry > engine > shared\n  cli --> model\n  model --> lint\n");
    let err = error(&src);
    let DiagramError::UpwardArrow {
        line,
        from,
        to,
        order_line,
        ..
    } = &err
    else {
        panic!("{err:?}");
    };
    assert_eq!((*line, *order_line), (14, 12));
    assert_eq!((from.as_str(), to.as_str()), ("model", "lint"));
    assert!(
        err.to_string().contains("`shared` is below `engine`"),
        "{err}"
    );
}

#[test]
fn a_dotted_and_a_type_arrow_pointing_upward_are_errors_too() {
    for arrow in ["-.->", "-->|type|", "-->|type, test|", "-.->|type|"] {
        let src = with(&format!(
            "  %% order: entry > engine > shared\n  model {arrow} cli\n"
        ));
        assert!(
            matches!(error(&src), DiagramError::UpwardArrow { .. }),
            "{arrow}"
        );
    }
}

#[test]
fn an_upward_test_edge_is_allowed() {
    for arrow in ["-->|test|", "-.->|test|"] {
        let src = with(&format!(
            "  %% order: entry > engine > shared\n  model {arrow} cli\n"
        ));
        assert!(parse(&src).is_ok(), "{arrow}");
    }
}

#[test]
fn arrows_within_a_boundary_or_to_unlisted_nodes_are_unconstrained() {
    let src = with(
        "  %% order: entry > shared\n  lint --> cli\n  cli --> lint\n  model --> solo\n  solo --> cli\n",
    );
    assert!(parse(&src).is_ok());
}

#[test]
fn the_order_survives_a_render_round_trip() {
    let contract = parse(&with(
        "  %% order: entry > engine > shared\n  cli --> lint\n",
    ))
    .unwrap();
    let rendered = render(&contract);
    assert!(
        rendered.contains("  %% order: entry > engine > shared\n"),
        "{rendered}"
    );
    let again = parse(&rendered).unwrap();
    assert_eq!(
        again.rules.order.unwrap().boundaries,
        ["entry", "engine", "shared"]
    );
    assert_eq!(again.nodes.len(), 4);
    let id = NodeId::new("cli").unwrap();
    assert!(contract.nodes.contains_key(&id));
}

const GRAPH: &str = "flowchart LR
  subgraph one[\"One\"]
    a
    b
  end
  subgraph two[\"Two\"]
    c
  end
  d
";

#[test]
fn no_cycles_lines_are_read_and_stored_with_their_lines() {
    let contract = parse(&format!(
        "{GRAPH}  a --> c\n  %% no-cycles\n  %% no-cycles: boundaries\n"
    ))
    .unwrap();
    assert_eq!(contract.rules.no_cycles, Some(11));
    assert_eq!(contract.rules.no_cycles_boundaries, Some(12));
}

#[test]
fn an_acyclic_contract_passes_even_with_a_cycle_inside_a_boundary() {
    let src = format!("{GRAPH}  a --> b\n  b --> a\n  a --> c\n  %% no-cycles: boundaries\n");
    assert!(parse(&src).is_ok());
}

#[test]
fn a_declared_node_cycle_is_a_contract_error_listing_its_edges_and_lines() {
    let src = format!("{GRAPH}  a --> b\n  b --> c\n  c -.-> a\n  %% no-cycles\n");
    let err = parse(&src).unwrap_err();
    assert!(
        matches!(&err, DiagramError::DeclaredCycle { line: 13, .. }),
        "{err:?}"
    );
    assert!(
        err.to_string()
            .contains("a --> b (line 10), b --> c (line 11), c --> a (line 12)"),
        "{err}"
    );
}

#[test]
fn a_two_node_cycle_is_listed_as_two_edges() {
    let src = format!("{GRAPH}  a --> d\n  d --> a\n  %% no-cycles\n");
    let err = parse(&src).unwrap_err();
    assert!(
        err.to_string()
            .contains("a --> d (line 10), d --> a (line 11)"),
        "{err}"
    );
}

#[test]
fn test_edges_do_not_count_as_a_cycle_but_type_edges_do() {
    let test = format!("{GRAPH}  a --> c\n  c -->|test| a\n  %% no-cycles\n");
    assert!(parse(&test).is_ok());
    let planned_test = format!("{GRAPH}  a --> c\n  c -.->|test| a\n  %% no-cycles\n");
    assert!(parse(&planned_test).is_ok());
    for arrow in ["-->|type|", "-->|type, test|", "-.->"] {
        let src = format!("{GRAPH}  a --> c\n  c {arrow} a\n  %% no-cycles\n");
        assert!(
            matches!(error(&src), DiagramError::DeclaredCycle { .. }),
            "{arrow}"
        );
    }
}

#[test]
fn a_boundary_cycle_is_listed_by_boundary_and_needs_its_own_line() {
    let edges = "  a --> c\n  c --> b\n";
    let nodes_only = format!("{GRAPH}{edges}  %% no-cycles\n");
    assert!(parse(&nodes_only).is_ok());
    let err = error(&format!("{GRAPH}{edges}  %% no-cycles: boundaries\n"));
    assert!(
        matches!(&err, DiagramError::DeclaredCycle { line: 12, .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("boundary cycles"), "{err}");
    assert!(err.to_string().contains("one --> two --> one"), "{err}");
}

#[test]
fn nodes_outside_a_boundary_add_no_boundary_edge() {
    let src = format!("{GRAPH}  a --> d\n  d --> c\n  c --> d\n  %% no-cycles: boundaries\n");
    assert!(parse(&src).is_ok());
}

#[test]
fn malformed_and_repeated_no_cycles_lines_are_errors() {
    for bad in [
        "%% no-cycles: nodes\n",
        "%% no-cycles:\n",
        "%% no-cycles please\n",
        "%% no-cycles: boundaries now\n",
    ] {
        let err = error(&format!("{GRAPH}{bad}"));
        assert!(
            matches!(err, DiagramError::MalformedRule { line: 10, .. }),
            "{bad}: {err:?}"
        );
    }
    let twice = error(&format!("{GRAPH}%% no-cycles\n%% no-cycles\n"));
    assert!(matches!(
        twice,
        DiagramError::DuplicateRule {
            line: 11,
            first_line: 10,
            ..
        }
    ));
    // why: a space before the colon is read like none, so this is the second boundaries line.
    let twice = error(&format!(
        "{GRAPH}%% no-cycles: boundaries\n%% no-cycles : boundaries\n"
    ));
    assert!(
        matches!(twice, DiagramError::DuplicateRule { line: 11, .. }),
        "{twice:?}"
    );
    assert!(parse(&format!("{GRAPH}%% no-cycles\n%% no-cycles: boundaries\n")).is_ok());
}

#[test]
fn the_no_cycles_lines_survive_a_render_round_trip() {
    let src = format!("{GRAPH}  a --> c\n  %% no-cycles\n  %% no-cycles: boundaries\n");
    let rendered = render(&parse(&src).unwrap());
    assert!(
        rendered.contains("  %% no-cycles\n  %% no-cycles: boundaries\n"),
        "{rendered}"
    );
    let again = parse(&rendered).unwrap();
    assert!(again.rules.no_cycles.is_some() && again.rules.no_cycles_boundaries.is_some());
}
