//! Line-oriented parser for the contract subset.

use std::collections::BTreeMap;

use super::error::DiagramError;
use crate::model::{Contract, Edge, EdgeKind, NodeId, NodeInfo};

const DIRECTIONS: [&str; 4] = ["LR", "RL", "TB", "BT"];
const RESERVED: [&str; 3] = ["end", "flowchart", "subgraph"];
const SOLID: &str = "-->";
const DOTTED: &str = "-.->";

/// Parses the contract subset of Mermaid `flowchart` into a [`Contract`].
///
/// The direction (`LR`, `TB`, ...) and boundary labels are validated but not stored, and edge
/// labels (`-->|text|`) are accepted and discarded. Declaration order does not matter for edge
/// endpoints, which are checked once the whole file has been read.
///
/// # Errors
/// Returns a [`DiagramError`] for the first problem found: anything outside the grammar in
/// `docs/DESIGN.md`, a missing or repeated header, invalid or duplicate ids, undeclared edge
/// endpoints, duplicate or self edges, and nested, unclosed or stray subgraph blocks.
pub fn parse(src: &str) -> Result<Contract, DiagramError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut parser = Parser::default();
    let mut line: u32 = 0;
    for raw in src.lines() {
        line = line.saturating_add(1);
        let text = raw.trim();
        if text.is_empty() || text.starts_with("%%") {
            continue;
        }
        parser.statement(text, line)?;
    }
    parser.finish()
}

struct PendingEdge {
    line: u32,
    from: NodeId,
    to: NodeId,
    kind: EdgeKind,
}

#[derive(Default)]
struct Parser {
    header_seen: bool,
    nodes: BTreeMap<NodeId, NodeInfo>,
    node_lines: BTreeMap<NodeId, u32>,
    boundaries: BTreeMap<String, u32>,
    open: Option<(String, u32)>,
    edges: Vec<PendingEdge>,
}

impl Parser {
    fn statement(&mut self, text: &str, line: u32) -> Result<(), DiagramError> {
        if !self.header_seen {
            check_header(text, line)?;
            self.header_seen = true;
            return Ok(());
        }
        if text == "end" {
            return self
                .open
                .take()
                .map(|_| ())
                .ok_or(DiagramError::StrayEnd { line });
        }
        if let Some(rest) = text.strip_prefix("subgraph")
            && rest.starts_with(char::is_whitespace)
        {
            return self.open_subgraph(rest.trim_start(), text, line);
        }
        if text.split_whitespace().next() == Some("flowchart") {
            return Err(DiagramError::MisplacedHeader { line });
        }
        self.node_or_edge(text, line)
    }

    fn open_subgraph(&mut self, rest: &str, text: &str, line: u32) -> Result<(), DiagramError> {
        let (raw_id, tail) = split_ident(rest);
        let id = valid_id(raw_id, line)?;
        if !tail.is_empty() && !matches!(parse_label(tail), Some((_, ""))) {
            return Err(unsupported(text, line));
        }
        if let Some((open, open_line)) = &self.open {
            return Err(DiagramError::NestedSubgraph {
                line,
                open: open.clone(),
                open_line: *open_line,
            });
        }
        if let Some(first_line) = self.boundaries.get(id.as_str()) {
            return Err(DiagramError::DuplicateBoundary {
                line,
                id: id.to_string(),
                first_line: *first_line,
            });
        }
        self.boundaries.insert(id.to_string(), line);
        self.open = Some((id.to_string(), line));
        Ok(())
    }

    fn node_or_edge(&mut self, text: &str, line: u32) -> Result<(), DiagramError> {
        let (raw_id, rest) = split_ident(text);
        if raw_id.is_empty() {
            return Err(unsupported(text, line));
        }
        if rest.starts_with('[') {
            return match parse_label(rest) {
                Some((label, "")) => self.declare(raw_id, Some(label), line),
                _ => Err(unsupported(text, line)),
            };
        }
        let rest = rest.trim_start();
        if rest.is_empty() {
            return self.declare(raw_id, None, line);
        }
        let (kind, after_arrow) = if let Some(after) = rest.strip_prefix(DOTTED) {
            (EdgeKind::Planned, after)
        } else if let Some(after) = rest.strip_prefix(SOLID) {
            (EdgeKind::Implemented, after)
        } else {
            return Err(unsupported(text, line));
        };
        let mut after_arrow = after_arrow.trim_start();
        if let Some(labelled) = after_arrow.strip_prefix('|') {
            let (_, after_label) = labelled
                .split_once('|')
                .ok_or_else(|| unsupported(text, line))?;
            after_arrow = after_label.trim_start();
        }
        let (raw_to, tail) = split_ident(after_arrow);
        if raw_to.is_empty() || !tail.is_empty() {
            return Err(unsupported(text, line));
        }
        let from = valid_id(raw_id, line)?;
        let to = valid_id(raw_to, line)?;
        self.edges.push(PendingEdge {
            line,
            from,
            to,
            kind,
        });
        Ok(())
    }

    fn declare(
        &mut self,
        raw_id: &str,
        label: Option<&str>,
        line: u32,
    ) -> Result<(), DiagramError> {
        let id = valid_id(raw_id, line)?;
        if RESERVED.contains(&id.as_str()) {
            return Err(DiagramError::ReservedWord {
                line,
                word: raw_id.to_owned(),
            });
        }
        if let Some(first_line) = self.node_lines.get(&id) {
            return Err(DiagramError::DuplicateNode {
                line,
                id,
                first_line: *first_line,
            });
        }
        self.node_lines.insert(id.clone(), line);
        self.nodes.insert(
            id,
            NodeInfo {
                label: label.map(str::to_owned),
                boundary: self.open.as_ref().map(|(boundary, _)| boundary.clone()),
            },
        );
        Ok(())
    }

    fn finish(self) -> Result<Contract, DiagramError> {
        if !self.header_seen {
            return Err(DiagramError::MissingHeader);
        }
        if let Some((id, line)) = self.open {
            return Err(DiagramError::UnclosedSubgraph { line, id });
        }
        let mut edges = BTreeMap::new();
        let mut edge_lines: BTreeMap<Edge, u32> = BTreeMap::new();
        for pending in self.edges {
            if pending.from == pending.to {
                return Err(DiagramError::SelfEdge {
                    line: pending.line,
                    id: pending.from,
                });
            }
            for endpoint in [&pending.from, &pending.to] {
                if !self.nodes.contains_key(endpoint) {
                    return Err(DiagramError::UndeclaredNode {
                        line: pending.line,
                        id: endpoint.clone(),
                    });
                }
            }
            let edge = Edge {
                from: pending.from,
                to: pending.to,
            };
            if let Some(first_line) = edge_lines.get(&edge) {
                return Err(DiagramError::DuplicateEdge {
                    line: pending.line,
                    from: edge.from,
                    to: edge.to,
                    first_line: *first_line,
                });
            }
            edge_lines.insert(edge.clone(), pending.line);
            edges.insert(edge, pending.kind);
        }
        Ok(Contract {
            nodes: self.nodes,
            edges,
        })
    }
}

fn check_header(text: &str, line: u32) -> Result<(), DiagramError> {
    let mut words = text.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("flowchart"), Some(direction), None) if DIRECTIONS.contains(&direction) => Ok(()),
        _ => Err(DiagramError::BadHeader {
            line,
            found: text.to_owned(),
        }),
    }
}

fn unsupported(text: &str, line: u32) -> DiagramError {
    DiagramError::Unsupported {
        line,
        statement: text.to_owned(),
    }
}

fn valid_id(raw: &str, line: u32) -> Result<NodeId, DiagramError> {
    NodeId::new(raw).map_err(|source| DiagramError::InvalidId { line, source })
}

/// Splits a leading identifier token off `s`. The token ends at whitespace, `[`, `|` or the start
/// of an arrow, so `a-->b` lexes as `a`, `-->`, `b` even though `-` is a legal id character.
fn split_ident(s: &str) -> (&str, &str) {
    let end = s
        .char_indices()
        .find(|&(i, c)| {
            c.is_whitespace()
                || c == '['
                || c == '|'
                || s.get(i..)
                    .is_some_and(|tail| tail.starts_with(SOLID) || tail.starts_with(DOTTED))
        })
        .map_or(s.len(), |(i, _)| i);
    s.split_at_checked(end).unwrap_or((s, ""))
}

/// Parses `["label"]` at the start of `s`, returning the label and whatever follows the `]`.
fn parse_label(s: &str) -> Option<(&str, &str)> {
    let inner = s.strip_prefix("[\"")?;
    let (label, tail) = inner.split_once('"')?;
    Some((label, tail.strip_prefix(']')?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn err_line(src: &str) -> (DiagramError, Option<u32>) {
        let err = parse(src).unwrap_err();
        let line = err.line();
        (err, line)
    }

    #[test]
    fn parses_nodes_boundaries_and_both_edge_kinds() {
        let src = "flowchart LR\n  subgraph shared[\"Shared\"]\n    model[\"model: ids\"]\n  end\n  cli\n  cli --> model\n  model -.-> cli\n";
        let contract = parse(src).unwrap();
        assert_eq!(
            contract.nodes[&id("model")],
            NodeInfo {
                label: Some("model: ids".to_owned()),
                boundary: Some("shared".to_owned()),
            }
        );
        assert_eq!(
            contract.nodes[&id("cli")],
            NodeInfo {
                label: None,
                boundary: None,
            }
        );
        assert_eq!(
            contract.edges[&Edge {
                from: id("cli"),
                to: id("model")
            }],
            EdgeKind::Implemented
        );
        assert_eq!(
            contract.edges[&Edge {
                from: id("model"),
                to: id("cli")
            }],
            EdgeKind::Planned
        );
    }

    #[test]
    fn accepts_every_direction_and_header_only_file() {
        for dir in DIRECTIONS {
            assert_eq!(parse(&format!("flowchart {dir}")), Ok(Contract::default()));
        }
    }

    #[test]
    fn tolerates_comments_blank_lines_indentation_crlf_and_bom() {
        let src = "\u{feff}%% top\r\n\r\n   flowchart   TB  \r\n\t%% indented comment\r\n\ta\r\n   b[\"B\"]   \r\n\r\n    a   -.->   b\r\n";
        let contract = parse(src).unwrap();
        assert_eq!(contract.nodes.len(), 2);
        assert_eq!(contract.edges.len(), 1);
    }

    #[test]
    fn edge_label_is_accepted_and_discarded() {
        let plain = parse("flowchart LR\na\nb\na --> b\n").unwrap();
        for variant in [
            "a -->|uses| b",
            "a -->| two words | b",
            "a-->|x|b",
            "a --> |x| b",
        ] {
            let src = format!("flowchart LR\na\nb\n{variant}\n");
            assert_eq!(parse(&src).unwrap(), plain, "{variant}");
        }
        let dotted = parse("flowchart LR\na\nb\na -.->|x| b\n").unwrap();
        assert_eq!(
            dotted.edges.values().copied().collect::<Vec<_>>(),
            [EdgeKind::Planned]
        );
    }

    #[test]
    fn arrows_need_no_surrounding_spaces() {
        let contract = parse("flowchart LR\na\nb\na-->b\n").unwrap();
        assert_eq!(contract.edges.len(), 1);
    }

    #[test]
    fn edges_may_precede_node_declarations() {
        let contract = parse("flowchart LR\na --> b\na\nb\n").unwrap();
        assert_eq!(contract.edges.len(), 1);
    }

    #[test]
    fn labels_may_contain_arbitrary_characters_except_quote() {
        let contract = parse("flowchart LR\na[\"x --> y %% [z] | end\"]\n").unwrap();
        assert_eq!(
            contract.nodes[&id("a")].label.as_deref(),
            Some("x --> y %% [z] | end")
        );
        let empty = parse("flowchart LR\na[\"\"]\n").unwrap();
        assert_eq!(empty.nodes[&id("a")].label.as_deref(), Some(""));
    }

    #[test]
    fn subgraph_without_label_is_accepted() {
        let contract = parse("flowchart LR\nsubgraph g\nn\nend\n").unwrap();
        assert_eq!(contract.nodes[&id("n")].boundary.as_deref(), Some("g"));
    }

    #[test]
    fn edges_inside_a_subgraph_do_not_change_boundaries() {
        let contract = parse("flowchart LR\na\nsubgraph g\nb\na --> b\nend\n").unwrap();
        assert_eq!(contract.nodes[&id("a")].boundary, None);
        assert_eq!(contract.edges.len(), 1);
    }

    #[test]
    fn empty_and_comment_only_files_have_no_header() {
        for src in ["", "\n\n", "%% only a comment\n"] {
            assert_eq!(parse(src), Err(DiagramError::MissingHeader));
            assert_eq!(parse(src).unwrap_err().line(), None);
        }
    }

    #[test]
    fn header_must_be_the_first_statement() {
        let (err, line) = err_line("%% c\n\na\nflowchart LR\n");
        assert!(
            matches!(err, DiagramError::BadHeader { line: 3, .. }),
            "{err:?}"
        );
        assert_eq!(line, Some(3));
    }

    #[test]
    fn graph_header_is_rejected() {
        let (err, line) = err_line("graph LR\na\n");
        assert!(matches!(err, DiagramError::BadHeader { .. }), "{err:?}");
        assert_eq!(line, Some(1));
    }

    #[test]
    fn header_needs_a_known_direction() {
        for src in [
            "flowchart",
            "flowchart TD",
            "flowchart lr",
            "flowchart LR LR",
            "flowchart LR %% c",
        ] {
            let (err, line) = err_line(&format!("\n{src}\n"));
            assert!(
                matches!(err, DiagramError::BadHeader { .. }),
                "{src}: {err:?}"
            );
            assert_eq!(line, Some(2), "{src}");
        }
    }

    #[test]
    fn repeated_header_is_misplaced() {
        let (err, line) = err_line("flowchart LR\na\nflowchart TB\n");
        assert_eq!(err, DiagramError::MisplacedHeader { line: 3 });
        assert_eq!(line, Some(3));
    }

    #[test]
    fn unsupported_statements_are_hard_errors_with_line_numbers() {
        let cases = [
            "classDef red fill:#f00",
            "class a red",
            "style a fill:#f00",
            "click a \"https://example.com\"",
            "linkStyle 0 stroke:#f00",
            "direction TB",
            "a --- b",
            "a ==> b",
            "a --x b",
            "a <--> b",
            "a -- text --> b",
            "a --> b --> c",
            "a -.-> b -.-> c",
            "a --> b & c",
            "a & b --> c",
            "a[label with spaces]",
            "a[\"unterminated]",
            "a[\"x\"] --> b",
            "a[\"x\"] trailing",
            "a [\"x\"]",
            "a --> |unterminated b",
            "a -->",
            "--> b",
            "[\"x\"]",
            "a --> b[\"x\"]",
            "a --> b %% trailing comment",
            "end now",
            "subgraph",
        ];
        for case in cases {
            let src = format!("flowchart LR\na\nb\nc\n{case}\n");
            let (err, line) = err_line(&src);
            assert!(
                matches!(
                    err,
                    DiagramError::Unsupported { line: 5, .. }
                        | DiagramError::ReservedWord { line: 5, .. }
                ),
                "{case}: {err:?}"
            );
            assert_eq!(line, Some(5), "{case}");
        }
    }

    #[test]
    fn invalid_node_ids_are_rejected() {
        for case in ["1abc", "-x", "a.b", "a:b", "_a", "a&b", "é", "9[\"l\"]"] {
            let (err, line) = err_line(&format!("flowchart LR\n\n{case}\n"));
            assert!(
                matches!(err, DiagramError::InvalidId { line: 3, .. }),
                "{case}: {err:?}"
            );
            assert_eq!(line, Some(3), "{case}");
        }
    }

    #[test]
    fn invalid_ids_in_edges_and_subgraphs_are_rejected() {
        for case in [
            "a --> 1b",
            "1a --> b",
            "a --> b.c",
            "subgraph 1g",
            "subgraph g.h[\"x\"]",
        ] {
            let (err, line) = err_line(&format!("flowchart LR\na\nb\n{case}\n"));
            assert!(
                matches!(err, DiagramError::InvalidId { line: 4, .. }),
                "{case}: {err:?}"
            );
            assert_eq!(line, Some(4), "{case}");
        }
    }

    #[test]
    fn reserved_words_cannot_be_node_ids() {
        for word in [
            "end[\"x\"]",
            "flowchart[\"x\"]",
            "subgraph[\"x\"]",
            "subgraph",
        ] {
            let (err, line) = err_line(&format!("flowchart LR\n{word}\n"));
            assert!(
                matches!(err, DiagramError::ReservedWord { line: 2, .. }),
                "{word}: {err:?}"
            );
            assert_eq!(line, Some(2));
        }
    }

    #[test]
    fn ids_that_merely_start_with_a_keyword_are_fine() {
        let contract = parse("flowchart LR\nendpoint\nsubgraphs\nflowcharts\n").unwrap();
        assert_eq!(contract.nodes.len(), 3);
    }

    #[test]
    fn undeclared_endpoint_is_reported_on_the_edge_line() {
        let (err, line) = err_line("flowchart LR\na\n\na --> ghost\n");
        assert_eq!(
            err,
            DiagramError::UndeclaredNode {
                line: 4,
                id: id("ghost")
            }
        );
        assert_eq!(line, Some(4));
        let (err, _) = err_line("flowchart LR\nb\nghost --> b\n");
        assert_eq!(
            err,
            DiagramError::UndeclaredNode {
                line: 3,
                id: id("ghost")
            }
        );
    }

    #[test]
    fn boundary_ids_are_not_nodes() {
        let (err, line) = err_line("flowchart LR\na\nsubgraph g\nb\nend\na --> g\n");
        assert_eq!(
            err,
            DiagramError::UndeclaredNode {
                line: 6,
                id: id("g")
            }
        );
        assert_eq!(line, Some(6));
    }

    #[test]
    fn duplicate_node_is_rejected_whatever_the_form() {
        for second in ["a", "a[\"again\"]"] {
            let (err, line) = err_line(&format!("flowchart LR\na[\"first\"]\n\n{second}\n"));
            assert_eq!(
                err,
                DiagramError::DuplicateNode {
                    line: 4,
                    id: id("a"),
                    first_line: 2
                }
            );
            assert_eq!(line, Some(4));
        }
    }

    #[test]
    fn duplicate_node_across_boundaries_is_rejected() {
        let (err, line) = err_line("flowchart LR\nsubgraph g\na\nend\nsubgraph h\na\nend\n");
        assert_eq!(
            err,
            DiagramError::DuplicateNode {
                line: 6,
                id: id("a"),
                first_line: 3
            }
        );
        assert_eq!(line, Some(6));
    }

    #[test]
    fn duplicate_edge_is_rejected_even_with_a_different_kind() {
        for second in ["a --> b", "a -.-> b", "a -->|x| b"] {
            for first in ["a --> b", "a -.-> b"] {
                let (err, line) = err_line(&format!("flowchart LR\na\nb\n{first}\n{second}\n"));
                assert_eq!(
                    err,
                    DiagramError::DuplicateEdge {
                        line: 5,
                        from: id("a"),
                        to: id("b"),
                        first_line: 4
                    }
                );
                assert_eq!(line, Some(5));
            }
        }
    }

    #[test]
    fn opposite_direction_edges_are_not_duplicates() {
        let contract = parse("flowchart LR\na\nb\na --> b\nb --> a\n").unwrap();
        assert_eq!(contract.edges.len(), 2);
    }

    #[test]
    fn self_edge_is_rejected() {
        let (err, line) = err_line("flowchart LR\na\n\na -.-> a\n");
        assert_eq!(
            err,
            DiagramError::SelfEdge {
                line: 4,
                id: id("a")
            }
        );
        assert_eq!(line, Some(4));
    }

    #[test]
    fn nested_subgraph_is_rejected_on_the_inner_line() {
        let (err, line) = err_line("flowchart LR\nsubgraph outer\n\nsubgraph inner\nend\nend\n");
        assert_eq!(
            err,
            DiagramError::NestedSubgraph {
                line: 4,
                open: "outer".to_owned(),
                open_line: 2
            }
        );
        assert_eq!(line, Some(4));
    }

    #[test]
    fn unclosed_subgraph_is_reported_on_its_opening_line() {
        let (err, line) = err_line("flowchart LR\na\nsubgraph g\nb\n");
        assert_eq!(
            err,
            DiagramError::UnclosedSubgraph {
                line: 3,
                id: "g".to_owned()
            }
        );
        assert_eq!(line, Some(3));
    }

    #[test]
    fn end_without_open_subgraph_is_rejected() {
        let (err, line) = err_line("flowchart LR\na\nend\n");
        assert_eq!(err, DiagramError::StrayEnd { line: 3 });
        assert_eq!(line, Some(3));
        let (err, line) = err_line("flowchart LR\nsubgraph g\nend\nend\n");
        assert_eq!(err, DiagramError::StrayEnd { line: 4 });
        assert_eq!(line, Some(4));
    }

    #[test]
    fn duplicate_boundary_is_rejected() {
        let (err, line) = err_line("flowchart LR\nsubgraph g\nend\n\nsubgraph g[\"G\"]\nend\n");
        assert_eq!(
            err,
            DiagramError::DuplicateBoundary {
                line: 5,
                id: "g".to_owned(),
                first_line: 2
            }
        );
        assert_eq!(line, Some(5));
    }

    #[test]
    fn subgraph_label_must_be_quoted() {
        for case in [
            "subgraph g[Shared]",
            "subgraph g [\"Shared\"]",
            "subgraph g[\"S\"] x",
        ] {
            let (err, line) = err_line(&format!("flowchart LR\n{case}\nend\n"));
            assert!(
                matches!(err, DiagramError::Unsupported { line: 2, .. }),
                "{case}: {err:?}"
            );
            assert_eq!(line, Some(2));
        }
    }

    #[test]
    fn first_error_in_file_order_wins_for_syntax() {
        let (_, line) = err_line("flowchart LR\na\nstyle a\nb --> c\n");
        assert_eq!(line, Some(3));
    }
}
