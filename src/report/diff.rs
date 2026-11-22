//! Renders a contract diff, for reviewers of a contract change.

use std::fmt::Write as _;

use serde::Serialize;

use crate::model::{ContractDiff, Edge, EdgeKind, NodeId};

use super::ReportError;
use super::common::{arrow, edge_kind_str};
use super::json::SCHEMA_VERSION;

/// Renders `diff` as uncoloured text, one line per change, or a line saying nothing changed.
///
/// Order: added nodes, removed nodes, added edges, removed edges, changed edges. A planned edge
/// is written `a -.-> b (planned)`; an implemented one needs no suffix.
#[must_use]
pub fn render_diff_text(diff: &ContractDiff) -> String {
    if diff.is_empty() {
        return "driftwall: no contract changes\n".to_owned();
    }
    let mut out = String::new();
    // why: fmt::Write into a String is infallible, so the discarded Results carry no error.
    for id in &diff.added_nodes {
        let _ = writeln!(out, "+ node {id}");
    }
    for id in &diff.removed_nodes {
        let _ = writeln!(out, "- node {id}");
    }
    for (edge, kind) in &diff.added_edges {
        let _ = writeln!(out, "+ edge {}", edge_text(edge, *kind));
    }
    for (edge, kind) in &diff.removed_edges {
        let _ = writeln!(out, "- edge {}", edge_text(edge, *kind));
    }
    for (edge, old, new) in &diff.changed_edges {
        let _ = writeln!(
            out,
            "~ edge {} => {} ({} -> {})",
            edge_text(edge, *old),
            edge_text(edge, *new),
            edge_kind_str(*old),
            edge_kind_str(*new),
        );
    }
    out
}

/// Renders `diff` as pretty-printed JSON ending in a newline.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when a value cannot be serialised.
pub fn render_diff_json(diff: &ContractDiff) -> Result<String, ReportError> {
    let document = Document {
        version: SCHEMA_VERSION,
        added_nodes: &diff.added_nodes,
        removed_nodes: &diff.removed_nodes,
        added_edges: declared(&diff.added_edges),
        removed_edges: declared(&diff.removed_edges),
        changed_edges: diff
            .changed_edges
            .iter()
            .map(|(edge, old, new)| Changed {
                from: &edge.from,
                to: &edge.to,
                old: *old,
                new: *new,
            })
            .collect(),
    };
    let mut out = serde_json::to_string_pretty(&document)?;
    out.push('\n');
    Ok(out)
}

#[derive(Serialize)]
struct Document<'a> {
    version: u32,
    added_nodes: &'a [NodeId],
    removed_nodes: &'a [NodeId],
    added_edges: Vec<Declared<'a>>,
    removed_edges: Vec<Declared<'a>>,
    changed_edges: Vec<Changed<'a>>,
}

#[derive(Serialize)]
struct Declared<'a> {
    from: &'a NodeId,
    to: &'a NodeId,
    kind: EdgeKind,
}

#[derive(Serialize)]
struct Changed<'a> {
    from: &'a NodeId,
    to: &'a NodeId,
    old: EdgeKind,
    new: EdgeKind,
}

fn declared(edges: &[(Edge, EdgeKind)]) -> Vec<Declared<'_>> {
    edges
        .iter()
        .map(|(edge, kind)| Declared {
            from: &edge.from,
            to: &edge.to,
            kind: *kind,
        })
        .collect()
}

/// `a --> b` or `a -.-> b (planned)`.
fn edge_text(edge: &Edge, kind: EdgeKind) -> String {
    let suffix = match kind {
        EdgeKind::Implemented => String::new(),
        EdgeKind::Planned => format!(" ({})", edge_kind_str(kind)),
    };
    format!("{} {} {}{suffix}", edge.from, arrow(Some(kind)), edge.to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: id(from),
            to: id(to),
        }
    }

    fn sample() -> ContractDiff {
        ContractDiff {
            added_nodes: vec![id("x")],
            removed_nodes: vec![id("y")],
            added_edges: vec![
                (edge("a", "b"), EdgeKind::Implemented),
                (edge("a", "c"), EdgeKind::Planned),
            ],
            removed_edges: vec![
                (edge("b", "c"), EdgeKind::Implemented),
                (edge("c", "a"), EdgeKind::Planned),
            ],
            changed_edges: vec![
                (edge("a", "d"), EdgeKind::Planned, EdgeKind::Implemented),
                (edge("d", "a"), EdgeKind::Implemented, EdgeKind::Planned),
            ],
        }
    }

    #[test]
    fn diff_text_is_exact() {
        let expected = "\
+ node x
- node y
+ edge a --> b
+ edge a -.-> c (planned)
- edge b --> c
- edge c -.-> a (planned)
~ edge a -.-> d (planned) => a --> d (planned -> implemented)
~ edge d --> a => d -.-> a (planned) (implemented -> planned)
";
        assert_eq!(render_diff_text(&sample()), expected);
    }

    #[test]
    fn empty_diff_says_no_changes() {
        assert_eq!(
            render_diff_text(&ContractDiff::default()),
            "driftwall: no contract changes\n"
        );
    }

    #[test]
    fn a_change_does_not_print_the_no_changes_line() {
        let diff = ContractDiff {
            added_nodes: vec![id("x")],
            ..ContractDiff::default()
        };
        assert_eq!(render_diff_text(&diff), "+ node x\n");
    }

    fn parse(diff: &ContractDiff) -> Value {
        serde_json::from_str(&render_diff_json(diff).unwrap()).unwrap()
    }

    #[test]
    fn diff_json_has_the_documented_shape() {
        let value = parse(&sample());
        assert_eq!(value["version"], json!(1));
        assert_eq!(value["added_nodes"], json!(["x"]));
        assert_eq!(value["removed_nodes"], json!(["y"]));
        assert_eq!(
            value["added_edges"][1],
            json!({ "from": "a", "to": "c", "kind": "planned" })
        );
        assert_eq!(
            value["removed_edges"][0],
            json!({ "from": "b", "to": "c", "kind": "implemented" })
        );
        assert_eq!(
            value["changed_edges"][0],
            json!({ "from": "a", "to": "d", "old": "planned", "new": "implemented" })
        );
    }

    #[test]
    fn empty_diff_json_has_empty_lists() {
        let value = parse(&ContractDiff::default());
        for key in [
            "added_nodes",
            "removed_nodes",
            "added_edges",
            "removed_edges",
            "changed_edges",
        ] {
            assert_eq!(value[key], json!([]), "{key}");
        }
    }

    #[test]
    fn json_ends_with_a_newline_and_is_deterministic() {
        let first = render_diff_json(&sample()).unwrap();
        assert!(first.ends_with("}\n"));
        assert_eq!(first, render_diff_json(&sample()).unwrap());
        assert_eq!(render_diff_text(&sample()), render_diff_text(&sample()));
    }
}
