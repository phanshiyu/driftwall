//! Line-oriented parser for the contract subset.

use std::collections::{BTreeMap, BTreeSet};

use super::error::DiagramError;
use super::label;
use super::rules::{self, RuleParser};
use crate::model::{
    Contract, Edge, EdgeKind, MapGlob, MapRule, MapTarget, NodeId, NodeInfo, Prefix,
};

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
/// A comment line `%% exports <node>: <Name>, <Name>` declares the public surface of a node and is
/// stored in [`Contract::surfaces`], and `%% map <node>: <glob>, <glob>` maps files to a node and
/// is stored in [`Contract::maps`]. Every other `%%` line is a plain comment. The node of an
/// exports line is checked like an edge endpoint, once the whole file has been read.
///
/// # Errors
/// Returns a [`DiagramError`] for the first problem found: anything outside the grammar in
/// `docs/DESIGN.md`, a missing or repeated header, invalid or duplicate ids, undeclared edge
/// endpoints, duplicate or self edges, nested, unclosed or stray subgraph blocks, and malformed,
/// duplicate or undeclared-node `%% exports` lines.
pub fn parse(src: &str) -> Result<Contract, DiagramError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut parser = Parser::default();
    let mut line: u32 = 0;
    for raw in src.lines() {
        line = line.saturating_add(1);
        let text = raw.trim();
        if let Some(directive) = directive(text, "exports") {
            parser.exports(directive, line)?;
            continue;
        }
        if let Some(directive) = directive(text, "map") {
            parser.map(directive, line)?;
            continue;
        }
        if let Some(found) = rules::directive(text) {
            parser.rules.record(&found, line)?;
            continue;
        }
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

struct PendingExports {
    line: u32,
    node: NodeId,
    names: BTreeSet<String>,
}

#[derive(Default)]
struct Parser {
    header_seen: bool,
    nodes: BTreeMap<NodeId, NodeInfo>,
    node_lines: BTreeMap<NodeId, u32>,
    boundaries: BTreeMap<String, u32>,
    open: Option<(String, u32)>,
    edges: Vec<PendingEdge>,
    exports: Vec<PendingExports>,
    exports_lines: BTreeMap<NodeId, u32>,
    maps: Vec<MapRule>,
    map_globs: BTreeMap<String, u32>,
    rules: RuleParser,
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
        if let Some(first_line) = self.node_lines.get(&id) {
            return Err(DiagramError::IdClash {
                line,
                id: id.to_string(),
                first_line: *first_line,
                first_is_boundary: false,
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
        let (planned, after_arrow) = if let Some(after) = rest.strip_prefix(DOTTED) {
            (true, after)
        } else if let Some(after) = rest.strip_prefix(SOLID) {
            (false, after)
        } else {
            return Err(unsupported(text, line));
        };
        let mut after_arrow = after_arrow.trim_start();
        let mut words = (false, false);
        if let Some(labelled) = after_arrow.strip_prefix('|') {
            let (label, after_label) = labelled
                .split_once('|')
                .ok_or_else(|| unsupported(text, line))?;
            // why: `type` and `test` are the label words with a meaning; any other label stays display only.
            words = label::restrictions(label, line)?;
            after_arrow = after_label.trim_start();
        }
        let kind = EdgeKind::from_parts(planned, words.0, words.1);
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

    /// Records the `%% exports` directive whose text after the keyword is `rest`.
    fn exports(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let malformed = |reason: String| DiagramError::MalformedExports { line, reason };
        let (raw_node, list) = rest
            .split_once(':')
            .ok_or_else(|| malformed("expected `<node>: <Name>, <Name>`".to_owned()))?;
        let raw_node = raw_node.trim();
        if raw_node.is_empty() {
            return Err(malformed("no node before the `:`".to_owned()));
        }
        let node = valid_id(raw_node, line)?;
        if list.trim().is_empty() {
            return Err(malformed(format!("`{node}` exports no names")));
        }
        let mut names = BTreeSet::new();
        for raw in list.split(',') {
            let name = raw.trim();
            if name.is_empty() {
                return Err(malformed("empty name in the list".to_owned()));
            }
            if !is_symbol_name(name) {
                return Err(malformed(format!("`{name}` is not an identifier")));
            }
            if !names.insert(name.to_owned()) {
                return Err(malformed(format!("`{name}` is listed twice")));
            }
        }
        if let Some(first_line) = self.exports_lines.get(&node) {
            return Err(DiagramError::DuplicateExports {
                line,
                id: node,
                first_line: *first_line,
            });
        }
        self.exports_lines.insert(node.clone(), line);
        self.exports.push(PendingExports { line, node, names });
        Ok(())
    }

    /// Records the `%% map` directive whose text after the keyword is `rest`.
    fn map(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let malformed = |reason: String| DiagramError::MalformedMap { line, reason };
        let (raw_target, list) = rest
            .split_once(':')
            .ok_or_else(|| malformed("expected `<node>: <glob>, <glob>`".to_owned()))?;
        let raw_target = raw_target.trim();
        let target = match raw_target.strip_suffix('*') {
            Some("") => MapTarget::Modules(Prefix::default()),
            Some(prefix) => {
                MapTarget::Modules(Prefix::new(prefix).map_err(|e| malformed(e.to_string()))?)
            }
            None if raw_target.is_empty() => {
                return Err(malformed("no node before the `:`".to_owned()));
            }
            None => MapTarget::Node(valid_id(raw_target, line)?),
        };
        let mut globs = Vec::new();
        for raw in list.split(',') {
            let glob =
                MapGlob::parse(raw).map_err(|e| malformed(format!("`{}`: {e}", raw.trim())))?;
            let key = glob.normalized();
            if let Some(first_line) = self.map_globs.get(&key) {
                return Err(DiagramError::DuplicateMapGlob {
                    line,
                    glob: glob.to_string(),
                    first_line: *first_line,
                });
            }
            self.map_globs.insert(key, line);
            globs.push(glob);
        }
        if matches!(target, MapTarget::Modules(_)) {
            let single = globs.len() == 1 && globs.iter().all(|g| g.directory().is_some());
            if !single {
                return Err(malformed(
                    "a module target takes exactly one directory glob such as `src/ui/`".to_owned(),
                ));
            }
        }
        self.maps.push(MapRule {
            line,
            target,
            globs,
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
        if let Some(first_line) = self.boundaries.get(id.as_str()) {
            return Err(DiagramError::IdClash {
                line,
                id: id.to_string(),
                first_line: *first_line,
                first_is_boundary: true,
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
        let mut surfaces = BTreeMap::new();
        for pending in self.exports {
            if !self.nodes.contains_key(&pending.node) {
                return Err(DiagramError::UndeclaredExportsNode {
                    line: pending.line,
                    id: pending.node,
                });
            }
            surfaces.insert(pending.node, pending.names);
        }
        for rule in &self.maps {
            if let MapTarget::Node(id) = &rule.target
                && !self.nodes.contains_key(id)
            {
                return Err(DiagramError::UndeclaredMapNode {
                    line: rule.line,
                    id: id.clone(),
                });
            }
        }
        let rules = self
            .rules
            .finish(&self.nodes, &self.boundaries, &edges, &edge_lines)?;
        Ok(Contract {
            nodes: self.nodes,
            edges,
            surfaces,
            maps: self.maps,
            rules,
        })
    }
}

/// The text after the keyword when `text` is a `%% <keyword>` directive: a comment whose first word
/// is `keyword`. Any other comment, and anything that is not a comment, is `None`.
fn directive<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = text
        .strip_prefix("%%")?
        .trim_start()
        .strip_prefix(keyword)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// Whether `name` can be a symbol: a letter, `_` or `$`, then letters, digits, `_` or `$`.
fn is_symbol_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
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
    fn node_and_boundary_cannot_share_an_id() {
        let (err, line) =
            err_line("flowchart LR\n  subgraph shared[\"S\"]\n    shared[\"shared: x\"]\n  end\n");
        assert_eq!(
            err,
            DiagramError::IdClash {
                line: 3,
                id: "shared".to_owned(),
                first_line: 2,
                first_is_boundary: true
            }
        );
        assert_eq!(line, Some(3));
        let (err, line) = err_line("flowchart LR\nx\n\nsubgraph x\nend\n");
        assert_eq!(
            err,
            DiagramError::IdClash {
                line: 4,
                id: "x".to_owned(),
                first_line: 2,
                first_is_boundary: false
            }
        );
        assert_eq!(line, Some(4));
        assert!(err.to_string().contains("already declared as a node"));
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

    fn names(list: &[&str]) -> std::collections::BTreeSet<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn exports_lines_declare_surfaces() {
        let src = "flowchart LR\na\nb\n%% exports a: Client, fetch_mrs\n%% exports b: default\n";
        let contract = parse(src).unwrap();
        assert_eq!(contract.surfaces.len(), 2);
        assert_eq!(contract.surfaces[&id("a")], names(&["Client", "fetch_mrs"]));
        assert_eq!(contract.surfaces[&id("b")], names(&["default"]));
    }

    #[test]
    fn exports_lines_tolerate_whitespace_and_any_position() {
        let src = "%%exports   a  :  X ,Y,  $z\r\n\t%%   exports b: _w\nflowchart LR\n  a\n  b\n";
        let contract = parse(src).unwrap();
        assert_eq!(contract.surfaces[&id("a")], names(&["X", "Y", "$z"]));
        assert_eq!(contract.surfaces[&id("b")], names(&["_w"]));
    }

    #[test]
    fn exports_may_name_a_node_declared_later_or_inside_a_boundary() {
        let src = "flowchart LR\n%% exports a: X\nsubgraph g\na\nend\n";
        assert_eq!(parse(src).unwrap().surfaces[&id("a")], names(&["X"]));
    }

    #[test]
    fn a_node_without_an_exports_line_has_no_surface() {
        let contract = parse("flowchart LR\na\nb\n%% exports a: X\n").unwrap();
        assert!(!contract.surfaces.contains_key(&id("b")));
    }

    #[test]
    fn other_comments_stay_plain_comments() {
        let src = "flowchart LR\na\n%% exportsx a: X\n%% exports: a\n%% export a: X\n%% see exports a: X\n%% Exports a: X\n%%\n";
        assert!(parse(src).unwrap().surfaces.is_empty());
    }

    #[test]
    fn a_comment_that_starts_with_the_word_exports_is_a_directive_or_an_error() {
        let (err, _) = err_line("flowchart LR\na\n%% exports are listed below\n");
        assert!(
            matches!(err, DiagramError::MalformedExports { line: 3, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn exports_naming_an_undeclared_node_is_reported_on_its_line_after_the_whole_file() {
        let (err, line) = err_line("flowchart LR\na\n\n%% exports ghost: X\nb\n");
        assert_eq!(
            err,
            DiagramError::UndeclaredExportsNode {
                line: 4,
                id: id("ghost")
            }
        );
        assert_eq!(line, Some(4));
    }

    #[test]
    fn a_boundary_id_is_not_a_node_for_exports() {
        let (err, _) = err_line("flowchart LR\nsubgraph g\na\nend\n%% exports g: X\n");
        assert_eq!(
            err,
            DiagramError::UndeclaredExportsNode {
                line: 5,
                id: id("g")
            }
        );
    }

    #[test]
    fn a_second_exports_line_for_a_node_is_rejected_with_both_lines() {
        let (err, line) = err_line("flowchart LR\na\n%% exports a: X\n\n%% exports a: Y\n");
        assert_eq!(
            err,
            DiagramError::DuplicateExports {
                line: 5,
                id: id("a"),
                first_line: 3
            }
        );
        assert_eq!(line, Some(5));
    }

    #[test]
    fn empty_or_malformed_export_lists_are_rejected_on_their_line() {
        let cases = [
            "%% exports",
            "%% exports a",
            "%% exports a:",
            "%% exports a:   ",
            "%% exports : X",
            "%% exports a: X,",
            "%% exports a: ,X",
            "%% exports a: X,,Y",
            "%% exports a: X Y",
            "%% exports a: X, X",
            "%% exports a: *",
            "%% exports a: 1abc",
            "%% exports a: X.Y",
            "%% exports a: X %% why",
        ];
        for case in cases {
            let (err, line) = err_line(&format!("flowchart LR\na\n\n{case}\n"));
            assert!(
                matches!(err, DiagramError::MalformedExports { line: 4, .. }),
                "{case}: {err:?}"
            );
            assert_eq!(line, Some(4), "{case}");
        }
    }

    #[test]
    fn an_exports_line_with_an_invalid_node_id_is_an_invalid_id() {
        let (err, line) = err_line("flowchart LR\na\n%% exports 1a: X\n");
        assert!(
            matches!(err, DiagramError::InvalidId { line: 3, .. }),
            "{err:?}"
        );
        assert_eq!(line, Some(3));
    }

    #[test]
    fn first_error_in_file_order_wins_for_syntax() {
        let (_, line) = err_line("flowchart LR\na\nstyle a\nb --> c\n");
        assert_eq!(line, Some(3));
    }

    fn rule_text(contract: &Contract) -> Vec<String> {
        contract
            .maps
            .iter()
            .map(|rule| {
                let globs: Vec<&str> = rule.globs.iter().map(MapGlob::as_str).collect();
                format!("{}@{}: {}", rule.target, rule.line, globs.join("|"))
            })
            .collect()
    }

    #[test]
    fn map_lines_declare_rules_with_their_lines() {
        let src = "flowchart LR\na\nb\n%% map a: src/a/, lib/*.ts\n  %%map   b :  ./x.ts\n%% map a: more/\n";
        let contract = parse(src).unwrap();
        assert_eq!(
            rule_text(&contract),
            ["a@4: src/a/|lib/*.ts", "b@5: ./x.ts", "a@6: more/"]
        );
    }

    #[test]
    fn module_form_takes_one_plain_directory() {
        let contract = parse("flowchart LR\na-x\n%% map a-*: src/\n%% map *: ./\n").unwrap();
        assert_eq!(rule_text(&contract), ["a-*@3: src/", "*@4: ./"]);
        for bad in [
            "%% map a-*: src/, lib/",
            "%% map a-*: src/*.ts",
            "%% map a-*: src/x",
            "%% map a*: src/",
        ] {
            let (err, line) = err_line(&format!("flowchart LR\na\n{bad}\n"));
            assert!(
                matches!(err, DiagramError::MalformedMap { .. }),
                "{bad}: {err:?}"
            );
            assert_eq!(line, Some(3), "{bad}");
        }
    }

    #[test]
    fn a_map_naming_an_undeclared_node_is_reported_on_its_line_after_the_whole_file() {
        let (err, line) = err_line("flowchart LR\na\n%% map ghost: x/\nb\n");
        assert_eq!(
            err,
            DiagramError::UndeclaredMapNode {
                line: 3,
                id: id("ghost")
            }
        );
        assert_eq!(line, Some(3));
    }

    #[test]
    fn malformed_map_lines_are_errors_on_their_line() {
        for bad in [
            "%% map",
            "%% map a",
            "%% map a:",
            "%% map : x/",
            "%% map a: x/,",
            "%% map a: !x/",
            "%% map a: ../x/",
            "%% map a: x/[ab]",
            "%% map a: a**b/",
            "%% map of the system",
        ] {
            let (err, line) = err_line(&format!("flowchart LR\na\n{bad}\n"));
            assert!(
                matches!(err, DiagramError::MalformedMap { line: 3, .. }),
                "{bad}: {err:?}"
            );
            assert_eq!(line, Some(3), "{bad}");
        }
        let (err, _) = err_line("flowchart LR\na\n%% map 1a: x/\n");
        assert!(
            matches!(err, DiagramError::InvalidId { line: 3, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn a_repeated_glob_is_rejected_with_both_lines_whatever_its_spelling() {
        let (err, _) = err_line("flowchart LR\na\nb\n%% map a: src/x/\n%% map b: /src/x/\n");
        assert_eq!(
            err,
            DiagramError::DuplicateMapGlob {
                line: 5,
                glob: "/src/x/".to_owned(),
                first_line: 4
            }
        );
        let (err, _) = err_line("flowchart LR\na\n%% map a: s/, s/\n");
        assert!(matches!(
            err,
            DiagramError::DuplicateMapGlob {
                line: 3,
                first_line: 3,
                ..
            }
        ));
    }

    #[test]
    fn only_a_comment_whose_first_word_is_map_is_a_directive() {
        let ok =
            parse("flowchart LR\na\n%% mapped a: x/\n%% see map a: x/\n%% Map a: x/\n").unwrap();
        assert_eq!(ok.maps, Vec::new());
        let (err, _) = err_line("flowchart LR\na\n%% mapped a: x/\n%%map\n");
        assert!(
            matches!(err, DiagramError::MalformedMap { line: 4, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn the_test_label_makes_a_test_edge_and_any_other_label_stays_plain() {
        let kind = |arrow: &str| {
            let contract = parse(&format!("flowchart LR\na\nb\na {arrow} b\n")).unwrap();
            contract.edges.values().copied().collect::<Vec<_>>()
        };
        assert_eq!(kind("-->|test|"), [EdgeKind::Test]);
        assert_eq!(kind("-->| test |"), [EdgeKind::Test]);
        assert_eq!(kind("-.->|test|"), [EdgeKind::PlannedTest]);
        assert_eq!(kind("-->|Test|"), [EdgeKind::Implemented]);
        assert_eq!(kind("-->|tests|"), [EdgeKind::Implemented]);
        assert_eq!(kind("-.->|uses|"), [EdgeKind::Planned]);
    }

    #[test]
    fn the_type_label_and_the_type_test_label_make_restricted_edges() {
        let kind = |arrow: &str| {
            let contract = parse(&format!("flowchart LR\na\nb\na {arrow} b\n")).unwrap();
            contract.edges.values().copied().collect::<Vec<_>>()
        };
        assert_eq!(kind("-->|type|"), [EdgeKind::Type]);
        assert_eq!(kind("-.->|type|"), [EdgeKind::PlannedType]);
        assert_eq!(kind("-->|type, test|"), [EdgeKind::TypeTest]);
        assert_eq!(kind("-.->|test,type|"), [EdgeKind::PlannedTypeTest]);
        assert_eq!(kind("-->|Type|"), [EdgeKind::Implemented]);
        assert_eq!(kind("-->|type, helper|"), [EdgeKind::Implemented]);
    }

    #[test]
    fn a_label_naming_a_word_twice_is_a_contract_error_at_its_line() {
        let (err, _) = err_line("flowchart LR\na\nb\na -->|type, type| b\n");
        assert!(
            matches!(err, DiagramError::RepeatedLabelWord { line: 4, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn type_edges_survive_a_render_round_trip() {
        let src = "flowchart LR\na\nb\nc\nd\na -->|type| b\nb -.->|type| c\nc -->|type, test| d\n";
        let contract = parse(src).unwrap();
        let rendered = crate::diagram::render(&contract);
        assert!(rendered.contains("a -->|type| b"), "{rendered}");
        assert!(rendered.contains("c -->|type, test| d"), "{rendered}");
        assert_eq!(parse(&rendered).unwrap(), contract);
    }

    #[test]
    fn test_edges_survive_a_render_round_trip() {
        let src = "flowchart LR\na\nb\nc\na -->|test| b\nb -.->|test| c\n";
        let contract = parse(src).unwrap();
        let rendered = crate::diagram::render(&contract);
        assert!(rendered.contains("a -->|test| b"), "{rendered}");
        assert!(rendered.contains("b -.->|test| c"), "{rendered}");
        assert_eq!(parse(&rendered).unwrap(), contract);
    }

    #[test]
    fn a_solid_and_a_test_edge_on_one_pair_is_a_duplicate_edge() {
        let (err, _) = err_line("flowchart LR\na\nb\na --> b\na -->|test| b\n");
        assert!(
            matches!(err, DiagramError::DuplicateEdge { line: 5, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn the_label_inside_the_arrow_is_a_contract_error() {
        let (err, _) = err_line("flowchart LR\na\nb\na -- test --> b\n");
        assert!(
            matches!(err, DiagramError::Unsupported { line: 4, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn map_rules_survive_a_render_round_trip_without_their_line_numbers() {
        let src = "flowchart LR\na\nb\n\n\n%% map b: z/\n%% map a: src/, lib/*.ts\n";
        let contract = parse(src).unwrap();
        let rendered = crate::diagram::render(&contract);
        let again = parse(&rendered).unwrap();
        assert_eq!(again, contract);
        assert_eq!(again.maps.len(), 2);
        assert!(
            rendered.ends_with("  %% map b: z/\n  %% map a: src/, lib/*.ts\n"),
            "{rendered}"
        );
    }
}
