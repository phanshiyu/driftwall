//! The rule directives of a contract: `%% order:`, and the others of "Contract rules".

use std::collections::{BTreeMap, BTreeSet};

use super::cycles::find_cycle;
use super::error::DiagramError;
use crate::model::{Edge, EdgeKind, LayerOrder, NodeId, NodeInfo, Rules};

/// A rule directive found on a comment line, with the text after its keyword.
pub(super) enum Directive<'a> {
    /// `%% order: <boundary> > <boundary>`
    Order(&'a str),
    /// `%% no-cycles` or `%% no-cycles: boundaries`: the text after the keyword.
    NoCycles(&'a str),
}

/// The rule directive a comment line is, or `None` for anything else.
///
/// The keyword is the first word of the comment and the colon of `order:` must follow it directly,
/// so a comment such as `%% orders of magnitude` stays plain.
pub(super) fn directive(text: &str) -> Option<Directive<'_>> {
    let body = text.strip_prefix("%%")?.trim_start();
    if let Some(rest) = body.strip_prefix("order:") {
        return Some(Directive::Order(rest.trim()));
    }
    let rest = body.strip_prefix("no-cycles")?;
    (rest.is_empty() || rest.starts_with(|c: char| c == ':' || c.is_whitespace()))
        .then(|| Directive::NoCycles(rest.trim()))
}

/// The directive lines that write `rules` back, in the order of "Contract rules".
pub(super) fn render(rules: &Rules) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(order) = &rules.order {
        lines.push(format!("  %% order: {}", order.boundaries.join(" > ")));
    }
    if rules.no_cycles.is_some() {
        lines.push("  %% no-cycles".to_owned());
    }
    if rules.no_cycles_boundaries.is_some() {
        lines.push("  %% no-cycles: boundaries".to_owned());
    }
    lines
}

/// What the parser has read of the rule directives, checked against the contract by [`finish`].
#[derive(Default)]
pub(super) struct RuleParser {
    order: Option<LayerOrder>,
    no_cycles: Option<u32>,
    no_cycles_boundaries: Option<u32>,
}

impl RuleParser {
    /// Records `found`, which is on `line`.
    pub(super) fn record(&mut self, found: &Directive<'_>, line: u32) -> Result<(), DiagramError> {
        match *found {
            Directive::Order(rest) => self.order(rest, line),
            Directive::NoCycles(rest) => self.no_cycles(rest, line),
        }
    }

    fn no_cycles(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let (slot, rule) = match rest.strip_prefix(':').map(str::trim) {
            None if rest.is_empty() => (&mut self.no_cycles, "no-cycles"),
            Some("boundaries") => (&mut self.no_cycles_boundaries, "no-cycles: boundaries"),
            _ => {
                return Err(DiagramError::MalformedRule {
                    line,
                    rule: "no-cycles".to_owned(),
                    reason: "expected `%% no-cycles` or `%% no-cycles: boundaries`".to_owned(),
                });
            }
        };
        if let Some(first_line) = *slot {
            return Err(DiagramError::DuplicateRule {
                line,
                rule: rule.to_owned(),
                first_line,
            });
        }
        *slot = Some(line);
        Ok(())
    }

    fn order(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let malformed = |reason: &str| DiagramError::MalformedRule {
            line,
            rule: "order".to_owned(),
            reason: reason.to_owned(),
        };
        if let Some(first) = &self.order {
            return Err(DiagramError::DuplicateRule {
                line,
                rule: "order".to_owned(),
                first_line: first.line,
            });
        }
        let mut boundaries: Vec<String> = Vec::new();
        for raw in rest.split('>') {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(malformed("expected `<boundary> > <boundary> > ...`"));
            }
            let id = NodeId::new(raw).map_err(|source| DiagramError::InvalidId { line, source })?;
            if boundaries.iter().any(|listed| listed == id.as_str()) {
                return Err(malformed(&format!("`{id}` is listed twice")));
            }
            boundaries.push(id.to_string());
        }
        if boundaries.len() < 2 {
            return Err(malformed("a layer order lists at least two boundaries"));
        }
        self.order = Some(LayerOrder { line, boundaries });
        Ok(())
    }

    /// Checks the directives against the declared nodes, boundaries and edges, and returns them.
    ///
    /// `edge_lines` is the line of every declared edge.
    pub(super) fn finish(
        self,
        nodes: &BTreeMap<NodeId, NodeInfo>,
        boundaries: &BTreeMap<String, u32>,
        edges: &BTreeMap<Edge, EdgeKind>,
        edge_lines: &BTreeMap<Edge, u32>,
    ) -> Result<Rules, DiagramError> {
        if let Some(order) = &self.order {
            check_order(order, nodes, boundaries, edges, edge_lines)?;
        }
        if let Some(line) = self.no_cycles {
            check_node_cycles(line, nodes, edges, edge_lines)?;
        }
        if let Some(line) = self.no_cycles_boundaries {
            check_boundary_cycles(line, nodes, edges)?;
        }
        Ok(Rules {
            order: self.order,
            no_cycles: self.no_cycles,
            no_cycles_boundaries: self.no_cycles_boundaries,
        })
    }
}

/// The boundaries of a layer order must be declared `subgraph`s, and no declared arrow may point
/// upward, test edges excepted.
fn check_order(
    order: &LayerOrder,
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
    edges: &BTreeMap<Edge, EdgeKind>,
    edge_lines: &BTreeMap<Edge, u32>,
) -> Result<(), DiagramError> {
    for listed in &order.boundaries {
        if boundaries.contains_key(listed) {
            continue;
        }
        let is_node = nodes.keys().any(|id| id.as_str() == listed);
        let reason = if is_node {
            format!("`{listed}` is a node, not a boundary")
        } else {
            format!("`{listed}` is not a declared boundary")
        };
        return Err(DiagramError::MalformedRule {
            line: order.line,
            rule: "order".to_owned(),
            reason,
        });
    }
    let rank_of = |id: &NodeId| {
        nodes
            .get(id)
            .and_then(|info| info.boundary.as_deref())
            .and_then(|boundary| order.rank(boundary).map(|rank| (boundary, rank)))
    };
    let upward = edges
        .iter()
        .filter(|(_, kind)| !matches!(kind, EdgeKind::Test | EdgeKind::PlannedTest))
        .filter_map(|(edge, _)| {
            let (from_boundary, from_rank) = rank_of(&edge.from)?;
            let (to_boundary, to_rank) = rank_of(&edge.to)?;
            (from_rank > to_rank).then_some((edge, from_boundary, to_boundary))
        })
        .min_by_key(|(edge, _, _)| edge_lines.get(*edge).copied().unwrap_or(u32::MAX));
    match upward {
        Some((edge, from_boundary, to_boundary)) => Err(DiagramError::UpwardArrow {
            line: edge_lines.get(edge).copied().unwrap_or(order.line),
            from: edge.from.clone(),
            to: edge.to.clone(),
            relation: format!("`{from_boundary}` is below `{to_boundary}`"),
            order_line: order.line,
        }),
        None => Ok(()),
    }
}

/// Whether `kind` counts for `%% no-cycles`: every edge but a pure test edge.
const fn counts_for_cycles(kind: EdgeKind) -> bool {
    !matches!(kind, EdgeKind::Test | EdgeKind::PlannedTest)
}

/// The declared edges, test edges excluded, must have no cycle between nodes.
fn check_node_cycles(
    line: u32,
    nodes: &BTreeMap<NodeId, NodeInfo>,
    edges: &BTreeMap<Edge, EdgeKind>,
    edge_lines: &BTreeMap<Edge, u32>,
) -> Result<(), DiagramError> {
    let mut graph: BTreeMap<String, BTreeSet<String>> = nodes
        .keys()
        .map(|id| (id.to_string(), BTreeSet::new()))
        .collect();
    for (edge, _) in edges.iter().filter(|(_, kind)| counts_for_cycles(**kind)) {
        graph
            .entry(edge.from.to_string())
            .or_default()
            .insert(edge.to.to_string());
    }
    let Some(cycle) = find_cycle(&graph) else {
        return Ok(());
    };
    let described: Vec<String> = cycle
        .iter()
        .zip(cycle.iter().cycle().skip(1))
        .map(|(from, to)| {
            let at = NodeId::new(from)
                .ok()
                .zip(NodeId::new(to).ok())
                .and_then(|(from, to)| edge_lines.get(&Edge { from, to }));
            at.map_or_else(
                || format!("{from} --> {to}"),
                |at| format!("{from} --> {to} (line {at})"),
            )
        })
        .collect();
    Err(DiagramError::DeclaredCycle {
        line,
        what: "node".to_owned(),
        cycle: described.join(", "),
    })
}

/// The boundary graph of the declared edges, test edges excluded, must have no cycle.
fn check_boundary_cycles(
    line: u32,
    nodes: &BTreeMap<NodeId, NodeInfo>,
    edges: &BTreeMap<Edge, EdgeKind>,
) -> Result<(), DiagramError> {
    let boundary = |id: &NodeId| nodes.get(id)?.boundary.clone();
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for info in nodes.values() {
        if let Some(name) = &info.boundary {
            graph.entry(name.clone()).or_default();
        }
    }
    for (edge, _) in edges.iter().filter(|(_, kind)| counts_for_cycles(**kind)) {
        if let (Some(from), Some(to)) = (boundary(&edge.from), boundary(&edge.to))
            && from != to
        {
            graph.entry(from).or_default().insert(to);
        }
    }
    let Some(cycle) = find_cycle(&graph) else {
        return Ok(());
    };
    let mut path = cycle.clone();
    path.extend(cycle.first().cloned());
    Err(DiagramError::DeclaredCycle {
        line,
        what: "boundary".to_owned(),
        cycle: path.join(" --> "),
    })
}
