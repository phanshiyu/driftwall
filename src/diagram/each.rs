//! Edge templates, `%% each`: parsing the line and expanding it into edges, see "Edge templates" in
//! `docs/DESIGN.md`.

use std::collections::BTreeMap;

use super::error::DiagramError;
use super::parse::{edge_tail, split_ident, valid_id};
use super::rules::not_a_boundary;
use crate::model::{
    BoundaryEdge, EachArrow, EachRule, Endpoint, NodeId, NodeInfo, Template, children,
};

/// Reads the text after `each`: `<boundary>: <arrow>, <arrow>`.
pub(super) fn parse(rest: &str, line: u32) -> Result<EachRule, DiagramError> {
    let malformed = |reason: String| DiagramError::MalformedRule {
        line,
        rule: "each".to_owned(),
        reason,
    };
    let (raw_boundary, list) = rest.split_once(':').ok_or_else(|| {
        malformed("expected `%% each <boundary>: <role> --> <role>, ...`".to_owned())
    })?;
    let boundary = valid_id(raw_boundary.trim(), line)?.to_string();
    let mut arrows: Vec<EachArrow> = Vec::new();
    for piece in split_arrows(list) {
        let piece = piece.trim();
        let arrow = arrow(piece, line).map_err(|error| match error {
            DiagramError::Unsupported { statement, .. } => {
                malformed(format!("`{statement}` is not `<role> <arrow> <role>`"))
            }
            other => other,
        })?;
        if arrow.from == arrow.to {
            return Err(malformed(format!(
                "`{piece}` joins a role to itself: a node has no edge to itself"
            )));
        }
        if arrows
            .iter()
            .any(|listed| (&listed.from, &listed.to) == (&arrow.from, &arrow.to))
        {
            return Err(malformed(format!(
                "`{} ... {}` is listed twice: a pair has at most one edge",
                arrow.from, arrow.to
            )));
        }
        arrows.push(arrow);
    }
    Ok(EachRule {
        line,
        boundary,
        arrows,
    })
}

/// One arrow `piece`, read with the code that reads the arrow of an edge.
fn arrow(piece: &str, line: u32) -> Result<EachArrow, DiagramError> {
    let (raw_from, rest) = split_ident(piece);
    if raw_from.is_empty() {
        return Err(DiagramError::Unsupported {
            line,
            statement: piece.to_owned(),
        });
    }
    let (kind, raw_to) = edge_tail(rest.trim_start(), piece, line)?;
    Ok(EachArrow {
        from: valid_id(raw_from, line)?.to_string(),
        to: valid_id(raw_to, line)?.to_string(),
        kind,
    })
}

/// Splits the arrow list at the commas outside `|...|`, because the label `type, test` has one.
fn split_arrows(list: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut in_label = false;
    for (at, c) in list.char_indices() {
        match c {
            '|' => in_label = !in_label,
            ',' if !in_label => {
                pieces.push(list.get(start..at).unwrap_or_default());
                start = at + 1;
            }
            _ => {}
        }
    }
    pieces.push(list.get(start..).unwrap_or_default());
    pieces
}

/// The node `child`-`role`, when the contract declares it inside `child`.
fn resolve(nodes: &BTreeMap<NodeId, NodeInfo>, child: &str, role: &str) -> Option<NodeId> {
    let id = NodeId::new(&format!("{child}-{role}")).ok()?;
    nodes
        .get(&id)
        .is_some_and(|info| info.within(child))
        .then_some(id)
}

/// The edges every template declares: each arrow for each child boundary that has both its nodes.
///
/// # Errors
/// A contract error for a boundary that is not one or has no child boundary, a role no child has,
/// and two expansions of one pair of nodes.
pub(super) fn expand(
    templates: &[EachRule],
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
) -> Result<Vec<BoundaryEdge>, DiagramError> {
    let mut edges: Vec<BoundaryEdge> = Vec::new();
    for rule in templates {
        let line = rule.line;
        if !boundaries.contains_key(&rule.boundary) {
            return Err(not_a_boundary(line, "each", &rule.boundary, nodes));
        }
        let kids: Vec<String> = children(nodes, &rule.boundary)
            .into_iter()
            .filter(|child| boundaries.contains_key(child))
            .collect();
        if kids.is_empty() {
            return Err(DiagramError::MalformedRule {
                line,
                rule: "each".to_owned(),
                reason: format!(
                    "`{}` has no child boundary to apply the arrows to",
                    rule.boundary
                ),
            });
        }
        // why: a role no child has is a typo, while a role some child lacks is an optional part.
        for role in rule.arrows.iter().flat_map(|a| [&a.from, &a.to]) {
            if !kids.iter().any(|kid| resolve(nodes, kid, role).is_some()) {
                return Err(DiagramError::MalformedRule {
                    line,
                    rule: "each".to_owned(),
                    reason: format!(
                        "role `{role}` resolves in no child of `{}`: none of {} has a node `<child>-{role}`",
                        rule.boundary,
                        kids.join(", "),
                    ),
                });
            }
        }
        for arrow in &rule.arrows {
            for kid in &kids {
                let (Some(from), Some(to)) = (
                    resolve(nodes, kid, &arrow.from),
                    resolve(nodes, kid, &arrow.to),
                ) else {
                    continue;
                };
                let (from, to) = (Endpoint::Node(from), Endpoint::Node(to));
                if let Some(first) = edges.iter().find(|e| e.from == from && e.to == to) {
                    return Err(DiagramError::DuplicateEdge {
                        line,
                        from,
                        to,
                        first_line: first.line,
                    });
                }
                edges.push(BoundaryEdge {
                    from,
                    to,
                    kind: arrow.kind,
                    line,
                    template: Some(Template {
                        boundary: rule.boundary.clone(),
                        from: arrow.from.clone(),
                        to: arrow.to.clone(),
                        arrow: arrow.text(),
                    }),
                });
            }
        }
    }
    Ok(edges)
}
