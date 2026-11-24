//! The presentation of the draft contract: layers, boundaries in dependency order, and cycles.
//!
//! Pure text from a [`Contract`]. See "The draft reads as a design" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Contract, Edge, EdgeKind, MapGlob, NodeId};

const PREAMBLE: [&str; 4] = [
    "%% driftwall architecture contract.",
    "%% Solid arrow  (-->)  = implemented dependency, enforced.",
    "%% Dotted arrow (-.->) = planned dependency, tolerated until built.",
    "%% Label |test|        = the dependency is allowed from test code only.",
];

/// The comment that explains the `observed packages` lines.
const OBSERVED_NOTE: &str = "  %% Observed third-party packages per node, a starting point and not enforced: write a line as `%% packages <node>: ..` to enforce it.";

/// Separator between the members of a cycle.
const CYCLE_JOIN: &str = " <-> ";

/// The strongly connected components of the graph on `nodes` with `edges`, each with its members
/// sorted, and the components sorted by their first member. Singletons are components too.
fn components<'a>(
    nodes: &[&'a NodeId],
    edges: &[(&'a NodeId, &'a NodeId)],
) -> Vec<Vec<&'a NodeId>> {
    let mut next: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for (from, to) in edges {
        next.entry(from).or_default().push(to);
    }
    // why: a node can reach itself through a cycle only, so reachability from each node is enough to find mutual pairs.
    let mut reach: BTreeMap<&NodeId, BTreeSet<&NodeId>> = BTreeMap::new();
    for node in nodes {
        let mut seen: BTreeSet<&NodeId> = BTreeSet::new();
        let mut stack: Vec<&NodeId> = next.get(node).cloned().unwrap_or_default();
        while let Some(current) = stack.pop() {
            if seen.insert(current) {
                stack.extend(next.get(current).into_iter().flatten().copied());
            }
        }
        reach.insert(node, seen);
    }
    let mut by_first: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for node in nodes {
        let first = reach
            .get(node)
            .into_iter()
            .flatten()
            .filter(|other| reach.get(*other).is_some_and(|back| back.contains(node)))
            .chain(std::iter::once(node))
            .min()
            .copied()
            .unwrap_or(node);
        by_first.entry(first).or_default().push(node);
    }
    by_first
        .into_values()
        .map(|mut members| {
            members.sort();
            members
        })
        .collect()
}

/// The rank of every node: the longest path, over the condensation of `edges`, from a component
/// nothing else depends on.
fn ranks<'a>(
    components: &[Vec<&'a NodeId>],
    edges: &[(&'a NodeId, &'a NodeId)],
) -> BTreeMap<&'a NodeId, usize> {
    let component_of: BTreeMap<&NodeId, usize> = components
        .iter()
        .enumerate()
        .flat_map(|(index, members)| members.iter().map(move |member| (*member, index)))
        .collect();
    let mut rank = vec![0_usize; components.len()];
    let steps: Vec<(usize, usize)> = edges
        .iter()
        .filter_map(|(from, to)| {
            let (from, to) = (*component_of.get(from)?, *component_of.get(to)?);
            (from != to).then_some((from, to))
        })
        .collect();
    // why: the condensation is a DAG, so relaxing every edge until nothing changes ends within as many rounds as the longest path.
    loop {
        let mut changed = false;
        for (from, to) in &steps {
            let candidate = rank.get(*from).copied().unwrap_or(0) + 1;
            if let Some(slot) = rank.get_mut(*to)
                && *slot < candidate
            {
                *slot = candidate;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    component_of
        .into_iter()
        .map(|(node, index)| (node, rank.get(index).copied().unwrap_or(0)))
        .collect()
}

/// A node or a boundary, positioned in the output.
enum Item<'a> {
    Boundary(&'a str, Vec<&'a NodeId>),
    Node(&'a NodeId),
}

/// Renders `contract` in dependency order, with its boundaries, a comment per cycle and one per
/// node in `observed`, the third-party packages it uses.
///
/// Test edges (`-->|test|`) are the edges only test code backs: they do not decide the order and
/// their cycles are named apart. The text parses back to `contract`.
pub(super) fn render(contract: &Contract, observed: &BTreeMap<NodeId, BTreeSet<String>>) -> String {
    let ids: Vec<&NodeId> = contract.nodes.keys().collect();
    let every: Vec<(&NodeId, &NodeId)> = contract
        .edges
        .keys()
        .map(|edge| (&edge.from, &edge.to))
        .collect();
    let production: Vec<(&NodeId, &NodeId)> = contract
        .edges
        .iter()
        .filter(|(_, kind)| !kind.is_test())
        .map(|(edge, _)| (&edge.from, &edge.to))
        .collect();
    let production_components = components(&ids, &production);
    let rank = ranks(&production_components, &production);
    let rank_of = |id: &NodeId| rank.get(id).copied().unwrap_or(0);

    let cycles = cycle_lines(&production_components, &components(&ids, &every));
    let mut lines: Vec<String> = PREAMBLE.iter().map(|line| (*line).to_owned()).collect();
    lines.push(summary_line(
        contract.nodes.len(),
        contract.edges.len(),
        cycles.len(),
    ));
    lines.push("%% Read left to right: entry points first, foundations last.".to_owned());
    lines.push("flowchart LR".to_owned());

    let mut by_boundary: BTreeMap<&str, Vec<&NodeId>> = BTreeMap::new();
    let mut items: Vec<(usize, String, Item)> = Vec::new();
    for (id, info) in &contract.nodes {
        match &info.boundary {
            Some(boundary) => by_boundary.entry(boundary).or_default().push(id),
            None => items.push((rank_of(id), id.to_string(), Item::Node(id))),
        }
    }
    for (boundary, mut members) in by_boundary {
        members.sort_by_key(|id| (rank_of(id), (*id).clone()));
        let first = members.first().map_or(0, |id| rank_of(id));
        items.push((
            first,
            boundary.to_owned(),
            Item::Boundary(boundary, members),
        ));
    }
    items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));

    let node_line = |id: &NodeId, indent: &str| {
        let label = contract.nodes.get(id).and_then(|info| info.label.as_ref());
        label.map_or_else(
            || format!("{indent}{id}"),
            |label| format!("{indent}{id}[\"{}\"]", sanitize(label)),
        )
    };
    let mut after_node = false;
    for (_, _, item) in &items {
        // why: loose nodes sit together in one block, boundaries stand apart.
        if !(after_node && matches!(item, Item::Node(_))) {
            lines.push(String::new());
        }
        after_node = matches!(item, Item::Node(_));
        match item {
            Item::Node(id) => lines.push(node_line(id, "  ")),
            Item::Boundary(boundary, members) => {
                lines.push(format!("  subgraph {boundary}[\"{}\"]", sanitize(boundary)));
                lines.extend(members.iter().map(|id| node_line(id, "    ")));
                lines.push("  end".to_owned());
            }
        }
    }

    if !contract.edges.is_empty() {
        lines.push(String::new());
        lines.extend(edge_lines(contract, rank_of));
    }
    if !cycles.is_empty() {
        lines.push(String::new());
        lines.extend(cycles.iter().map(|line| format!("  {line}")));
    }
    if !observed.is_empty() {
        lines.push(String::new());
        lines.push(OBSERVED_NOTE.to_owned());
        for (node, names) in observed {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            lines.push(format!(
                "  %% observed packages {node}: {}",
                names.join(", ")
            ));
        }
    }
    if !contract.maps.is_empty() {
        lines.push(String::new());
        lines.extend(contract.maps.iter().map(|rule| {
            let globs: Vec<&str> = rule.globs.iter().map(MapGlob::as_str).collect();
            format!("  %% map {}: {}", rule.target, globs.join(", "))
        }));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// `%% 3 nodes, 1 edge, 0 cycles; generated by driftwall init`, singular when a count is one.
fn summary_line(nodes: usize, edges: usize, cycles: usize) -> String {
    let counted = |n: usize, noun: &str| {
        if n == 1 {
            format!("{n} {noun}")
        } else {
            format!("{n} {noun}s")
        }
    };
    format!(
        "%% {}, {}, {}; generated by driftwall init",
        counted(nodes, "node"),
        counted(edges, "edge"),
        counted(cycles, "cycle"),
    )
}

/// One line per edge with its arrow, ordered by the rank and id of its source, then of its target.
fn edge_lines(contract: &Contract, rank_of: impl Fn(&NodeId) -> usize) -> Vec<String> {
    let mut edges: Vec<(&Edge, EdgeKind)> = contract
        .edges
        .iter()
        .map(|(edge, kind)| (edge, *kind))
        .collect();
    edges.sort_by_key(|(edge, _)| {
        (
            rank_of(&edge.from),
            edge.from.clone(),
            rank_of(&edge.to),
            edge.to.clone(),
        )
    });
    edges
        .iter()
        .map(|(edge, kind)| format!("  {} {} {}", edge.from, kind.arrow(), edge.to))
        .collect()
}

/// One `%% cycles` comment per component of more than one node: those of the production edges,
/// then those of all edges that are not already one of them.
fn cycle_lines(production: &[Vec<&NodeId>], every: &[Vec<&NodeId>]) -> Vec<String> {
    let join = |members: &[&NodeId]| {
        members
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(CYCLE_JOIN)
    };
    let known: BTreeSet<&[&NodeId]> = production
        .iter()
        .filter(|members| members.len() > 1)
        .map(Vec::as_slice)
        .collect();
    let mut lines: Vec<String> = known
        .iter()
        .map(|members| format!("%% cycles: {}", join(members)))
        .collect();
    lines.extend(
        every
            .iter()
            .filter(|members| members.len() > 1 && !known.contains(members.as_slice()))
            .map(|members| format!("%% cycles (tests only): {}", join(members))),
    );
    lines
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
    use crate::model::{EdgeKind, NodeInfo};

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn contract(nodes: &[(&str, Option<&str>)], edges: &[(&str, &str)]) -> Contract {
        let mut out = Contract::default();
        for (name, boundary) in nodes {
            out.nodes.insert(
                id(name),
                NodeInfo {
                    label: Some(format!("{name}: x")),
                    boundary: boundary.map(str::to_owned),
                    outer: Vec::new(),
                },
            );
        }
        for (from, to) in edges {
            let edge = Edge {
                from: id(from),
                to: id(to),
            };
            out.edges.insert(edge, EdgeKind::Implemented);
        }
        out
    }

    fn body(text: &str) -> Vec<&str> {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("%% driftwall"))
            .collect()
    }

    #[test]
    fn entries_come_first_and_foundations_last() {
        let c = contract(
            &[("model", None), ("cli", None), ("lint", None), ("a", None)],
            &[("cli", "lint"), ("lint", "model"), ("cli", "model")],
        );
        let text = render(&c, &BTreeMap::new());
        let order: Vec<&str> = text
            .lines()
            .filter(|line| line.contains("[\""))
            .map(|line| line.trim().split('[').next().unwrap())
            .collect();
        assert_eq!(order, ["a", "cli", "lint", "model"], "{text}");
        assert_eq!(parse(&text).as_ref(), Ok(&c));
    }

    #[test]
    fn rank_is_the_longest_path_not_the_shortest() {
        let c = contract(
            &[("a", None), ("b", None), ("c", None)],
            &[("a", "b"), ("b", "c"), ("a", "c")],
        );
        let text = render(&c, &BTreeMap::new());
        let edges: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("  ") && line.contains("-->"))
            .collect();
        assert_eq!(edges, ["  a --> b", "  a --> c", "  b --> c"]);
    }

    #[test]
    fn a_cycle_is_one_component_and_is_commented_after_the_edges() {
        let c = contract(
            &[("x", None), ("a", None), ("b", None), ("z", None)],
            &[("x", "a"), ("a", "b"), ("b", "a"), ("b", "z")],
        );
        let text = render(&c, &BTreeMap::new());
        assert!(text.contains("%% 4 nodes, 4 edges, 1 cycle; generated by driftwall init"));
        let last = body(&text).last().copied().unwrap();
        assert_eq!(last, "%% cycles: a <-> b");
        assert_eq!(parse(&text).as_ref(), Ok(&c));
    }

    #[test]
    fn a_cycle_through_test_code_is_named_apart() {
        let c = contract(&[("a", None), ("b", None)], &[("a", "b"), ("b", "a")]);
        let mut c = c;
        c.edges.insert(
            Edge {
                from: id("b"),
                to: id("a"),
            },
            EdgeKind::Test,
        );
        let text = render(&c, &BTreeMap::new());
        assert!(text.contains("  b -->|test| a"), "{text}");
        assert!(text.contains("  %% cycles (tests only): a <-> b"), "{text}");
        assert!(!text.contains("  %% cycles: "), "{text}");
    }

    #[test]
    fn a_boundary_sits_at_its_lowest_ranked_member_and_orders_its_members() {
        let c = contract(
            &[
                ("app", None),
                ("zed", Some("g")),
                ("ant", Some("g")),
                ("core", None),
            ],
            &[("app", "ant"), ("ant", "core"), ("app", "zed")],
        );
        let text = render(&c, &BTreeMap::new());
        let shape: Vec<&str> = body(&text)
            .into_iter()
            .filter(|line| {
                !line.starts_with("%%") && !line.contains("-->") && *line != "flowchart LR"
            })
            .collect();
        assert_eq!(
            shape,
            [
                "app[\"app: x\"]",
                "subgraph g[\"g\"]",
                "ant[\"ant: x\"]",
                "zed[\"zed: x\"]",
                "end",
                "core[\"core: x\"]",
            ]
        );
        assert_eq!(parse(&text).as_ref(), Ok(&c));
    }

    #[test]
    fn output_is_identical_across_runs() {
        let c = contract(
            &[("a", Some("g")), ("b", Some("g")), ("c", None)],
            &[("a", "b"), ("b", "a"), ("c", "a")],
        );
        assert_eq!(render(&c, &BTreeMap::new()), render(&c, &BTreeMap::new()));
    }

    #[test]
    fn map_lines_follow_the_drawing_and_keep_boundaries_and_labels() {
        let mut c = contract(&[("a", Some("g")), ("b", Some("g"))], &[("a", "b")]);
        c.maps.push(crate::model::MapRule {
            line: 0,
            target: crate::model::MapTarget::Node(id("a")),
            globs: vec![MapGlob::parse("src/a/").unwrap()],
        });
        let text = render(&c, &BTreeMap::new());
        assert!(text.contains("subgraph g"), "{text}");
        assert!(text.contains("a[\"a: x\"]"), "{text}");
        assert!(text.ends_with("\n  %% map a: src/a/\n"), "{text}");
        assert_eq!(parse(&text).as_ref(), Ok(&c));
    }
}
