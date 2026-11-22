//! Renders the architecture overview and the single-node view, as text and as JSON.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Serialize;

use crate::model::{
    BlindSpots, BlindSummary, BlindTally, EdgeKind, EdgeSummary, ImportKind, MAX_SITES,
    NodeSummary, NodeView, Overview, Site, Symbol,
};

use super::ReportError;
use super::blind;
use super::common::{arrow, capped, plural, site_str, slash_path};
use super::json::SCHEMA_VERSION;

/// Renders `overview` as uncoloured text: a summary line, the nodes, then the edges.
///
/// Planned edges use `-.->`, undeclared ones are marked `UNDECLARED`, and every edge carries its
/// number of import sites and, when imports use any, the symbols they use (the first six, then
/// `+N more`). A node with a declared surface lists it as `exports: ...`, and a node that is blind
/// to anything ends with ` blind: ...`. The text ends with the blind spots block.
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
        let exports = node.exports.as_ref().map_or_else(String::new, |names| {
            format!(" exports: {}", capped(names.iter().map(String::as_str)))
        });
        let _ = writeln!(
            out,
            "  {}: {} ({}){exports}{}",
            node.id,
            dirs_str(&node.dirs),
            plural(node.files, "file"),
            blind::node_suffix(&node.blind_spots),
        );
    }
    out.push_str("edges:\n");
    if overview.edges.is_empty() {
        out.push_str("  (none)\n");
    }
    for edge in &overview.edges {
        let _ = writeln!(out, "  {}", edge_line(edge));
    }
    out.push_str(&blind::summary_block(
        &overview.blind_spots,
        &overview.unmapped,
    ));
    out
}

/// What `neighbors` text shows beyond the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeViewOptions {
    /// List every import site of every edge instead of the first
    /// [`MAX_SITES`](crate::model::MAX_SITES).
    pub all_sites: bool,
    /// List the source files the node owns under the `files:` line.
    pub files: bool,
}

/// Renders `view` as uncoloured text: the node, then its outgoing and incoming edges.
///
/// Each edge has up to [`MAX_SITES`](crate::model::MAX_SITES) example import sites, or all of
/// them with `options.all_sites`. A declared surface is listed in full as `exports: ...` after the
/// file count, and the files themselves follow the `files:` line with `options.files`. The text
/// ends with the node's blind spots, one item per line.
#[must_use]
pub fn render_node_view_text(view: &NodeView, options: NodeViewOptions) -> String {
    let NodeView {
        node,
        outgoing,
        incoming,
        files,
        blind_spots,
    } = view;
    let mut out = format!("node: {}\n", node.id);
    // why: fmt::Write into a String is infallible, so the discarded Results carry no error.
    if let Some(label) = &node.label {
        let _ = writeln!(out, "label: {label}");
    }
    let _ = writeln!(out, "dirs: {}", dirs_str(&node.dirs));
    let _ = writeln!(out, "files: {}", node.files);
    if options.files {
        for file in files {
            let _ = writeln!(out, "  {}", slash_path(file));
        }
    }
    if let Some(names) = &node.exports {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let _ = writeln!(out, "exports: {}", names.join(", "));
    }
    for (title, edges) in [("outgoing", outgoing), ("incoming", incoming)] {
        let _ = writeln!(out, "{title}:");
        if edges.is_empty() {
            out.push_str("  (none)\n");
        }
        for edge in edges {
            let _ = writeln!(out, "  {}", edge_line(edge));
            let shown = if options.all_sites {
                edge.sites.len()
            } else {
                MAX_SITES
            };
            for site in edge.sites.iter().take(shown) {
                let _ = writeln!(out, "    {}", site_str(&site.at));
            }
            if edge.sites.len() > shown {
                let _ = writeln!(out, "    ... {} more", edge.sites.len() - shown);
            }
        }
    }
    out.push_str(&blind::items_block(blind_spots));
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
        blind_spots: Blind<'a>,
    }
    /// The summary beside the tally of what no node owns.
    #[derive(Serialize)]
    struct Blind<'a> {
        #[serde(flatten)]
        summary: &'a BlindSummary,
        unmapped: &'a BlindTally,
    }
    to_json(&Document {
        version: SCHEMA_VERSION,
        summary: Summary {
            errors: overview.errors,
            warnings: overview.warnings,
        },
        nodes: &overview.nodes,
        edges: &overview.edges,
        blind_spots: Blind {
            summary: &overview.blind_spots,
            unmapped: &overview.unmapped,
        },
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
        files: &'a [PathBuf],
        blind_spots: &'a BlindSpots,
    }
    to_json(&Document {
        version: SCHEMA_VERSION,
        node: &view.node,
        outgoing: &view.outgoing,
        incoming: &view.incoming,
        files: &view.files,
        blind_spots: &view.blind_spots,
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

/// The word an edge shows for a use of the target module itself, see "Symbols" in the design.
const MODULE_USE: &str = "module";

/// `a --> b (2 imports)`, then the kinds of the imports (see [`usage`]), ` UNDECLARED` when the
/// contract has no such edge and ` uses: module, x, y` when the imports use the module or symbols.
fn edge_line(edge: &EdgeSummary) -> String {
    let undeclared = if edge.kind.is_none() {
        " UNDECLARED"
    } else {
        ""
    };
    let names: Vec<&str> = edge
        .module
        .then_some(MODULE_USE)
        .into_iter()
        .chain(edge.symbols.iter().map(Symbol::as_str))
        .collect();
    let listed = if names.is_empty() {
        String::new()
    } else {
        format!(" uses: {}", capped(names.into_iter()))
    };
    format!(
        "{} {} {} {}{undeclared}{listed}",
        edge.from,
        arrow(edge.kind),
        edge.to,
        usage(edge),
    )
}

/// How many sites of `edge` hold each property worth showing, in display order.
fn properties(edge: &EdgeSummary) -> [(&'static str, usize); 4] {
    let count = |holds: fn(&Site) -> bool| edge.sites.iter().filter(|site| holds(site)).count();
    [
        ("type-only", count(|s| s.kind == ImportKind::TypeOnly)),
        ("embed", count(|s| s.kind == ImportKind::Embed)),
        ("asset", count(|s| s.kind == ImportKind::Asset)),
        ("test-only", count(|s| s.test_only)),
    ]
}

/// `(2 imports)`. When every site holds a property that is not plain runtime code, one annotation
/// per such property follows, `(3 imports) (type-only)`. When a property holds for some sites
/// only, the counts replace the annotations: `(12 imports, 3 type-only, 2 test-only)`.
fn usage(edge: &EdgeSummary) -> String {
    let imports = plural(edge.realised, "import");
    let present: Vec<(&str, usize)> = properties(edge)
        .into_iter()
        .filter(|&(_, count)| count > 0)
        .collect();
    if present.is_empty() {
        return format!("({imports})");
    }
    if present.iter().all(|&(_, count)| count == edge.realised) {
        let notes: Vec<String> = present
            .iter()
            .map(|(name, _)| format!(" ({name})"))
            .collect();
        return format!("({imports}){}", notes.concat());
    }
    let counts: Vec<String> = present
        .iter()
        .map(|(name, count)| format!(", {count} {name}"))
        .collect();
    format!("({imports}{})", counts.concat())
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
    use crate::model::{Location, NodeId};
    use serde_json::{Value, json};
    use std::collections::BTreeSet;

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn site(file: &str, line: u32) -> Site {
        Site::plain(Location {
            file: PathBuf::from(file),
            line,
        })
    }

    fn node(raw: &str, dirs: &[&str], files: usize) -> NodeSummary {
        NodeSummary {
            id: id(raw),
            label: Some(format!("{raw}: label")),
            dirs: dirs.iter().map(PathBuf::from).collect(),
            files,
            exports: None,
            blind_spots: BlindTally::default(),
        }
    }

    fn exporting(mut summary: NodeSummary, names: &[&str]) -> NodeSummary {
        summary.exports = Some(names.iter().map(|name| (*name).to_owned()).collect());
        summary
    }

    fn using(mut summary: EdgeSummary, names: &[&str]) -> EdgeSummary {
        summary.symbols = names
            .iter()
            .map(|name| match *name {
                "*" => Symbol::All,
                other => Symbol::Name(other.to_owned()),
            })
            .collect();
        summary
    }

    fn edge(from: &str, to: &str, kind: Option<EdgeKind>, lines: &[u32]) -> EdgeSummary {
        EdgeSummary {
            from: id(from),
            to: id(to),
            kind,
            realised: lines.len(),
            sites: lines.iter().map(|&line| site("src/x.ts", line)).collect(),
            symbols: BTreeSet::new(),
            module: false,
        }
    }

    fn sample() -> Overview {
        Overview {
            blind_spots: BlindSummary::default(),
            unmapped: BlindTally::default(),
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
blind spots: none
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
blind spots: none
";
        assert_eq!(render_overview_text(&Overview::default()), expected);
    }

    #[test]
    fn node_view_text_is_exact() {
        let view = NodeView {
            files: Vec::new(),
            blind_spots: BlindSpots::default(),
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
blind spots: none
";
        assert_eq!(
            render_node_view_text(&view, NodeViewOptions::default()),
            expected
        );
    }

    #[test]
    fn node_view_text_omits_a_missing_label() {
        let mut summary = node("a", &[], 0);
        summary.label = None;
        let view = NodeView {
            files: Vec::new(),
            blind_spots: BlindSpots::default(),
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
blind spots: none
";
        assert_eq!(
            render_node_view_text(&view, NodeViewOptions::default()),
            expected
        );
    }

    #[test]
    fn edges_list_the_symbols_they_use_after_the_flags() {
        let overview = Overview {
            blind_spots: BlindSummary::default(),
            unmapped: BlindTally::default(),
            nodes: vec![],
            edges: vec![
                using(
                    edge("a", "b", Some(EdgeKind::Implemented), &[1, 2]),
                    &["fetch", "Client", "*"],
                ),
                using(edge("c", "a", None, &[3]), &["x"]),
                edge("b", "c", Some(EdgeKind::Planned), &[]),
            ],
            errors: 0,
            warnings: 0,
        };
        let text = render_overview_text(&overview);
        let expected = "\
edges:
  a --> b (2 imports) uses: Client, fetch, *
  c --> a (1 import) UNDECLARED uses: x
  b -.-> c (0 imports)
blind spots: none
";
        assert!(text.ends_with(expected), "{text}");
    }

    #[test]
    fn more_than_six_symbols_are_cut_with_a_count() {
        let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let overview = Overview {
            edges: vec![using(
                edge("a", "b", Some(EdgeKind::Implemented), &[1]),
                &names,
            )],
            ..Overview::default()
        };
        let text = render_overview_text(&overview);
        assert!(
            text.contains("  a --> b (1 import) uses: a, b, c, d, e, f +2 more\n"),
            "{text}"
        );
        let seven = &names[..7];
        let overview = Overview {
            edges: vec![using(
                edge("a", "b", Some(EdgeKind::Implemented), &[1]),
                seven,
            )],
            ..Overview::default()
        };
        assert!(render_overview_text(&overview).contains("uses: a, b, c, d, e, f +1 more\n"));
    }

    #[test]
    fn brief_marks_a_declared_surface_on_the_node_line() {
        let overview = Overview {
            nodes: vec![
                exporting(node("gitlab", &["src/gitlab"], 2), &["post_note", "Client"]),
                node("model", &["src/model"], 1),
                exporting(
                    node("big", &["src/big"], 1),
                    &["a", "b", "c", "d", "e", "f", "g"],
                ),
            ],
            ..Overview::default()
        };
        let expected = "\
nodes:
  gitlab: src/gitlab (2 files) exports: Client, post_note
  model: src/model (1 file)
  big: src/big (1 file) exports: a, b, c, d, e, f +1 more
";
        let text = render_overview_text(&overview);
        assert!(text.contains(expected), "{text}");
    }

    #[test]
    fn neighbors_lists_the_whole_surface_and_the_edge_symbols() {
        let view = NodeView {
            files: Vec::new(),
            blind_spots: BlindSpots::default(),
            node: exporting(
                node("b", &["src/b"], 2),
                &["a", "b", "c", "d", "e", "f", "g", "h"],
            ),
            outgoing: vec![],
            incoming: vec![using(
                edge("a", "b", Some(EdgeKind::Implemented), &[4]),
                &["a", "h"],
            )],
        };
        let expected = "\
node: b
label: b: label
dirs: src/b
files: 2
exports: a, b, c, d, e, f, g, h
outgoing:
  (none)
incoming:
  a --> b (1 import) uses: a, h
    src/x.ts:4
blind spots: none
";
        assert_eq!(
            render_node_view_text(&view, NodeViewOptions::default()),
            expected
        );
    }

    #[test]
    fn json_carries_symbols_always_and_exports_only_when_declared() {
        let overview = Overview {
            blind_spots: BlindSummary::default(),
            unmapped: BlindTally::default(),
            nodes: vec![
                exporting(node("a", &[], 0), &["Client", "fetch"]),
                node("b", &[], 0),
            ],
            edges: vec![
                using(
                    edge("a", "b", Some(EdgeKind::Implemented), &[1]),
                    &["fetch", "*", "Client"],
                ),
                edge("b", "a", Some(EdgeKind::Planned), &[]),
            ],
            errors: 0,
            warnings: 0,
        };
        let value = parse(&render_overview_json(&overview).unwrap());
        assert_eq!(value["nodes"][0]["exports"], json!(["Client", "fetch"]));
        assert!(value["nodes"][1].get("exports").is_none());
        assert_eq!(
            value["edges"][0]["symbols"],
            json!(["Client", "fetch", "*"])
        );
        assert_eq!(value["edges"][1]["symbols"], json!([]));

        let view = NodeView {
            files: Vec::new(),
            blind_spots: BlindSpots::default(),
            node: overview.nodes[0].clone(),
            outgoing: overview.edges.clone(),
            incoming: vec![],
        };
        let value = parse(&render_node_view_json(&view).unwrap());
        assert_eq!(value["node"]["exports"], json!(["Client", "fetch"]));
        assert_eq!(
            value["outgoing"][0]["symbols"],
            json!(["Client", "fetch", "*"])
        );
    }

    /// An edge of `n` runtime sites at lines 1.. with the given sites replaced by `mark`.
    fn marked(n: u32, mark: &[(u32, ImportKind, bool)]) -> EdgeSummary {
        let lines: Vec<u32> = (1..=n).collect();
        let mut summary = edge("a", "b", Some(EdgeKind::Implemented), &lines);
        for &(line, kind, test_only) in mark {
            let index = usize::try_from(line - 1).unwrap();
            summary.sites[index].kind = kind;
            summary.sites[index].test_only = test_only;
        }
        summary
    }

    fn line_of(summary: &EdgeSummary) -> String {
        edge_line(summary)
    }

    #[test]
    fn an_edge_whose_sites_all_hold_a_property_is_annotated_with_it() {
        let all = |kind, test_only| {
            let marks: Vec<_> = (1..=3).map(|line| (line, kind, test_only)).collect();
            line_of(&marked(3, &marks))
        };
        assert_eq!(
            all(ImportKind::TypeOnly, false),
            "a --> b (3 imports) (type-only)"
        );
        assert_eq!(all(ImportKind::Embed, false), "a --> b (3 imports) (embed)");
        assert_eq!(all(ImportKind::Asset, false), "a --> b (3 imports) (asset)");
        assert_eq!(
            all(ImportKind::Runtime, true),
            "a --> b (3 imports) (test-only)"
        );
        assert_eq!(
            all(ImportKind::TypeOnly, true),
            "a --> b (3 imports) (type-only) (test-only)"
        );
    }

    #[test]
    fn an_edge_with_mixed_sites_shows_a_count_per_property() {
        let mixed = marked(
            12,
            &[
                (1, ImportKind::TypeOnly, false),
                (2, ImportKind::TypeOnly, false),
                (3, ImportKind::TypeOnly, true),
                (4, ImportKind::Runtime, true),
            ],
        );
        assert_eq!(
            line_of(&mixed),
            "a --> b (12 imports, 3 type-only, 2 test-only)"
        );
    }

    #[test]
    fn a_property_that_holds_everywhere_beside_one_that_does_not_still_gets_counts() {
        let some = marked(
            2,
            &[
                (1, ImportKind::TypeOnly, false),
                (2, ImportKind::TypeOnly, true),
            ],
        );
        assert_eq!(
            line_of(&some),
            "a --> b (2 imports, 2 type-only, 1 test-only)"
        );
    }

    #[test]
    fn plain_runtime_edges_and_empty_edges_show_only_the_count() {
        assert_eq!(line_of(&marked(2, &[])), "a --> b (2 imports)");
        assert_eq!(line_of(&marked(0, &[])), "a --> b (0 imports)");
    }

    #[test]
    fn a_module_use_is_listed_first_as_module_and_never_as_a_star() {
        let mut module_only = marked(1, &[]);
        module_only.module = true;
        assert_eq!(line_of(&module_only), "a --> b (1 import) uses: module");
        let mut both = using(marked(2, &[]), &["Client", "*"]);
        both.module = true;
        assert_eq!(
            line_of(&both),
            "a --> b (2 imports) uses: module, Client, *"
        );
        assert_eq!(
            line_of(&using(marked(1, &[]), &["Client"])),
            "a --> b (1 import) uses: Client"
        );
    }

    #[test]
    fn all_sites_lists_every_site_and_the_default_stops_at_three() {
        let view = NodeView {
            files: Vec::new(),
            blind_spots: BlindSpots::default(),
            node: node("a", &[], 0),
            outgoing: vec![marked(5, &[])],
            incoming: vec![],
        };
        let capped = render_node_view_text(&view, NodeViewOptions::default());
        assert!(
            capped.contains("    src/x.ts:3\n    ... 2 more\n"),
            "{capped}"
        );
        let every = render_node_view_text(
            &view,
            NodeViewOptions {
                all_sites: true,
                ..NodeViewOptions::default()
            },
        );
        assert!(every.contains("    src/x.ts:5\n"), "{every}");
        assert!(!every.contains("more"), "{every}");
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
                "blind_spots": { "unread": 0, "excluded": 0, "ignored": 0, "skipped": 0, "unresolved": 0 },
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
                    { "file": "src/x.ts", "line": 4, "kind": "runtime", "test_only": false },
                    { "file": "src/x.ts", "line": 9, "kind": "runtime", "test_only": false },
                    { "file": "src/x.ts", "line": 12, "kind": "runtime", "test_only": false },
                    { "file": "src/x.ts", "line": 20, "kind": "runtime", "test_only": false },
                ],
                "symbols": [],
                "module": false,
            })
        );
        assert_eq!(value["edges"][1]["kind"], json!("planned"));
        assert!(value["edges"][2]["kind"].is_null());
    }

    #[test]
    fn node_view_json_has_the_documented_shape() {
        let view = NodeView {
            files: Vec::new(),
            blind_spots: BlindSpots::default(),
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
