//! The rule directives of "Contract rules": what parses, and every contract error.

use super::*;
use crate::contract::NodeId;

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
    let order = contract.rules.order[0].clone();
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
fn several_order_lines_are_independent_chains() {
    let contract = parse(&with(
        "  %% order: entry > engine\n  %% order: engine > shared\n  %% order: entry > shared\n",
    ))
    .unwrap();
    let lines: Vec<u32> = contract.rules.order.iter().map(|o| o.line).collect();
    assert_eq!(lines, [12, 13, 14]);
    let rendered = render(&contract);
    assert_eq!(rendered.matches("  %% order: ").count(), 3, "{rendered}");
    let again: Vec<Vec<String>> = parse(&rendered)
        .unwrap()
        .rules
        .order
        .into_iter()
        .map(|o| o.boundaries)
        .collect();
    let first: Vec<Vec<String>> = contract
        .rules
        .order
        .into_iter()
        .map(|o| o.boundaries)
        .collect();
    assert_eq!(again, first);
}

#[test]
fn disjoint_chains_and_a_single_shared_boundary_agree() {
    let src = "flowchart LR
  subgraph a[\"A\"]
    n1
  end
  subgraph b[\"B\"]
    n2
  end
  subgraph c[\"C\"]
    n3
  end
  subgraph d[\"D\"]
    n4
  end
  %% order: a > b
  %% order: c > d
  %% order: b > c
";
    assert_eq!(parse(src).unwrap().rules.order.len(), 3);
}

#[test]
fn chains_that_contradict_are_a_contract_error_naming_both_lines() {
    let err = error(&with(
        "%% order: entry > engine > shared\n%% order: shared > entry\n",
    ));
    assert_eq!(err.line(), Some(13), "{err}");
    let text = err.to_string();
    assert!(text.contains("line 12"), "{text}");
    assert!(text.contains("`shared` is below `entry` there"), "{text}");
}

#[test]
fn an_upward_arrow_is_checked_against_every_chain() {
    let err = error(&with(
        "%% order: entry > engine\n%% order: engine > shared\n  model --> lint\n",
    ));
    assert!(matches!(err, DiagramError::UpwardArrow { .. }), "{err}");
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
        again.rules.order[0].boundaries,
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

#[test]
fn a_budget_line_names_a_node_and_any_subset_of_the_three_measures() {
    let src = format!(
        "{GRAPH}  %% budget a: fan-out 3, lines 2000\n  %% budget c: files 0\n  %%   budget d: lines 5, files 6, fan-out 7\n  %% budget b: interface 4, files 1\n"
    );
    let budgets = parse(&src).unwrap().rules.budgets;
    let a = &budgets[&NodeId::new("a").unwrap()];
    assert_eq!(
        (a.line, a.fan_out, a.files, a.lines),
        (10, Some(3), None, Some(2000))
    );
    let c = &budgets[&NodeId::new("c").unwrap()];
    assert_eq!((c.fan_out, c.files, c.lines), (None, Some(0), None));
    let d = &budgets[&NodeId::new("d").unwrap()];
    assert_eq!((d.fan_out, d.files, d.lines), (Some(7), Some(6), Some(5)));
    let b = &budgets[&NodeId::new("b").unwrap()];
    assert_eq!(
        (b.interface, b.files, a.interface),
        (Some(4), Some(1), None)
    );
}

#[test]
fn a_malformed_budget_is_a_contract_error_at_its_line() {
    for bad in [
        "%% budget a\n",
        "%% budget a:\n",
        "%% budget a: fan-out\n",
        "%% budget a: fan-out x\n",
        "%% budget a: fan-out -1\n",
        "%% budget a: fan-out 1.5\n",
        "%% budget a: speed 3\n",
        "%% budget a: files 3 4\n",
        "%% budget a: files 3,\n",
        "%% budget a: files 3, files 4\n",
        "%% budget a: interface 3, interface 4\n",
        "%% budget a: interface x\n",
        "%% budget a: interface -2\n",
        "%% budget a: interface\n",
        "%% budget ghost: files 3\n",
        "%% budget 9a: files 3\n",
    ] {
        let err = error(&format!("{GRAPH}{bad}"));
        assert!(
            matches!(
                err,
                DiagramError::MalformedRule { .. } | DiagramError::InvalidId { .. }
            ),
            "{bad}: {err:?}"
        );
        assert_eq!(err.line(), Some(10), "{bad}");
    }
}

#[test]
fn a_second_budget_for_one_node_is_an_error_but_two_nodes_are_fine() {
    let err = error(&format!(
        "{GRAPH}%% budget a: files 3\n%% budget a: lines 9\n"
    ));
    assert!(
        matches!(&err, DiagramError::DuplicateRule { line: 11, first_line: 10, rule } if rule == "budget a"),
        "{err:?}"
    );
    assert!(
        parse(&format!(
            "{GRAPH}%% budget a: files 3\n%% budget b: files 3\n"
        ))
        .is_ok()
    );
}

#[test]
fn a_comment_that_only_starts_with_budget_stays_plain() {
    assert!(
        parse(&format!(
            "{GRAPH}%% budgets are per node\n%% budgeting\n%% budget: files 3\n"
        ))
        .unwrap()
        .rules
        .is_empty()
    );
}

#[test]
fn budgets_survive_a_render_round_trip() {
    let src = format!(
        "{GRAPH}  %% budget b: files 4\n  %% budget a: fan-out 3, lines 2000, interface 5\n"
    );
    let rendered = render(&parse(&src).unwrap());
    assert!(
        rendered.contains(
            "  %% budget a: fan-out 3, lines 2000, interface 5\n  %% budget b: files 4\n"
        ),
        "{rendered}"
    );
    let again = parse(&rendered).unwrap().rules.budgets;
    assert_eq!(again.len(), 2);
    assert_eq!(again[&NodeId::new("a").unwrap()].fan_out, Some(3));
    assert_eq!(again[&NodeId::new("a").unwrap()].interface, Some(5));
}

#[test]
fn a_tests_line_takes_only_isolated_and_only_once() {
    let rules = parse(&format!("{GRAPH}  %% tests: isolated\n"))
        .unwrap()
        .rules;
    assert_eq!(rules.tests_isolated, Some(10));
    for bad in [
        "%% tests: all\n",
        "%% tests:\n",
        "%% tests: isolated please\n",
    ] {
        let err = error(&format!("{GRAPH}{bad}"));
        assert!(
            matches!(err, DiagramError::MalformedRule { line: 10, .. }),
            "{bad}: {err:?}"
        );
    }
    let twice = error(&format!("{GRAPH}%% tests: isolated\n%% tests: isolated\n"));
    assert!(matches!(
        twice,
        DiagramError::DuplicateRule {
            line: 11,
            first_line: 10,
            ..
        }
    ));
}

#[test]
fn comments_that_only_start_with_tests_stay_plain() {
    let src = format!("{GRAPH}%% tests -->|test| a: unit tests\n%% tests are next to the code\n");
    assert!(parse(&src).unwrap().rules.is_empty());
}

#[test]
fn the_tests_line_survives_a_render_round_trip() {
    let rendered = render(&parse(&format!("{GRAPH}%% tests: isolated\n")).unwrap());
    assert!(rendered.contains("  %% tests: isolated\n"), "{rendered}");
    assert!(parse(&rendered).unwrap().rules.tests_isolated.is_some());
}
