//! Edge templates, `%% each`: parsing the line and expanding it into edges, package rules and
//! denials, see "Edge templates" in `docs/DESIGN.md`.

use std::collections::BTreeMap;

use super::error::DiagramError;
use super::parse::{edge_tail, split_ident, valid_id};
use super::rules::not_a_boundary;
use crate::contract::{
    BoundaryEdge, Denial, EachArrow, EachBody, EachRule, Endpoint, MapRule, NodeId, NodeInfo,
    PackagePattern, PackageRule, PackageRuleKind, Template, child_of, children,
};

/// Reads the text after `each`: `<boundary>: <arrow>, <arrow>`, or one of the rule forms
/// `<boundary>: packages <role>: <name>, ...`, `forbid-packages`, `deny <end>: <end>, ...` and
/// `map <dir>/, ...`.
pub(super) fn parse(rest: &str, line: u32) -> Result<EachRule, DiagramError> {
    let (raw_boundary, list) = rest.split_once(':').ok_or_else(|| {
        malformed(
            line,
            "expected `%% each <boundary>: <role> --> <role>, ...`".to_owned(),
        )
    })?;
    let boundary = valid_id(raw_boundary.trim(), line)?.to_string();
    let body = if let Some(dirs) = map_form(list.trim()) {
        EachBody::Map {
            dirs: map_dirs(dirs, line)?,
        }
    } else if let Some((word, head, tail)) = rule_form(list.trim()) {
        rule_body(word, head, tail, line)?
    } else {
        EachBody::Arrows(arrows(list, line)?)
    };
    Ok(EachRule {
        line,
        boundary,
        body,
    })
}

/// The `each` contract error at `line`.
fn malformed(line: u32, reason: String) -> DiagramError {
    DiagramError::MalformedRule {
        line,
        rule: "each".to_owned(),
        reason,
    }
}

/// The keyword, the end before the second colon and the text after it, when `list` is a rule form:
/// one of the keywords, whitespace, one word and a colon. Anything else is the arrow form, so a
/// role may be called `deny`.
fn rule_form(list: &str) -> Option<(&'static str, &str, &str)> {
    ["packages", "forbid-packages", "deny"]
        .into_iter()
        .find_map(|word| {
            let after = list.strip_prefix(word)?;
            after.starts_with(char::is_whitespace).then_some(())?;
            let (head, tail) = after.split_once(':')?;
            let head = head.trim();
            (!head.is_empty() && split_ident(head) == (head, "")).then_some((word, head, tail))
        })
}

/// The directory list after `map`, when `list` is the map form: the word `map`, whitespace and a
/// first token with a `/`. An id never has one, so a role called `map` still reads as an arrow.
fn map_form(list: &str) -> Option<&str> {
    let after = list.strip_prefix("map")?;
    after.starts_with(char::is_whitespace).then_some(())?;
    let rest = after.trim_start();
    let first = rest.split(|c: char| c.is_whitespace() || c == ',').next()?;
    first.contains('/').then_some(rest)
}

/// The directory patterns of the map form, checked: each ends in `/`, has `{child}` and no other
/// `{...}`, and is a plain directory once the child is filled in.
fn map_dirs(list: &str, line: u32) -> Result<Vec<String>, DiagramError> {
    let mut dirs: Vec<String> = Vec::new();
    for piece in list.split(',') {
        let dir = piece.trim();
        let bad = |reason: &str| malformed(line, format!("`{dir}`: {reason}"));
        if dir.is_empty() {
            return Err(malformed(
                line,
                "expected `%% each <boundary>: map <dir>/[, <dir>/]`".to_owned(),
            ));
        }
        MapRule::check_pattern(dir).map_err(|reason| bad(&reason))?;
        if dirs.iter().any(|listed| listed == dir) {
            return Err(bad("listed twice"));
        }
        dirs.push(dir.to_owned());
    }
    Ok(dirs)
}

/// The body of a rule form: the role or end `head` and the list `tail` after its colon.
fn rule_body(word: &str, head: &str, tail: &str, line: u32) -> Result<EachBody, DiagramError> {
    let head = valid_id(head, line)?.to_string();
    let pieces: Vec<&str> = if tail.trim().is_empty() {
        Vec::new()
    } else {
        tail.split(',').collect()
    };
    if word == "deny" {
        if pieces.is_empty() {
            return Err(malformed(
                line,
                "expected `%% each <boundary>: deny <end>: <end>, <end>`".to_owned(),
            ));
        }
        let mut to: Vec<String> = Vec::new();
        for piece in pieces {
            let end = valid_id(piece.trim(), line)?.to_string();
            if end == head {
                return Err(malformed(
                    line,
                    format!("`deny {head}: {end}` joins an end to itself"),
                ));
            }
            if to.contains(&end) {
                return Err(malformed(
                    line,
                    format!("`{end}` is listed twice in `deny {head}`"),
                ));
            }
            to.push(end);
        }
        return Ok(EachBody::Deny { from: head, to });
    }
    let kind = if word == "packages" {
        PackageRuleKind::Allow
    } else {
        PackageRuleKind::Forbid
    };
    if pieces.is_empty() && kind == PackageRuleKind::Forbid {
        return Err(malformed(line, format!("`{word}` needs at least one name")));
    }
    let mut names: Vec<PackagePattern> = Vec::new();
    for piece in pieces {
        let name =
            PackagePattern::parse(piece).map_err(|reason| malformed(line, reason.to_owned()))?;
        if names.contains(&name) {
            return Err(malformed(
                line,
                format!("`{name}` is listed twice in `{word} {head}`"),
            ));
        }
        names.push(name);
    }
    Ok(EachBody::Packages {
        kind,
        role: head,
        names,
    })
}

/// The arrows of the list `list`.
fn arrows(list: &str, line: u32) -> Result<Vec<EachArrow>, DiagramError> {
    let mut arrows: Vec<EachArrow> = Vec::new();
    for piece in split_arrows(list) {
        let piece = piece.trim();
        let arrow = arrow(piece, line).map_err(|error| match error {
            DiagramError::Unsupported { statement, .. } => malformed(
                line,
                format!("`{statement}` is not `<role> <arrow> <role>`"),
            ),
            other => other,
        })?;
        if arrow.from == arrow.to {
            return Err(malformed(
                line,
                format!("`{piece}` joins a role to itself: a node has no edge to itself"),
            ));
        }
        if arrows
            .iter()
            .any(|listed| (&listed.from, &listed.to) == (&arrow.from, &arrow.to))
        {
            return Err(malformed(
                line,
                format!(
                    "`{} ... {}` is listed twice: a pair has at most one edge",
                    arrow.from, arrow.to
                ),
            ));
        }
        arrows.push(arrow);
    }
    Ok(arrows)
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

/// What the templates declare: edges, package rules and denials, each for every child that has
/// the roles.
pub(super) struct Expanded {
    /// The edges, boundary edges for every rule.
    pub edges: Vec<BoundaryEdge>,
    /// The package rules, in the order of the lines.
    pub packages: Vec<PackageRule>,
    /// The denials, in the order of the lines.
    pub deny: Vec<Denial>,
    /// The module-form map rules of the `map` lines, in the order of the lines.
    pub maps: Vec<MapRule>,
}

/// The template that an expansion of `rule` for `child` comes from, with `arrow` as its text.
fn template_of(
    rule: &EachRule,
    child: &str,
    ends: (bool, bool),
    (from, to): (&str, &str),
    arrow: String,
) -> Template {
    Template {
        boundary: rule.boundary.clone(),
        child: child.to_owned(),
        from_role: ends.0,
        to_role: ends.1,
        from: from.to_owned(),
        to: to.to_owned(),
        arrow,
    }
}

/// What every template declares: each arrow, package rule and denial for each child boundary that
/// has the nodes it needs.
///
/// # Errors
/// A contract error for a boundary that is not one or has no child boundary, a role no child has,
/// and two expansions of one pair of nodes.
pub(super) fn expand(
    templates: &[EachRule],
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
) -> Result<Expanded, DiagramError> {
    let mut expanded = Expanded {
        edges: Vec::new(),
        packages: Vec::new(),
        deny: Vec::new(),
        maps: Vec::new(),
    };
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
            return Err(malformed(
                line,
                format!(
                    "`{}` has no child boundary to apply the arrows to",
                    rule.boundary
                ),
            ));
        }
        match &rule.body {
            EachBody::Arrows(arrows) => {
                expand_arrows(rule, arrows, &kids, nodes, boundaries, &mut expanded.edges)?;
            }
            EachBody::Packages { kind, role, names } => {
                if !kids.iter().any(|kid| resolve(nodes, kid, role).is_some()) {
                    return Err(no_child_has(rule, role, &kids));
                }
                for kid in &kids {
                    let Some(node) = resolve(nodes, kid, role) else {
                        continue;
                    };
                    let arrow = format!("{} {role}", kind.directive());
                    expanded.packages.push(PackageRule {
                        line,
                        node,
                        kind: *kind,
                        names: names.clone(),
                        template: Some(template_of(rule, kid, (true, false), (role, ""), arrow)),
                    });
                }
            }
            EachBody::Map { dirs } => expand_maps(rule, dirs, &kids, &mut expanded.maps)?,
            EachBody::Deny { from, to } => {
                for target in to {
                    let text = format!("deny {from}: {target}");
                    let ends = classify(
                        rule,
                        (from, target),
                        (&text, "write it as an ordinary `%% deny`"),
                        &kids,
                        nodes,
                        boundaries,
                    )?;
                    for kid in &kids {
                        let (Some(source), Some(sink)) = (
                            ends.0.expand(nodes, kid, from),
                            ends.1.expand(nodes, kid, target),
                        ) else {
                            continue;
                        };
                        let flags = (matches!(ends.0, End::Role), matches!(ends.1, End::Role));
                        expanded.deny.push(Denial {
                            line,
                            from: source,
                            to: sink,
                            template: Some(template_of(
                                rule,
                                kid,
                                flags,
                                (from, target),
                                text.clone(),
                            )),
                        });
                    }
                }
            }
        }
    }
    Ok(expanded)
}

/// The module-form map rules of the map form of `rule`: one per child and directory pattern.
fn expand_maps(
    rule: &EachRule,
    dirs: &[String],
    kids: &[String],
    maps: &mut Vec<MapRule>,
) -> Result<(), DiagramError> {
    for kid in kids {
        for dir in dirs {
            maps.push(
                MapRule::expansion(rule.line, &rule.boundary, kid, dir)
                    .map_err(|reason| malformed(rule.line, reason))?,
            );
        }
    }
    Ok(())
}

/// The error for a role that no child of the template's boundary has.
fn no_child_has(template: &EachRule, name: &str, children: &[String]) -> DiagramError {
    malformed(
        template.line,
        format!(
            "role `{name}` resolves in no child of `{}`: none of {} has a node `<child>-{name}`",
            template.boundary,
            children.join(", "),
        ),
    )
}

/// The edges of the arrows of `rule`, appended to `edges`.
fn expand_arrows(
    rule: &EachRule,
    arrows: &[EachArrow],
    kids: &[String],
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
    edges: &mut Vec<BoundaryEdge>,
) -> Result<(), DiagramError> {
    let line = rule.line;
    let ends = arrows
        .iter()
        .map(|arrow| {
            classify(
                rule,
                (&arrow.from, &arrow.to),
                (&arrow.text(), "write it as an ordinary edge"),
                kids,
                nodes,
                boundaries,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (arrow, ends_of) in arrows.iter().zip(ends) {
        let (from, to) = &ends_of;
        for kid in kids {
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
            let flags = (
                matches!(ends_of.0, End::Role),
                matches!(ends_of.1, End::Role),
            );
            let roles = (arrow.from.as_str(), arrow.to.as_str());
            edges.push(BoundaryEdge {
                from,
                to,
                kind: arrow.kind,
                line,
                template: Some(template_of(rule, kid, flags, roles, arrow.text())),
            });
        }
    }
    Ok(())
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

/// Reads the two names `(from, to)` of an arrow or a denial as role or fixed end, with the contract
/// errors of "Edge templates". `shown` is the text the errors name and the advice for two fixed
/// ends.
fn classify(
    rule: &EachRule,
    (from, to): (&str, &str),
    (shown, advice): (&str, &str),
    kids: &[String],
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
) -> Result<(End, End), DiagramError> {
    let mut ends = Vec::new();
    for name in [from, to] {
        let resolves = kids.iter().any(|kid| resolve(nodes, kid, name).is_some());
        let declared = NodeId::new(name)
            .ok()
            .filter(|id| nodes.contains_key(id))
            .map(Endpoint::Node)
            .or_else(|| {
                boundaries
                    .contains_key(name)
                    .then(|| Endpoint::Boundary(name.to_owned()))
            });
        let Some(fixed) = declared else {
            // why: a role no child has is a typo, while a role some child lacks is an optional part.
            if !resolves {
                return Err(no_child_has(rule, name, kids));
            }
            ends.push(End::Role);
            continue;
        };
        // why: 0.2.0 read every name as a role, so a name that is a role of some child stays one.
        if resolves {
            ends.push(End::Role);
            continue;
        }
        check_fixed(rule, name, &fixed, nodes).map_err(|reason| malformed(rule.line, reason))?;
        ends.push(End::Fixed(fixed));
    }
    match (ends.pop(), ends.pop()) {
        (Some(End::Fixed(_)), Some(End::Fixed(_))) => Err(malformed(
            rule.line,
            format!("`{shown}` joins two declared ids, `{from}` and `{to}`: {advice}"),
        )),
        (Some(to), Some(from)) => Ok((from, to)),
        _ => Err(malformed(rule.line, "an arrow has two ends".to_owned())),
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
