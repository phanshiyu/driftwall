//! Renders a contract diff, for reviewers of a contract change.

use std::fmt::Write as _;

use serde::Serialize;

use std::fmt::Display;

use crate::model::{
    BoundaryMove, ContractDiff, DebtAdded, Edge, EdgeEnds, EdgeKind, MoveCause, Moved, NodeId,
};

use super::ReportError;
use super::common::{arrow, edge_kind_str};
use super::json::SCHEMA_VERSION;

/// Renders `diff` as uncoloured text, one line per change, or a line saying nothing changed.
///
/// Order: added nodes, removed nodes, added edges, removed edges, changed edges, then the same three
/// for boundary edges, added exports, removed exports, then the `rules changed:` and `moved between
/// nodes:` sections. A planned edge is written `a -.-> b (planned)`; an implemented one needs no
/// suffix. An exported name is written `+ exports node: Name`, one line per name, so a node that
/// loses every name (and with that its surface) shows only `-` lines.
#[must_use]
pub fn render_diff_text(diff: &ContractDiff) -> String {
    render_diff_text_with_debt(diff, &[])
}

/// Like [`render_diff_text`], and a `debt added:` section after the contract changes, one
/// `  <kind> <key>: <old> -> <new>` line per baseline entry that is new or grew (see "Debt
/// baseline" in `docs/DESIGN.md`).
#[must_use]
pub fn render_diff_text_with_debt(diff: &ContractDiff, debt: &[DebtAdded]) -> String {
    if diff.is_empty() && debt.is_empty() {
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
    for (ends, kind) in &diff.added_boundary_edges {
        let _ = writeln!(out, "+ edge {}", ends_text(&ends.from, &ends.to, *kind));
    }
    for (ends, kind) in &diff.removed_boundary_edges {
        let _ = writeln!(out, "- edge {}", ends_text(&ends.from, &ends.to, *kind));
    }
    for (ends, old, new) in &diff.changed_boundary_edges {
        let _ = writeln!(
            out,
            "~ edge {} => {} ({} -> {})",
            ends_text(&ends.from, &ends.to, *old),
            ends_text(&ends.from, &ends.to, *new),
            edge_kind_str(*old),
            edge_kind_str(*new),
        );
    }
    for (node, name) in &diff.added_exports {
        let _ = writeln!(out, "+ exports {node}: {name}");
    }
    for (node, name) in &diff.removed_exports {
        let _ = writeln!(out, "- exports {node}: {name}");
    }
    push_rules(&mut out, diff);
    push_moved(&mut out, &diff.moved);
    if !debt.is_empty() {
        out.push_str("debt added:\n");
    }
    for added in debt {
        let _ = writeln!(
            out,
            "  {} {}: {} -> {}",
            added.kind, added.key, added.old, added.new
        );
    }
    out
}

/// The `rules changed:` section: directive lines added and removed, then nodes that changed
/// boundary. Nothing when there are none.
fn push_rules(out: &mut String, diff: &ContractDiff) {
    if diff.rules_added.is_empty()
        && diff.rules_removed.is_empty()
        && diff.boundary_moves.is_empty()
    {
        return;
    }
    out.push_str("rules changed:\n");
    // why: fmt::Write into a String is infallible, so the discarded Results carry no error.
    for line in &diff.rules_added {
        let _ = writeln!(out, "  + {line}");
    }
    for line in &diff.rules_removed {
        let _ = writeln!(out, "  - {line}");
    }
    for BoundaryMove { node, from, to } in &diff.boundary_moves {
        let (from, to) = (from.as_deref(), to.as_deref());
        let _ = writeln!(
            out,
            "  node {node} moved from boundary {} to {}",
            from.unwrap_or("(none)"),
            to.unwrap_or("(none)")
        );
    }
}

/// The `moved between nodes:` section, one `<file>: <old> -> <new> (<cause>)` line per file.
fn push_moved(out: &mut String, moved: &[Moved]) {
    if moved.is_empty() {
        return;
    }
    out.push_str("moved between nodes:\n");
    let owner = |node: &Option<NodeId>| {
        node.as_ref()
            .map_or_else(|| "(none)".to_owned(), ToString::to_string)
    };
    for file in moved {
        let cause = match &file.cause {
            MoveCause::Renamed { from } => format!("renamed from {}", from.display()),
            MoveCause::RenamedAndMappingChanged { from } => {
                format!("renamed from {}, mapping changed", from.display())
            }
            MoveCause::MappingChanged => "mapping changed".to_owned(),
        };
        let _ = writeln!(
            out,
            "  {}: {} -> {} ({cause})",
            file.file.display(),
            owner(&file.from_node),
            owner(&file.to_node)
        );
    }
}

/// Renders `diff` as pretty-printed JSON ending in a newline.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when a value cannot be serialised.
pub fn render_diff_json(diff: &ContractDiff) -> Result<String, ReportError> {
    diff_json(diff, None)
}

/// Like [`render_diff_json`], and `debt_added`, the baseline entries that are new or grew, as
/// `{kind, key, old, new}`, last.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when a value cannot be serialised.
pub fn render_diff_json_with_debt(
    diff: &ContractDiff,
    debt: &[DebtAdded],
) -> Result<String, ReportError> {
    diff_json(diff, Some(debt))
}

fn diff_json(diff: &ContractDiff, debt: Option<&[DebtAdded]>) -> Result<String, ReportError> {
    let document = Document {
        debt_added: debt,
        version: SCHEMA_VERSION,
        added_nodes: &diff.added_nodes,
        removed_nodes: &diff.removed_nodes,
        added_edges: declared(&diff.added_edges),
        removed_edges: declared(&diff.removed_edges),
        added_boundary_edges: ended(&diff.added_boundary_edges),
        removed_boundary_edges: ended(&diff.removed_boundary_edges),
        changed_boundary_edges: diff
            .changed_boundary_edges
            .iter()
            .map(|(ends, old, new)| ChangedEnds {
                from: ends.from.as_str(),
                to: ends.to.as_str(),
                old: *old,
                new: *new,
            })
            .collect(),
        added_exports: exported(&diff.added_exports),
        removed_exports: exported(&diff.removed_exports),
        rules_added: &diff.rules_added,
        rules_removed: &diff.rules_removed,
        boundary_moves: &diff.boundary_moves,
        moved: &diff.moved,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    added_boundary_edges: Vec<DeclaredEnds<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    removed_boundary_edges: Vec<DeclaredEnds<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    changed_boundary_edges: Vec<ChangedEnds<'a>>,
    added_exports: Vec<Exported<'a>>,
    removed_exports: Vec<Exported<'a>>,
    rules_added: &'a [String],
    rules_removed: &'a [String],
    boundary_moves: &'a [BoundaryMove],
    moved: &'a [Moved],
    #[serde(skip_serializing_if = "Option::is_none")]
    debt_added: Option<&'a [DebtAdded]>,
}

#[derive(Serialize)]
struct Exported<'a> {
    node: &'a NodeId,
    symbol: &'a str,
}

fn exported(names: &[(NodeId, String)]) -> Vec<Exported<'_>> {
    names
        .iter()
        .map(|(node, symbol)| Exported {
            node,
            symbol: symbol.as_str(),
        })
        .collect()
}

#[derive(Serialize)]
struct Declared<'a> {
    from: &'a NodeId,
    to: &'a NodeId,
    kind: EdgeKind,
}

/// A boundary edge as JSON: its ends are ids, and the kind is one of the kinds of a node edge.
#[derive(Serialize)]
struct DeclaredEnds<'a> {
    from: &'a str,
    to: &'a str,
    kind: EdgeKind,
}

#[derive(Serialize)]
struct ChangedEnds<'a> {
    from: &'a str,
    to: &'a str,
    old: EdgeKind,
    new: EdgeKind,
}

fn ended(edges: &[(EdgeEnds, EdgeKind)]) -> Vec<DeclaredEnds<'_>> {
    edges
        .iter()
        .map(|(ends, kind)| DeclaredEnds {
            from: ends.from.as_str(),
            to: ends.to.as_str(),
            kind: *kind,
        })
        .collect()
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
    ends_text(&edge.from, &edge.to, kind)
}

/// The same for the two ends of a boundary edge, which may be nodes or boundaries.
fn ends_text(from: &impl Display, to: &impl Display, kind: EdgeKind) -> String {
    let suffix = match kind {
        EdgeKind::Implemented => String::new(),
        _ => format!(" ({})", edge_kind_str(kind)),
    };
    format!("{from} {} {to}{suffix}", arrow(Some(kind)))
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
            added_exports: vec![(id("a"), "X".to_owned()), (id("a"), "Z".to_owned())],
            removed_exports: vec![(id("b"), "Y".to_owned())],
            ..ContractDiff::default()
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
+ exports a: X
+ exports a: Z
- exports b: Y
";
        assert_eq!(render_diff_text(&sample()), expected);
    }

    #[test]
    fn test_edges_show_their_label_and_kind() {
        let diff = ContractDiff {
            added_edges: vec![(edge("a", "b"), EdgeKind::Test)],
            changed_edges: vec![(edge("a", "c"), EdgeKind::Implemented, EdgeKind::PlannedTest)],
            ..ContractDiff::default()
        };
        assert_eq!(
            render_diff_text(&diff),
            "+ edge a -->|test| b (test)\n~ edge a --> c => a -.->|test| c (planned-test) (implemented -> planned-test)\n"
        );
    }

    #[test]
    fn an_export_change_alone_is_a_change() {
        let diff = ContractDiff {
            removed_exports: vec![(id("gitlab"), "Y".to_owned())],
            ..ContractDiff::default()
        };
        assert_eq!(render_diff_text(&diff), "- exports gitlab: Y\n");
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
        assert_eq!(
            value["added_exports"],
            json!([{ "node": "a", "symbol": "X" }, { "node": "a", "symbol": "Z" }])
        );
        assert_eq!(
            value["removed_exports"],
            json!([{ "node": "b", "symbol": "Y" }])
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
            "added_exports",
            "removed_exports",
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
