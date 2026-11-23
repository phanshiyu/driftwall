//! The rule directives of a contract: `%% order:`, and the others of "Contract rules".

use std::collections::BTreeMap;

use super::error::DiagramError;
use crate::model::{Edge, EdgeKind, LayerOrder, NodeId, NodeInfo, Rules};

/// A rule directive found on a comment line, with the text after its keyword.
pub(super) enum Directive<'a> {
    /// `%% order: <boundary> > <boundary>`
    Order(&'a str),
}

/// The rule directive a comment line is, or `None` for anything else.
///
/// The keyword is the first word of the comment and the colon of `order:` must follow it directly,
/// so a comment such as `%% orders of magnitude` stays plain.
pub(super) fn directive(text: &str) -> Option<Directive<'_>> {
    let body = text.strip_prefix("%%")?.trim_start();
    body.strip_prefix("order:")
        .map(|rest| Directive::Order(rest.trim()))
}

/// The directive lines that write `rules` back, in the order of "Contract rules".
pub(super) fn render(rules: &Rules) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(order) = &rules.order {
        lines.push(format!("  %% order: {}", order.boundaries.join(" > ")));
    }
    lines
}

/// What the parser has read of the rule directives, checked against the contract by [`finish`].
#[derive(Default)]
pub(super) struct RuleParser {
    order: Option<LayerOrder>,
}

impl RuleParser {
    /// Records `found`, which is on `line`.
    pub(super) fn record(&mut self, found: &Directive<'_>, line: u32) -> Result<(), DiagramError> {
        match *found {
            Directive::Order(rest) => self.order(rest, line),
        }
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
        Ok(Rules { order: self.order })
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
