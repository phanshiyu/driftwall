//! Deterministic renderer for the contract subset.

use std::collections::BTreeMap;

use crate::model::{Contract, EdgeKind, MapGlob, NodeId, NodeInfo};

const PREAMBLE: [&str; 4] = [
    "%% driftwall architecture contract.",
    "%% Solid arrow  (-->)  = implemented dependency, enforced.",
    "%% Dotted arrow (-.->) = planned dependency, tolerated until built.",
    "flowchart LR",
];

/// Renders `contract` as the Mermaid subset that [`parse`](super::parse) accepts.
///
/// Output is byte-identical for equal contracts: boundaries in id order with their nodes, then
/// unboundaried nodes, then edges sorted by `(from, to)`, then one `%% exports` line per node with
/// a surface, sorted by node with its names sorted, then the `%% map` lines in contract order. A node with an empty surface has no line, since
/// the format cannot say it.
///
/// `parse(&render(&c)) == Ok(c)` holds for every contract whose ids are valid, whose labels
/// contain no `"` or line break, and whose surfaces name only declared nodes and use identifiers
/// (see the format), with two exceptions. Boundary labels are not stored in
/// [`Contract`], so each boundary is rendered with its id as its label. And the reserved words
/// `end`, `flowchart` and `subgraph` cannot be node ids in the format, so a contract using one
/// does not round-trip. Quotes and line breaks inside a node label are replaced (by `'` and a
/// space) because the format cannot express them.
#[must_use]
pub fn render(contract: &Contract) -> String {
    let mut lines: Vec<String> = PREAMBLE.iter().map(|line| (*line).to_owned()).collect();

    let mut by_boundary: BTreeMap<&str, Vec<(&NodeId, &NodeInfo)>> = BTreeMap::new();
    let mut loose: Vec<(&NodeId, &NodeInfo)> = Vec::new();
    for (id, info) in &contract.nodes {
        match &info.boundary {
            Some(boundary) => by_boundary
                .entry(boundary.as_str())
                .or_default()
                .push((id, info)),
            None => loose.push((id, info)),
        }
    }

    for (boundary, members) in &by_boundary {
        lines.push(String::new());
        lines.push(format!("  subgraph {boundary}[\"{}\"]", sanitize(boundary)));
        lines.extend(members.iter().map(|(id, info)| node_line(id, info, "    ")));
        lines.push("  end".to_owned());
    }

    if !loose.is_empty() {
        lines.push(String::new());
        lines.extend(loose.iter().map(|(id, info)| node_line(id, info, "  ")));
    }

    if !contract.edges.is_empty() {
        lines.push(String::new());
        for (edge, kind) in &contract.edges {
            let arrow = match kind {
                EdgeKind::Implemented => "-->",
                EdgeKind::Planned => "-.->",
            };
            lines.push(format!("  {} {arrow} {}", edge.from, edge.to));
        }
    }

    let exports: Vec<String> = contract
        .surfaces
        .iter()
        .filter(|(_, names)| !names.is_empty())
        .map(|(node, names)| {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            format!("  %% exports {node}: {}", names.join(", "))
        })
        .collect();
    if !exports.is_empty() {
        lines.push(String::new());
        lines.extend(exports);
    }

    if !contract.maps.is_empty() {
        lines.push(String::new());
        for rule in &contract.maps {
            let globs: Vec<&str> = rule.globs.iter().map(MapGlob::as_str).collect();
            lines.push(format!("  %% map {}: {}", rule.target, globs.join(", ")));
        }
    }

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn node_line(id: &NodeId, info: &NodeInfo, indent: &str) -> String {
    info.label.as_ref().map_or_else(
        || format!("{indent}{id}"),
        |label| format!("{indent}{id}[\"{}\"]", sanitize(label)),
    )
}

fn sanitize(label: &str) -> String {
    label
        .chars()
        .map(|c| match c {
            '"' => '\'',
            '\n' | '\r' => ' ',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::parse;
    use crate::model::Edge;

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn node(label: Option<&str>, boundary: Option<&str>) -> NodeInfo {
        NodeInfo {
            label: label.map(str::to_owned),
            boundary: boundary.map(str::to_owned),
        }
    }

    fn contract(nodes: &[(&str, NodeInfo)], edges: &[(&str, &str, EdgeKind)]) -> Contract {
        Contract {
            nodes: nodes
                .iter()
                .map(|(name, info)| (id(name), info.clone()))
                .collect(),
            edges: edges
                .iter()
                .map(|(from, to, kind)| {
                    (
                        Edge {
                            from: id(from),
                            to: id(to),
                        },
                        *kind,
                    )
                })
                .collect(),
            ..Contract::default()
        }
    }

    fn assert_round_trips(c: &Contract) {
        let text = render(c);
        assert_eq!(parse(&text).as_ref(), Ok(c), "rendered:\n{text}");
    }

    #[test]
    fn empty_contract_round_trips() {
        let c = Contract::default();
        assert_round_trips(&c);
        assert_eq!(
            render(&c).lines().last(),
            Some("flowchart LR"),
            "an empty contract renders just the header"
        );
    }

    #[test]
    fn unlabeled_unboundaried_nodes_round_trip() {
        assert_round_trips(&contract(
            &[("a", node(None, None)), ("b", node(None, None))],
            &[],
        ));
    }

    #[test]
    fn labeled_nodes_with_awkward_labels_round_trip() {
        assert_round_trips(&contract(
            &[
                ("a", node(Some("cli: args, exit codes"), None)),
                ("b", node(Some(""), None)),
                ("c", node(Some("  padded  "), None)),
                (
                    "d",
                    node(Some("x --> y %% [z] | end | \u{e9}\u{1f980}"), None),
                ),
            ],
            &[],
        ));
    }

    #[test]
    fn boundaries_and_both_edge_kinds_round_trip() {
        assert_round_trips(&contract(
            &[
                ("cli", node(Some("cli: args"), Some("entry"))),
                ("lint", node(None, Some("engine"))),
                ("init", node(Some("init"), Some("engine"))),
                ("model", node(None, None)),
                ("loose-node_2", node(Some("L"), None)),
            ],
            &[
                ("cli", "lint", EdgeKind::Implemented),
                ("cli", "init", EdgeKind::Planned),
                ("lint", "model", EdgeKind::Planned),
                ("model", "cli", EdgeKind::Implemented),
            ],
        ));
    }

    #[test]
    fn node_ids_that_look_like_keywords_but_are_not_round_trip() {
        assert_round_trips(&contract(
            &[
                ("endpoint", node(None, Some("subgraphs"))),
                ("class", node(None, None)),
                ("style", node(None, None)),
                ("graph", node(None, None)),
            ],
            &[("endpoint", "class", EdgeKind::Planned)],
        ));
    }

    #[test]
    fn arrows_follow_edge_kind() {
        let c = contract(
            &[("a", node(None, None)), ("b", node(None, None))],
            &[
                ("a", "b", EdgeKind::Implemented),
                ("b", "a", EdgeKind::Planned),
            ],
        );
        let text = render(&c);
        assert!(text.lines().any(|l| l == "  a --> b"), "{text}");
        assert!(text.lines().any(|l| l == "  b -.-> a"), "{text}");
    }

    #[test]
    fn output_layout_is_stable() {
        let c = contract(
            &[
                ("z", node(None, None)),
                ("b", node(Some("B"), Some("g2"))),
                ("a", node(None, Some("g1"))),
                ("m", node(None, Some("g2"))),
            ],
            &[
                ("z", "a", EdgeKind::Planned),
                ("a", "b", EdgeKind::Implemented),
            ],
        );
        let expected = "\
%% driftwall architecture contract.
%% Solid arrow  (-->)  = implemented dependency, enforced.
%% Dotted arrow (-.->) = planned dependency, tolerated until built.
flowchart LR

  subgraph g1[\"g1\"]
    a
  end

  subgraph g2[\"g2\"]
    b[\"B\"]
    m
  end

  z

  a --> b
  z -.-> a
";
        assert_eq!(render(&c), expected);
    }

    #[test]
    fn render_is_deterministic_and_independent_of_insertion_order() {
        let nodes = [
            ("a", node(None, Some("g"))),
            ("b", node(Some("B"), None)),
            ("c", node(None, None)),
        ];
        let edges = [
            ("a", "b", EdgeKind::Planned),
            ("c", "a", EdgeKind::Implemented),
        ];
        let forward = contract(&nodes, &edges);
        let mut reversed_nodes = nodes.clone();
        reversed_nodes.reverse();
        let mut reversed_edges = edges;
        reversed_edges.reverse();
        let backward = contract(&reversed_nodes, &reversed_edges);
        assert_eq!(render(&forward), render(&forward));
        assert_eq!(render(&forward), render(&backward));
    }

    #[test]
    fn unrepresentable_label_characters_are_replaced_not_emitted() {
        let c = contract(&[("a", node(Some("say \"hi\"\nnow"), None))], &[]);
        let parsed = parse(&render(&c)).unwrap();
        assert_eq!(
            parsed.nodes[&id("a")].label.as_deref(),
            Some("say 'hi' now")
        );
    }

    fn with_surfaces(mut c: Contract, surfaces: &[(&str, &[&str])]) -> Contract {
        for (node, names) in surfaces {
            c.surfaces
                .insert(id(node), names.iter().map(|n| (*n).to_owned()).collect());
        }
        c
    }

    #[test]
    fn surfaces_round_trip() {
        let c = with_surfaces(
            contract(
                &[
                    ("a", node(None, None)),
                    ("b", node(Some("B"), Some("g"))),
                    ("c", node(None, None)),
                ],
                &[("a", "b", EdgeKind::Implemented)],
            ),
            &[
                ("b", &["Client", "default", "_x", "$y", "fetch_mrs"]),
                ("a", &["X"]),
            ],
        );
        assert_round_trips(&c);
        assert_round_trips(&with_surfaces(
            contract(&[("only", node(None, None))], &[]),
            &[("only", &["X"])],
        ));
    }

    #[test]
    fn exports_lines_come_after_edges_sorted_by_node_and_name() {
        let c = with_surfaces(
            contract(
                &[("a", node(None, None)), ("b", node(None, None))],
                &[("a", "b", EdgeKind::Planned)],
            ),
            &[("b", &["zeta", "Alpha", "beta"]), ("a", &["X"])],
        );
        let expected = "\
%% driftwall architecture contract.
%% Solid arrow  (-->)  = implemented dependency, enforced.
%% Dotted arrow (-.->) = planned dependency, tolerated until built.
flowchart LR

  a
  b

  a -.-> b

  %% exports a: X
  %% exports b: Alpha, beta, zeta
";
        assert_eq!(render(&c), expected);
    }

    #[test]
    fn exports_lines_follow_nodes_when_there_are_no_edges() {
        let c = with_surfaces(contract(&[("a", node(None, None))], &[]), &[("a", &["X"])]);
        assert!(
            render(&c).ends_with("  a\n\n  %% exports a: X\n"),
            "{}",
            render(&c)
        );
    }

    #[test]
    fn an_empty_surface_is_not_rendered() {
        let c = with_surfaces(contract(&[("a", node(None, None))], &[]), &[("a", &[])]);
        assert_eq!(
            render(&c),
            render(&contract(&[("a", node(None, None))], &[]))
        );
    }

    #[test]
    fn surface_order_does_not_depend_on_insertion_order() {
        let nodes = [("a", node(None, None)), ("b", node(None, None))];
        let forward = with_surfaces(contract(&nodes, &[]), &[("a", &["X", "Y"]), ("b", &["Z"])]);
        let backward = with_surfaces(contract(&nodes, &[]), &[("b", &["Z"]), ("a", &["Y", "X"])]);
        assert_eq!(render(&forward), render(&backward));
    }

    #[test]
    fn boundary_labels_are_not_preserved() {
        let src = "flowchart LR\nsubgraph g[\"Pretty Name\"]\na\nend\n";
        let first = parse(src).unwrap();
        let again = parse(&render(&first)).unwrap();
        assert_eq!(first, again);
        assert!(render(&first).contains("subgraph g[\"g\"]"));
    }
}
