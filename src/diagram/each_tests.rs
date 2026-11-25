//! `%% each` edge templates: parsing, expansion, the contract errors and the round trip of "Edge
//! templates".

use super::*;
use crate::contract::{EdgeKind, Endpoint, NodeId};

/// `billing` has a controller, a service and a view; `search` has no view; `shell` is written
/// directly in `features` and is not a child boundary.
const FIXTURE: &str = "flowchart LR
  subgraph features
    subgraph billing
      billing-controller
      billing-service
      billing-view
    end
    subgraph search
      search-controller
      subgraph search-core
        search-service
      end
    end
    features-shell
  end
";

fn with(extra: &str) -> String {
    format!("{FIXTURE}{extra}")
}

fn error(extra: &str) -> DiagramError {
    parse(&with(extra)).unwrap_err()
}

fn pairs(extra: &str) -> Vec<(String, String, EdgeKind, u32)> {
    let contract = parse(&with(extra)).unwrap();
    assert!(
        contract.edges.is_empty(),
        "an expansion is not a node edge: {:?}",
        contract.edges
    );
    contract
        .boundary_edges
        .iter()
        .map(|e| (e.from.to_string(), e.to.to_string(), e.kind, e.line))
        .collect()
}

fn reason(error: &DiagramError) -> String {
    match error {
        DiagramError::MalformedRule { rule, reason, .. } => {
            assert_eq!(rule, "each");
            reason.clone()
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_arrow_expands_for_each_child_that_has_both_roles_and_a_missing_part_is_skipped() {
    let found = pairs("  %% each features: controller --> service, controller -.-> view\n");
    let controller_service = EdgeKind::Implemented;
    assert_eq!(
        found,
        [
            (
                "billing-controller".to_owned(),
                "billing-service".to_owned(),
                controller_service,
                16
            ),
            (
                "search-controller".to_owned(),
                "search-service".to_owned(),
                controller_service,
                16
            ),
            (
                "billing-controller".to_owned(),
                "billing-view".to_owned(),
                EdgeKind::Planned,
                16
            ),
        ]
    );
}

#[test]
fn a_role_resolves_to_a_node_nested_deeper_in_the_child_but_not_to_one_outside_it() {
    // why: `search-service` is in `search-core`, inside `search`, so it is the role `service`.
    assert_eq!(
        pairs("  %% each features: controller --> service\n").len(),
        2
    );
    let outside = "flowchart LR\n  subgraph f\n    subgraph a\n      a-x\n    end\n    subgraph b\n      b-x\n    end\n  end\n  a-y\n  %% each f: x --> y\n";
    let message = reason(&parse(outside).unwrap_err());
    assert!(
        message.contains("role `y` resolves in no child of `f`"),
        "{message}"
    );
}

#[test]
fn every_arrow_form_of_an_edge_is_an_arrow_of_a_template() {
    let found = pairs(
        "  %% each features: controller --> service, controller -->|test| view, service -.->|type, test| controller\n",
    );
    assert!(found.iter().any(|f| f.2 == EdgeKind::Implemented));
    assert!(found.iter().any(|f| f.2 == EdgeKind::Test));
    assert!(found.iter().any(|f| f.2 == EdgeKind::PlannedTypeTest));
}

#[test]
fn a_comma_in_a_label_does_not_split_the_arrow() {
    let found =
        pairs("  %% each features: controller -->|type, test| service, service --> controller\n");
    assert_eq!(found.len(), 4);
    assert!(found.iter().any(|f| f.2 == EdgeKind::TypeTest));
}

#[test]
fn the_boundary_must_be_a_declared_boundary_with_a_child_boundary() {
    for (line, expect) in [
        (
            "  %% each nowhere: controller --> service\n",
            "`nowhere` is not a declared boundary",
        ),
        (
            "  %% each billing-service: controller --> service\n",
            "is a node, not a boundary",
        ),
        (
            "  %% each billing: controller --> service\n",
            "`billing` has no child boundary",
        ),
    ] {
        let message = reason(&error(line));
        assert!(message.contains(expect), "{line}: {message}");
    }
}

#[test]
fn a_role_that_resolves_in_no_child_is_a_typo_and_names_the_line() {
    let err = error("  %% each features: controller --> servce\n");
    assert_eq!(err.line(), Some(16));
    let message = reason(&err);
    assert!(
        message.contains("role `servce` resolves in no child of `features`"),
        "{message}"
    );
    assert!(message.contains("billing, search"), "{message}");
}

#[test]
fn a_malformed_list_is_an_error_with_the_line() {
    for line in [
        "  %% each features:\n",
        "  %% each features controller --> service\n",
        "  %% each features: controller\n",
        "  %% each features: controller --> service,\n",
        "  %% each features: controller -- test --> service\n",
        "  %% each features: controller --> service --> view\n",
        "  %% each features: controller --> controller\n",
        "  %% each features: controller --> service, controller -.-> service\n",
        "  %% each features: controller -->|type, type| service\n",
        "  %% each features: 9 --> service\n",
        "  %% each is a plain word\n",
    ] {
        let err = error(line);
        assert_eq!(err.line(), Some(16), "{line}: {err}");
    }
}

#[test]
fn two_templates_that_expand_to_one_edge_are_a_duplicate_edge_naming_both_lines() {
    let err = error(
        "  %% each features: controller --> service\n  %% each features: controller -.-> service\n",
    );
    assert!(
        matches!(
            err,
            DiagramError::DuplicateEdge {
                line: 17,
                first_line: 16,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn only_a_comment_whose_first_word_is_each_is_a_template() {
    assert!(parse(&with("  %% eachother is a plain comment\n")).is_ok());
    assert!(parse(&with("  %%each: not a template\n")).is_ok());
}

#[test]
fn an_expansion_conflicts_with_no_node_edge_and_the_node_edge_stays() {
    let contract = parse(&with(
        "  billing-controller --> billing-service\n  %% each features: controller --> service\n",
    ))
    .unwrap();
    assert_eq!(contract.edges.len(), 1);
    assert_eq!(contract.boundary_edges.len(), 2);
}

#[test]
fn templates_render_back_and_expansions_do_not() {
    let contract = parse(&with(
        "  %% each features: controller -->|type, test| service, controller -.-> view\n",
    ))
    .unwrap();
    let rendered = render(&contract);
    assert!(
        rendered.contains(
            "  %% each features: controller -->|type, test| service, controller -.-> view\n"
        ),
        "{rendered}"
    );
    assert!(!rendered.contains("billing-controller -->"), "{rendered}");
    let again = parse(&rendered).unwrap();
    assert_eq!(again.nodes, contract.nodes);
    assert_eq!(again.boundary_edges.len(), contract.boundary_edges.len());
    assert_eq!(again.rules.each.len(), 1);
    assert_eq!(again.rules.each[0].arrows, contract.rules.each[0].arrows);
    assert_eq!(render(&again), rendered);
}

fn ends(contract: &crate::contract::Contract) -> Vec<(String, String, EdgeKind)> {
    contract
        .boundary_edges
        .iter()
        .map(|e| (e.from.to_string(), e.to.to_string(), e.kind))
        .collect()
}

#[test]
fn a_fixed_end_is_kept_for_every_child_that_has_the_role() {
    let contract = parse(&with(
        "  ui\n  %% each features: view --> ui, service -->|type| ui, ui -.-> controller\n",
    ))
    .unwrap();
    assert_eq!(
        ends(&contract),
        [
            ("billing-view".into(), "ui".into(), EdgeKind::Implemented),
            ("billing-service".into(), "ui".into(), EdgeKind::Type),
            ("search-service".into(), "ui".into(), EdgeKind::Type),
            ("ui".into(), "billing-controller".into(), EdgeKind::Planned),
            ("ui".into(), "search-controller".into(), EdgeKind::Planned),
        ]
    );
    assert!(contract.boundary_edges.iter().all(|e| e.line == 17));
}

#[test]
fn a_fixed_boundary_end_expands_to_a_boundary_edge() {
    let contract = parse(&with(
        "  subgraph shared\n    shared-ui\n  end\n  %% each features: view --> shared\n",
    ))
    .unwrap();
    let [edge] = &contract.boundary_edges[..] else {
        panic!("{:?}", contract.boundary_edges);
    };
    assert_eq!(edge.from.to_string(), "billing-view");
    assert_eq!(
        edge.to,
        crate::contract::Endpoint::Boundary("shared".into())
    );
}

#[test]
fn a_node_written_directly_in_the_boundary_is_a_fixed_end() {
    let contract = parse(&with("  %% each features: features-shell --> controller\n")).unwrap();
    assert_eq!(contract.boundary_edges.len(), 2);
}

#[test]
fn two_fixed_ends_are_an_error_that_says_to_write_an_ordinary_edge() {
    let err = error("  ui\n  core\n  %% each features: ui --> core\n");
    assert_eq!(err.line(), Some(18));
    let message = reason(&err);
    assert!(
        message.contains("write it as an ordinary edge"),
        "{message}"
    );
}

#[test]
fn a_fixed_end_cannot_be_the_boundary_or_lie_inside_a_child() {
    for (extra, expect) in [
        (
            "  %% each features: features --> controller\n",
            "is the boundary of the template",
        ),
        (
            "  %% each features: billing-service --> controller\n",
            "is inside `billing`",
        ),
        (
            "  %% each features: search-service --> controller\n",
            "is inside `search`",
        ),
        (
            "  %% each features: search-core --> controller\n",
            "is inside `search`",
        ),
    ] {
        let err = error(extra);
        assert_eq!(err.line(), Some(16), "{extra}");
        let message = reason(&err);
        assert!(message.contains(expect), "{extra}: {message}");
    }
}

#[test]
fn a_fixed_boundary_with_no_node_is_an_error() {
    let message = reason(&error(
        "  subgraph hollow\n  end\n  %% each features: view --> hollow\n",
    ));
    assert!(message.contains("declares no node"), "{message}");
}

#[test]
fn a_name_that_is_a_role_of_some_child_stays_a_role_even_when_declared() {
    // why: 0.2.0 contract: `store` is declared and `billing-store` is a role, which must still parse.
    let source = "flowchart LR
  store
  subgraph features
    subgraph billing
      billing-service
      billing-store
    end
    subgraph search
      search-service
    end
  end
  %% each features: service --> store
";
    let contract = parse(source).unwrap();
    let ends: Vec<_> = contract
        .boundary_edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();
    assert_eq!(
        ends,
        vec![(
            Endpoint::Node(NodeId::new("billing-service").unwrap()),
            Endpoint::Node(NodeId::new("billing-store").unwrap())
        )]
    );
}

#[test]
fn an_arrow_with_a_fixed_end_renders_back_as_the_template() {
    let contract = parse(&with(
        "  ui\n  %% each features: view --> ui, ui -->|type| controller\n",
    ))
    .unwrap();
    let rendered = render(&contract);
    assert!(
        rendered.contains("  %% each features: view --> ui, ui -->|type| controller\n"),
        "{rendered}"
    );
    assert!(!rendered.contains("billing-view -->"), "{rendered}");
    let again = parse(&rendered).unwrap();
    assert_eq!(ends(&again), ends(&contract));
    assert_eq!(render(&again), rendered);
}
