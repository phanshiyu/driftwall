//! Edge templates, `%% each`: parsing the line and expanding it into edges, see "Edge templates" in
//! `docs/DESIGN.md`.

use std::collections::BTreeMap;

use super::error::DiagramError;
use super::parse::{edge_tail, split_ident, valid_id};
use super::rules::not_a_boundary;
use crate::contract::{
    BoundaryEdge, EachArrow, EachRule, Endpoint, NodeId, NodeInfo, Template, child_of, children,
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
        let ends = rule
            .arrows
            .iter()
            .map(|arrow| classify(rule, arrow, &kids, nodes, boundaries))
            .collect::<Result<Vec<_>, _>>()?;
        for (arrow, ends_of) in rule.arrows.iter().zip(ends) {
            let (from, to) = &ends_of;
            for kid in &kids {
                let (Some(from), Some(to)) = (
                    from.expand(nodes, kid, &arrow.from),
                    to.expand(nodes, kid, &arrow.to),
                ) else {
                    continue;
                };
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
                        child: kid.clone(),
                        from_role: matches!(ends_of.0, End::Role),
                        to_role: matches!(ends_of.1, End::Role),
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

/// One end of a template arrow once read against the contract.
enum End {
    /// A role, resolved inside each child.
    Role,
    /// An id the contract declares, the same for every child.
    Fixed(Endpoint),
}

impl End {
    /// The end of the expansion for `child`, or `None` when the child lacks the role.
    fn expand(
        &self,
        nodes: &BTreeMap<NodeId, NodeInfo>,
        child: &str,
        role: &str,
    ) -> Option<Endpoint> {
        match self {
            Self::Role => resolve(nodes, child, role).map(Endpoint::Node),
            Self::Fixed(end) => Some(end.clone()),
        }
    }
}

/// Reads both ends of `arrow` as role or fixed end, with the contract errors of "Edge templates".
fn classify(
    rule: &EachRule,
    arrow: &EachArrow,
    kids: &[String],
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
) -> Result<(End, End), DiagramError> {
    let malformed = |reason: String| DiagramError::MalformedRule {
        line: rule.line,
        rule: "each".to_owned(),
        reason,
    };
    let mut ends = Vec::new();
    for name in [&arrow.from, &arrow.to] {
        let resolves = kids.iter().any(|kid| resolve(nodes, kid, name).is_some());
        let declared = NodeId::new(name)
            .ok()
            .filter(|id| nodes.contains_key(id))
            .map(Endpoint::Node)
            .or_else(|| {
                boundaries
                    .contains_key(name)
                    .then(|| Endpoint::Boundary(name.clone()))
            });
        let Some(fixed) = declared else {
            // why: a role no child has is a typo, while a role some child lacks is an optional part.
            if !resolves {
                return Err(malformed(format!(
                    "role `{name}` resolves in no child of `{}`: none of {} has a node `<child>-{name}`",
                    rule.boundary,
                    kids.join(", "),
                )));
            }
            ends.push(End::Role);
            continue;
        };
        // why: 0.2.0 read every name as a role, so a name that is a role of some child stays one.
        if resolves {
            ends.push(End::Role);
            continue;
        }
        check_fixed(rule, name, &fixed, nodes).map_err(malformed)?;
        ends.push(End::Fixed(fixed));
    }
    match (ends.pop(), ends.pop()) {
        (Some(End::Fixed(_)), Some(End::Fixed(_))) => Err(malformed(format!(
            "`{}` joins two declared ids, `{}` and `{}`: write it as an ordinary edge",
            arrow.text(),
            arrow.from,
            arrow.to
        ))),
        (Some(to), Some(from)) => Ok((from, to)),
        _ => Err(malformed("an arrow has two ends".to_owned())),
    }
}

/// Why `fixed` cannot be the fixed end of a template over `rule.boundary`, if it cannot.
fn check_fixed(
    rule: &EachRule,
    name: &str,
    fixed: &Endpoint,
    nodes: &BTreeMap<NodeId, NodeInfo>,
) -> Result<(), String> {
    let boundary = rule.boundary.as_str();
    if name == boundary {
        return Err(format!(
            "`{name}` is the boundary of the template: an arrow cannot end at it"
        ));
    }
    let inside = |child: &str| {
        format!(
            "`{name}` is inside `{child}`, a child of `{boundary}`: a fixed end lies outside the children"
        )
    };
    match fixed {
        Endpoint::Node(id) => {
            let info = nodes
                .get(id)
                .ok_or_else(|| format!("`{name}` is not declared"))?;
            if let Some(child) = child_of(id, info, boundary)
                && child != id.as_str()
            {
                return Err(inside(child));
            }
        }
        Endpoint::Boundary(id) => {
            let mut members = nodes.values().filter(|info| info.within(id)).peekable();
            if members.peek().is_none() {
                return Err(format!(
                    "boundary `{id}` declares no node, so an arrow to or from it covers nothing"
                ));
            }
            for info in members {
                let mut chain = info.boundaries().skip_while(|b| *b != boundary).skip(1);
                if let Some(child) = chain.next()
                    && child != id
                {
                    return Err(inside(child));
                }
            }
        }
    }
    Ok(())
}
