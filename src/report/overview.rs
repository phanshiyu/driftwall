//! Renders the architecture overview and the single-node view, as text and as JSON.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Serialize;

use crate::model::{EdgeKind, EdgeSummary, NodeSummary, NodeView, Overview};

use super::ReportError;
use super::common::{arrow, plural, site_str, slash_path};
use super::json::SCHEMA_VERSION;

/// Renders `overview` as uncoloured text: a summary line, the nodes, then the edges.
///
/// Planned edges use `-.->`, undeclared ones are marked `UNDECLARED`, and every edge carries its
/// number of import sites.
#[must_use]
pub fn render_overview_text(overview: &Overview) -> String {
    let count = |kind| {
        overview
            .edges
            .iter()
            .filter(|edge| edge.kind == Some(kind))
            .count()
    };
    let (implemented, planned) = (count(EdgeKind::Implemented), count(EdgeKind::Planned));
    let mut out = format!(
        "driftwall: {} nodes, {} edges ({implemented} implemented, {planned} planned), {} errors, {} warnings\n",
        overview.nodes.len(),
        implemented + planned,
        overview.errors,
        overview.warnings,
    );
    out.push_str("nodes:\n");
    if overview.nodes.is_empty() {
        out.push_str("  (none)\n");
    }
    for node in &overview.nodes {
        // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
        let _ = writeln!(
            out,
            "  {}: {} ({})",
            node.id,
            dirs_str(&node.dirs),
            plural(node.files, "file")
        );
    }
    out.push_str("edges:\n");
    if overview.edges.is_empty() {
        out.push_str("  (none)\n");
    }
    for edge in &overview.edges {
        let _ = writeln!(out, "  {}", edge_line(edge));
    }
    out
}

/// Renders `view` as uncoloured text: the node, then its outgoing and incoming edges, each with
/// up to [`MAX_SITES`](crate::model::MAX_SITES) example import sites.
#[must_use]
pub fn render_node_view_text(view: &NodeView) -> String {
    let NodeView {
        node,
        outgoing,
        incoming,
    } = view;
    let mut out = format!("node: {}\n", node.id);
    // why: fmt::Write into a String is infallible, so the discarded Results carry no error.
    if let Some(label) = &node.label {
        let _ = writeln!(out, "label: {label}");
    }
    let _ = writeln!(out, "dirs: {}", dirs_str(&node.dirs));
    let _ = writeln!(out, "files: {}", node.files);
    for (title, edges) in [("outgoing", outgoing), ("incoming", incoming)] {
        let _ = writeln!(out, "{title}:");
        if edges.is_empty() {
            out.push_str("  (none)\n");
        }
        for edge in edges {
            let _ = writeln!(out, "  {}", edge_line(edge));
            for site in &edge.sites {
                let _ = writeln!(out, "    {}", site_str(site));
            }
            if edge.realised > edge.sites.len() {
                let _ = writeln!(out, "    ... {} more", edge.realised - edge.sites.len());
            }
        }
    }
    out
}

/// Renders `overview` as pretty-printed JSON ending in a newline.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when a value cannot be serialised, such as a path that is
/// not valid UTF-8.
pub fn render_overview_json(overview: &Overview) -> Result<String, ReportError> {
    #[derive(Serialize)]
    struct Document<'a> {
        version: u32,
        summary: Summary,
        nodes: &'a [NodeSummary],
        edges: &'a [EdgeSummary],
    }
    to_json(&Document {
        version: SCHEMA_VERSION,
        summary: Summary {
            errors: overview.errors,
            warnings: overview.warnings,
        },
        nodes: &overview.nodes,
        edges: &overview.edges,
    })
}

/// Renders `view` as pretty-printed JSON ending in a newline.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when a value cannot be serialised.
pub fn render_node_view_json(view: &NodeView) -> Result<String, ReportError> {
    #[derive(Serialize)]
    struct Document<'a> {
        version: u32,
        node: &'a NodeSummary,
        outgoing: &'a [EdgeSummary],
        incoming: &'a [EdgeSummary],
    }
    to_json(&Document {
        version: SCHEMA_VERSION,
        node: &view.node,
        outgoing: &view.outgoing,
        incoming: &view.incoming,
    })
}

#[derive(Serialize)]
struct Summary {
    errors: usize,
    warnings: usize,
}

fn to_json(document: &impl Serialize) -> Result<String, ReportError> {
    let mut out = serde_json::to_string_pretty(document)?;
    out.push('\n');
    Ok(out)
}

/// `a --> b (2 imports)`, with ` UNDECLARED` appended when the contract has no such edge.
fn edge_line(edge: &EdgeSummary) -> String {
    let undeclared = if edge.kind.is_none() {
        " UNDECLARED"
    } else {
        ""
    };
    format!(
        "{} {} {} ({}){undeclared}",
        edge.from,
        arrow(edge.kind),
        edge.to,
        plural(edge.realised, "import"),
    )
}

/// The directories joined by `, `, or `-` for a node no `.arch` file names.
fn dirs_str(dirs: &[PathBuf]) -> String {
    if dirs.is_empty() {
        return "-".to_owned();
    }
    dirs.iter()
        .map(|dir| slash_path(dir))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Location, MAX_SITES, NodeId};
    use serde_json::{Value, json};

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn site(file: &str, line: u32) -> Location {
        Location {
            file: PathBuf::from(file),
            line,
        }
    }

    fn node(raw: &str, dirs: &[&str], files: usize) -> NodeSummary {
        NodeSummary {
            id: id(raw),
            label: Some(format!("{raw}: label")),
            dirs: dirs.iter().map(PathBuf::from).collect(),
            files,
        }
    }

    fn edge(from: &str, to: &str, kind: Option<EdgeKind>, lines: &[u32]) -> EdgeSummary {
        EdgeSummary {
            from: id(from),
            to: id(to),
            kind,
            realised: lines.len(),
            sites: lines
                .iter()
                .take(MAX_SITES)
                .map(|&line| site("src/x.ts", line))
                .collect(),
        }
    }

    fn sample() -> Overview {
        Overview {
            nodes: vec![
                node("cli", &["src"], 3),
                node("lint", &["src/lint", "src/lint2"], 1),
                node("model", &[], 0),
            ],
            edges: vec![
                edge("cli", "lint", Some(EdgeKind::Implemented), &[4, 9, 12, 20]),
                edge("lint", "model", Some(EdgeKind::Planned), &[]),
                edge("model", "cli", None, &[2]),
            ],
            errors: 1,
            warnings: 2,
        }
    }

    #[test]
    fn overview_text_is_exact() {
        let expected = "\
driftwall: 3 nodes, 2 edges (1 implemented, 1 planned), 1 errors, 2 warnings
nodes:
  cli: src (3 files)
  lint: src/lint, src/lint2 (1 file)
  model: - (0 files)
edges:
  cli --> lint (4 imports)
  lint -.-> model (0 imports)
  model --> cli (1 import) UNDECLARED
";
        assert_eq!(render_overview_text(&sample()), expected);
    }

    #[test]
    fn empty_overview_text_says_none() {
        let expected = "\
driftwall: 0 nodes, 0 edges (0 implemented, 0 planned), 0 errors, 0 warnings
nodes:
  (none)
edges:
  (none)
";
        assert_eq!(render_overview_text(&Overview::default()), expected);
    }

    #[test]
    fn node_view_text_is_exact() {
        let view = NodeView {
            node: node("lint", &["src/lint"], 5),
            outgoing: vec![
                edge("lint", "model", Some(EdgeKind::Planned), &[3]),
                edge("lint", "cli", None, &[1, 2, 3, 4, 5]),
            ],
            incoming: vec![],
        };
        let expected = "\
node: lint
label: lint: label
dirs: src/lint
files: 5
outgoing:
  lint -.-> model (1 import)
    src/x.ts:3
  lint --> cli (5 imports) UNDECLARED
    src/x.ts:1
    src/x.ts:2
    src/x.ts:3
    ... 2 more
incoming:
  (none)
";
        assert_eq!(render_node_view_text(&view), expected);
    }

    #[test]
    fn node_view_text_omits_a_missing_label() {
        let mut summary = node("a", &[], 0);
        summary.label = None;
        let view = NodeView {
            node: summary,
            outgoing: vec![],
            incoming: vec![],
        };
        let expected = "\
node: a
dirs: -
files: 0
outgoing:
  (none)
incoming:
  (none)
";
        assert_eq!(render_node_view_text(&view), expected);
    }

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn overview_json_has_the_documented_shape() {
        let value = parse(&render_overview_json(&sample()).unwrap());
        assert_eq!(value["version"], json!(1));
        assert_eq!(value["summary"], json!({ "errors": 1, "warnings": 2 }));
        assert_eq!(
            value["nodes"][1],
            json!({
                "id": "lint",
                "label": "lint: label",
                "dirs": ["src/lint", "src/lint2"],
                "files": 1,
            })
        );
        assert_eq!(
            value["edges"][0],
            json!({
                "from": "cli",
                "to": "lint",
                "kind": "implemented",
                "realised": 4,
                "sites": [
                    { "file": "src/x.ts", "line": 4 },
                    { "file": "src/x.ts", "line": 9 },
                    { "file": "src/x.ts", "line": 12 },
                ],
            })
        );
        assert_eq!(value["edges"][1]["kind"], json!("planned"));
        assert!(value["edges"][2]["kind"].is_null());
    }

    #[test]
    fn node_view_json_has_the_documented_shape() {
        let view = NodeView {
            node: node("lint", &["src/lint"], 5),
            outgoing: vec![edge("lint", "model", Some(EdgeKind::Implemented), &[3])],
            incoming: vec![],
        };
        let value = parse(&render_node_view_json(&view).unwrap());
        assert_eq!(value["version"], json!(1));
        assert_eq!(value["node"]["id"], json!("lint"));
        assert_eq!(value["outgoing"][0]["to"], json!("model"));
        assert_eq!(value["incoming"], json!([]));
    }

    #[test]
    fn json_ends_with_a_newline_and_keeps_key_order() {
        let out = render_overview_json(&sample()).unwrap();
        assert!(out.ends_with("}\n"));
        let at = |key: &str| out.find(key).unwrap();
        assert!(at("\"version\"") < at("\"summary\""));
        assert!(at("\"summary\"") < at("\"nodes\""));
        assert!(at("\"nodes\"") < at("\"edges\""));
    }

    #[test]
    fn text_and_json_spell_edge_kinds_alike() {
        let text = render_overview_text(&sample());
        let value = parse(&render_overview_json(&sample()).unwrap());
        assert!(text.contains("lint -.-> model"));
        assert_eq!(value["edges"][1]["kind"], json!("planned"));
        assert!(text.contains("cli --> lint"));
        assert_eq!(value["edges"][0]["kind"], json!("implemented"));
    }

    #[test]
    fn output_is_deterministic() {
        assert_eq!(
            render_overview_text(&sample()),
            render_overview_text(&sample())
        );
        assert_eq!(
            render_overview_json(&sample()).unwrap(),
            render_overview_json(&sample()).unwrap()
        );
    }
}
