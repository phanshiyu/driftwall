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
