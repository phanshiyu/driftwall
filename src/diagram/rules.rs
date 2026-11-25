//! The rule directives of a contract: `%% order:`, and the others of "Contract rules".

use std::collections::{BTreeMap, BTreeSet};

use super::cycles::find_cycle;
use super::each;
use super::error::DiagramError;
use crate::model::{
    BoundaryEdge, Budget, Contract, Denial, EachArrow, EachRule, Edge, EdgeKind, Endpoint,
    LayerOrder, NodeId, NodeInfo, OpenAccess, Rules, children_apart,
};

/// A rule directive found on a comment line, with the text after its keyword.
pub(super) enum Directive<'a> {
    /// `%% order: <boundary> > <boundary>`
    Order(&'a str),
    /// `%% order open: <boundary> > <boundary>`
    OpenOrder(&'a str),
    /// `%% isolated: <boundary>, <boundary>`: the text after the colon.
    Isolated(&'a str),
    /// `%% no-cycles` or `%% no-cycles: boundaries`: the text after the keyword.
    NoCycles(&'a str),
    /// `%% budget <node>: fan-out <n>, ...`: the text after the keyword.
    Budget(&'a str),
    /// `%% tests: isolated`: the text after the colon.
    Tests(&'a str),
    /// `%% facade <node>, <node>`: the text after the keyword.
    Facade(&'a str),
    /// `%% each <boundary>: <arrow>, <arrow>`: the text after the keyword.
    Each(&'a str),
    /// `%% deny <from>: <to>, <to>`: the text after the keyword.
    Deny(&'a str),
}

/// The rule directive a comment line is, or `None` for anything else.
///
/// The keyword is the first word of the comment and the colon of `order:` must follow it directly,
/// so a comment such as `%% orders of magnitude` stays plain.
pub(super) fn directive(text: &str) -> Option<Directive<'_>> {
    let body = text.strip_prefix("%%")?.trim_start();
    if let Some(rest) = body.strip_prefix("order open:") {
        return Some(Directive::OpenOrder(rest.trim()));
    }
    if let Some(rest) = body.strip_prefix("order:") {
        return Some(Directive::Order(rest.trim()));
    }
    if let Some(rest) = body.strip_prefix("isolated:") {
        return Some(Directive::Isolated(rest.trim()));
    }
    if let Some(rest) = body.strip_prefix("tests:") {
        return Some(Directive::Tests(rest.trim()));
    }
    if let Some(rest) = body.strip_prefix("facade")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return Some(Directive::Facade(rest.trim()));
    }
    if let Some(rest) = body.strip_prefix("each")
        && rest.starts_with(char::is_whitespace)
    {
        return Some(Directive::Each(rest.trim()));
    }
    if let Some(rest) = body.strip_prefix("deny")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return Some(Directive::Deny(rest.trim()));
    }
    if let Some(rest) = body.strip_prefix("budget")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return Some(Directive::Budget(rest.trim()));
    }
    let rest = body.strip_prefix("no-cycles")?;
    (rest.is_empty() || rest.starts_with(|c: char| c == ':' || c.is_whitespace()))
        .then(|| Directive::NoCycles(rest.trim()))
}

/// The directive lines that write `rules` back, in the order of "Contract rules".
pub(super) fn render(rules: &Rules) -> Vec<String> {
    let mut lines = Vec::new();
    for order in &rules.order {
        let open = if order.open { " open" } else { "" };
        lines.push(format!(
            "  %% order{open}: {}",
            order.boundaries.join(" > ")
        ));
    }
    if !rules.isolated.is_empty() {
        let listed: Vec<&str> = rules.isolated.keys().map(String::as_str).collect();
        lines.push(format!("  %% isolated: {}", listed.join(", ")));
    }
    if rules.no_cycles.is_some() {
        lines.push("  %% no-cycles".to_owned());
    }
    if rules.no_cycles_boundaries.is_some() {
        lines.push("  %% no-cycles: boundaries".to_owned());
    }
    if rules.tests_isolated.is_some() {
        lines.push("  %% tests: isolated".to_owned());
    }
    if !rules.facades.is_empty() {
        let nodes: Vec<String> = rules.facades.keys().map(ToString::to_string).collect();
        lines.push(format!("  %% facade {}", nodes.join(", ")));
    }
    for rule in &rules.each {
        let arrows: Vec<String> = rule.arrows.iter().map(EachArrow::text).collect();
        lines.push(format!(
            "  %% each {}: {}",
            rule.boundary,
            arrows.join(", ")
        ));
    }
    for denial in &rules.deny {
        lines.push(format!("  {}", denial.text()));
    }
    for (node, budget) in &rules.budgets {
        let limits: Vec<String> = [
            ("fan-out", budget.fan_out),
            ("files", budget.files),
            ("lines", budget.lines),
            ("interface", budget.interface),
        ]
        .into_iter()
        .filter_map(|(name, limit)| Some(format!("{name} {}", limit?)))
        .collect();
        lines.push(format!("  %% budget {node}: {}", limits.join(", ")));
    }
    lines
}

/// What the parser has read of the rule directives, checked against the contract by [`finish`].
#[derive(Default)]
pub(super) struct RuleParser {
    order: Vec<LayerOrder>,
    no_cycles: Option<u32>,
    no_cycles_boundaries: Option<u32>,
    budgets: BTreeMap<NodeId, Budget>,
    tests_isolated: Option<u32>,
    isolated: BTreeMap<String, u32>,
    facades: BTreeMap<NodeId, u32>,
    each: Vec<EachRule>,
    deny: Vec<PendingDenial>,
}

/// A `%% deny` pair whose ids are not yet checked against the declared nodes and boundaries.
struct PendingDenial {
    line: u32,
    from: NodeId,
    to: NodeId,
}

impl RuleParser {
    /// The `%% each` templates read so far, for the parser to expand once nodes are known.
    pub(super) fn each_rules(&self) -> &[EachRule] {
        &self.each
    }

    /// Records `found`, which is on `line`.
    pub(super) fn record(&mut self, found: &Directive<'_>, line: u32) -> Result<(), DiagramError> {
        match *found {
            Directive::Order(rest) => self.order(rest, line, false),
            Directive::OpenOrder(rest) => self.order(rest, line, true),
            Directive::NoCycles(rest) => self.no_cycles(rest, line),
            Directive::Budget(rest) => self.budget(rest, line),
            Directive::Tests(rest) => self.tests(rest, line),
            Directive::Isolated(rest) => self.isolated(rest, line),
            Directive::Facade(rest) => self.facade(rest, line),
            Directive::Deny(rest) => self.deny(rest, line),
            Directive::Each(rest) => {
                self.each.push(each::parse(rest, line)?);
                Ok(())
            }
        }
    }

    fn isolated(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let malformed = |reason: String| DiagramError::MalformedRule {
            line,
            rule: "isolated".to_owned(),
            reason,
        };
        if rest.is_empty() {
            return Err(malformed(
                "expected `%% isolated: <boundary>, <boundary>`".to_owned(),
            ));
        }
        for raw in rest.split(',') {
            let id = NodeId::new(raw.trim())
                .map_err(|source| DiagramError::InvalidId { line, source })?;
            if let Some(first) = self.isolated.get(id.as_str()) {
                return Err(malformed(format!(
                    "`{id}` is already isolated on line {first}"
                )));
            }
            self.isolated.insert(id.to_string(), line);
        }
        Ok(())
    }

    fn deny(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let malformed = |reason: String| DiagramError::MalformedRule {
            line,
            rule: "deny".to_owned(),
            reason,
        };
        let usage = "expected `%% deny <from>: <to>, <to>`";
        let Some((raw_from, raw_to)) = rest.split_once(':') else {
            return Err(malformed(usage.to_owned()));
        };
        if raw_to.trim().is_empty() {
            return Err(malformed(usage.to_owned()));
        }
        let id = |raw: &str| {
            NodeId::new(raw.trim()).map_err(|source| DiagramError::InvalidId { line, source })
        };
        let from = id(raw_from)?;
        for raw in raw_to.split(',') {
            let to = id(raw)?;
            if let Some(first) = self
                .deny
                .iter()
                .find(|pending| pending.from == from && pending.to == to)
            {
                return Err(malformed(format!(
                    "`{from}` to `{to}` is already denied on line {}",
                    first.line
                )));
            }
            self.deny.push(PendingDenial {
                line,
                from: from.clone(),
                to,
            });
        }
        Ok(())
    }

    fn facade(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let malformed = |reason: String| DiagramError::MalformedRule {
            line,
            rule: "facade".to_owned(),
            reason,
        };
        if rest.is_empty() {
            return Err(malformed("expected `%% facade <node>, <node>`".to_owned()));
        }
        for raw in rest.split(',') {
            let node = NodeId::new(raw.trim())
                .map_err(|source| DiagramError::InvalidId { line, source })?;
            if let Some(first) = self.facades.get(&node) {
                return Err(malformed(format!(
                    "`{node}` is already a facade on line {first}"
                )));
            }
            self.facades.insert(node, line);
        }
        Ok(())
    }

    fn tests(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        if rest != "isolated" {
            return Err(DiagramError::MalformedRule {
                line,
                rule: "tests".to_owned(),
                reason: "expected `%% tests: isolated`".to_owned(),
            });
        }
        if let Some(first_line) = self.tests_isolated {
            return Err(DiagramError::DuplicateRule {
                line,
                rule: "tests".to_owned(),
                first_line,
            });
        }
        self.tests_isolated = Some(line);
        Ok(())
    }

    fn budget(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let malformed = |reason: String| DiagramError::MalformedRule {
            line,
            rule: "budget".to_owned(),
            reason,
        };
        let (raw_node, list) = rest.split_once(':').ok_or_else(|| {
            malformed(
                "expected `<node>: fan-out <n>, files <n>, lines <n>, interface <n>`".to_owned(),
            )
        })?;
        let node = NodeId::new(raw_node.trim())
            .map_err(|source| DiagramError::InvalidId { line, source })?;
        let mut budget = Budget {
            line,
            ..Budget::default()
        };
        for item in list.split(',') {
            let mut words = item.split_whitespace();
            let (Some(measure), Some(raw), None) = (words.next(), words.next(), words.next())
            else {
                return Err(malformed(format!(
                    "`{}` is not `<measure> <n>` with the measure `fan-out`, `files`, `lines` or `interface`",
                    item.trim()
                )));
            };
            let slot = match measure {
                "fan-out" => &mut budget.fan_out,
                "files" => &mut budget.files,
                "lines" => &mut budget.lines,
                "interface" => &mut budget.interface,
                other => {
                    return Err(malformed(format!(
                        "`{other}` is not a measure: use `fan-out`, `files`, `lines` or `interface`"
                    )));
                }
            };
            if slot.is_some() {
                return Err(malformed(format!("`{measure}` is given twice")));
            }
            let limit = raw
                .parse::<usize>()
                .map_err(|_| malformed(format!("`{raw}` is not a whole number")))?;
            *slot = Some(limit);
        }
        if let Some(first) = self.budgets.get(&node) {
            return Err(DiagramError::DuplicateRule {
                line,
                rule: format!("budget {node}"),
                first_line: first.line,
            });
        }
        self.budgets.insert(node, budget);
        Ok(())
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

    fn order(&mut self, rest: &str, line: u32, open: bool) -> Result<(), DiagramError> {
        let malformed = |reason: &str| DiagramError::MalformedRule {
            line,
            rule: "order".to_owned(),
            reason: reason.to_owned(),
        };
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
        let chain = LayerOrder {
            line,
            open,
            boundaries,
        };
        if let Some(reason) = self
            .order
            .iter()
            .find_map(|first| contradiction(first, &chain))
        {
            return Err(malformed(&reason));
        }
        self.order.push(chain);
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
        boundary_edges: &[BoundaryEdge],
    ) -> Result<Rules, DiagramError> {
        for order in &self.order {
            check_order(order, nodes, boundaries, edges, edge_lines, boundary_edges)?;
        }
        for (id, line) in &self.isolated {
            if !boundaries.contains_key(id) {
                return Err(not_a_boundary(*line, "isolated", id, nodes));
            }
        }
        check_isolated(&self.isolated, nodes, edges, edge_lines, boundary_edges)?;
        let deny = resolve_denials(&self.deny, nodes, boundaries)?;
        check_denied_arrows(&deny, nodes, edges, edge_lines, boundary_edges)?;
        for (node, budget) in &self.budgets {
            if !nodes.contains_key(node) {
                return Err(DiagramError::MalformedRule {
                    line: budget.line,
                    rule: "budget".to_owned(),
                    reason: format!("`{node}` is not a declared node"),
                });
            }
        }
        for (node, line) in &self.facades {
            if !nodes.contains_key(node) {
                return Err(DiagramError::MalformedRule {
                    line: *line,
                    rule: "facade".to_owned(),
                    reason: format!("`{node}` is not a declared node"),
                });
            }
        }
        if let Some(line) = self.no_cycles {
            check_node_cycles(line, nodes, edges, edge_lines, boundary_edges)?;
        }
        if let Some(line) = self.no_cycles_boundaries {
            check_boundary_cycles(line, nodes, edges, boundary_edges)?;
        }
        Ok(Rules {
            order: self.order,
            no_cycles: self.no_cycles,
            no_cycles_boundaries: self.no_cycles_boundaries,
            budgets: self.budgets,
            tests_isolated: self.tests_isolated,
            isolated: self.isolated,
            facades: self.facades,
            each: self.each,
            deny,
        })
    }
}

/// Resolves the ids of every denial: a declared node, else a declared boundary, else an error.
fn resolve_denials(
    pending: &[PendingDenial],
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
) -> Result<Vec<Denial>, DiagramError> {
    let endpoint = |id: &NodeId, line: u32| {
        if nodes.contains_key(id) {
            Ok(Endpoint::Node(id.clone()))
        } else if boundaries.contains_key(id.as_str()) {
            Ok(Endpoint::Boundary(id.to_string()))
        } else {
            Err(DiagramError::MalformedRule {
                line,
                rule: "deny".to_owned(),
                reason: format!("`{id}` is not a declared node or boundary"),
            })
        }
    };
    pending
        .iter()
        .map(|denial| {
            Ok(Denial {
                line: denial.line,
                from: endpoint(&denial.from, denial.line)?,
                to: endpoint(&denial.to, denial.line)?,
            })
        })
        .collect()
}

/// No declared edge of any kind may cover a pair a denial covers (see "Denial" in
/// `docs/DESIGN.md`): the contract would contradict itself. The first such edge by line is named.
fn check_denied_arrows(
    deny: &[Denial],
    nodes: &BTreeMap<NodeId, NodeInfo>,
    edges: &BTreeMap<Edge, EdgeKind>,
    edge_lines: &BTreeMap<Edge, u32>,
    boundary_edges: &[BoundaryEdge],
) -> Result<(), DiagramError> {
    for denial in deny {
        let from_nodes = edges
            .iter()
            .filter(|(edge, _)| denial.covers(nodes, edge))
            .map(|(edge, kind)| {
                let line = edge_lines.get(edge).copied().unwrap_or(denial.line);
                (line, format!("{} {} {}", edge.from, kind.arrow(), edge.to))
            });
        let from_boundaries = boundary_edges
            .iter()
            .filter(|declared| {
                declared
                    .pairs(nodes)
                    .iter()
                    .any(|pair| denial.covers(nodes, pair))
            })
            .map(|declared| (declared.line, declared.text()));
        if let Some((line, text)) = from_nodes.chain(from_boundaries).min() {
            return Err(DiagramError::MalformedRule {
                line: denial.line,
                rule: "deny".to_owned(),
                reason: format!(
                    "`{text}` on line {line} is declared, but the denial covers it: the contract contradicts itself"
                ),
            });
        }
    }
    Ok(())
}

/// A denial must remove something: some pair it covers that an open chain allows, after
/// narrowing. One that removes nothing is a dead rule, a contract error at its line. Checked on
/// the whole contract, as the chains, the edges and the members are needed together.
pub(super) fn check_denials_remove_something(contract: &Contract) -> Result<(), DiagramError> {
    let nodes = &contract.nodes;
    for denial in &contract.rules.deny {
        let allowed = denial.from.members(nodes).into_iter().any(|from| {
            denial.to.members(nodes).into_iter().any(|to| {
                let pair = Edge {
                    from: from.clone(),
                    to: to.clone(),
                };
                matches!(contract.open_access(&pair), Some(OpenAccess::Allowed(_)))
            })
        });
        if !allowed {
            return Err(DiagramError::MalformedRule {
                line: denial.line,
                rule: "deny".to_owned(),
                reason: format!(
                    "no open chain lets `{}` use `{}`, so the denial removes nothing",
                    denial.from, denial.to
                ),
            });
        }
    }
    Ok(())
}

/// Why `later` disagrees with `first`, or `None` when they agree: every two boundaries both list
/// must be in the same relative order (see "Layer order").
fn contradiction(first: &LayerOrder, later: &LayerOrder) -> Option<String> {
    let shared: Vec<&String> = later
        .boundaries
        .iter()
        .filter(|boundary| first.rank(boundary).is_some())
        .collect();
    shared.iter().enumerate().find_map(|(at, above)| {
        shared.iter().skip(at + 1).find_map(|below| {
            (first.rank(above) > first.rank(below)).then(|| {
                format!(
                    "contradicts the `%% order` on line {}: `{above}` is below `{below}` there but above it here",
                    first.line
                )
            })
        })
    })
}

/// The boundaries of a layer order must be declared `subgraph`s, and no declared arrow may point
/// upward, test edges excepted.
fn check_order(
    order: &LayerOrder,
    nodes: &BTreeMap<NodeId, NodeInfo>,
    boundaries: &BTreeMap<String, u32>,
    edges: &BTreeMap<Edge, EdgeKind>,
    edge_lines: &BTreeMap<Edge, u32>,
    boundary_edges: &[BoundaryEdge],
) -> Result<(), DiagramError> {
    for listed in &order.boundaries {
        if boundaries.contains_key(listed) {
            continue;
        }
        return Err(not_a_boundary(order.line, "order", listed, nodes));
    }
    // why: a node's layer comes from the one boundary the chain lists around it, so two nested ones
    // why: listed together would give it two.
    for outer in &order.boundaries {
        for inner in &order.boundaries {
            if contains(nodes, outer, inner) {
                return Err(DiagramError::MalformedRule {
                    line: order.line,
                    rule: "order".to_owned(),
                    reason: format!("`{outer}` contains `{inner}`, and a chain may not list both"),
                });
            }
        }
    }
    let rank_of = |id: &NodeId| nodes.get(id).and_then(|info| order.place(info));
    let upward_pair = |edge: &Edge| {
        let (from_boundary, from_rank) = rank_of(&edge.from)?;
        let (to_boundary, to_rank) = rank_of(&edge.to)?;
        (from_rank > to_rank).then(|| format!("`{from_boundary}` is below `{to_boundary}`"))
    };
    let counts = |kind: &EdgeKind| !matches!(kind, EdgeKind::Test | EdgeKind::PlannedTest);
    let node_upward = edges
        .iter()
        .filter(|(_, kind)| counts(kind))
        .filter_map(|(edge, _)| {
            Some((
                edge_lines.get(edge).copied().unwrap_or(order.line),
                Endpoint::Node(edge.from.clone()),
                Endpoint::Node(edge.to.clone()),
                upward_pair(edge)?,
            ))
        });
    // why: a boundary edge is its pairs, and one upward pair makes the whole edge a contradiction.
    let boundary_upward = boundary_edges
        .iter()
        .filter(|declared| counts(&declared.kind))
        .filter_map(|declared| {
            let relation = declared.pairs(nodes).iter().find_map(&upward_pair)?;
            Some((
                declared.line,
                declared.from.clone(),
                declared.to.clone(),
                relation,
            ))
        });
    match node_upward
        .chain(boundary_upward)
        .min_by_key(|(line, ..)| *line)
    {
        Some((line, from, to, relation)) => Err(DiagramError::UpwardArrow {
            line,
            from,
            to,
            relation,
            order_line: order.line,
        }),
        None => Ok(()),
    }
}

/// The error for a directive that names `id` as a boundary when it is not one.
pub(super) fn not_a_boundary(
    line: u32,
    rule: &str,
    id: &str,
    nodes: &BTreeMap<NodeId, NodeInfo>,
) -> DiagramError {
    let reason = if nodes.keys().any(|node| node.as_str() == id) {
        format!("`{id}` is a node, not a boundary")
    } else {
        format!("`{id}` is not a declared boundary")
    };
    DiagramError::MalformedRule {
        line,
        rule: rule.to_owned(),
        reason,
    }
}

/// Whether boundary `outer` has boundary `inner` nested inside it, at any depth.
fn contains(nodes: &BTreeMap<NodeId, NodeInfo>, outer: &str, inner: &str) -> bool {
    nodes.values().any(|info| {
        let chain: Vec<&str> = info.boundaries().collect();
        let at = |id: &str| chain.iter().position(|member| *member == id);
        matches!((at(outer), at(inner)), (Some(a), Some(b)) if a < b)
    })
}

/// No declared edge may join two children of an isolated boundary (see "Isolated boundaries"): the
/// first such edge by line is the error.
fn check_isolated(
    isolated: &BTreeMap<String, u32>,
    nodes: &BTreeMap<NodeId, NodeInfo>,
    edges: &BTreeMap<Edge, EdgeKind>,
    edge_lines: &BTreeMap<Edge, u32>,
    boundary_edges: &[BoundaryEdge],
) -> Result<(), DiagramError> {
    let joins = |boundary: &str, pair: &Edge| {
        children_apart(nodes, boundary, &pair.from, &pair.to)
            .map(|(first, second)| (first.to_owned(), second.to_owned()))
    };
    let mut found: Vec<DiagramError> = Vec::new();
    for (boundary, isolated_line) in isolated {
        let from_nodes = edges.iter().filter_map(|(edge, kind)| {
            let (first, second) = joins(boundary, edge)?;
            Some((
                edge_lines.get(edge).copied().unwrap_or(*isolated_line),
                format!("{} {} {}", edge.from, kind.arrow(), edge.to),
                (first, second),
            ))
        });
        let from_boundaries = boundary_edges.iter().filter_map(|declared| {
            let (first, second) = declared
                .pairs(nodes)
                .iter()
                .find_map(|pair| joins(boundary, pair))?;
            Some((declared.line, declared.text(), (first, second)))
        });
        found.extend(
            from_nodes
                .chain(from_boundaries)
                .map(|(line, edge, apart)| DiagramError::IsolatedArrow {
                    line,
                    edge,
                    boundary: boundary.clone(),
                    apart: Box::new(apart),
                    isolated_line: *isolated_line,
                }),
        );
    }
    found.sort_by_key(DiagramError::line);
    found.into_iter().next().map_or(Ok(()), Err)
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
    boundary_edges: &[BoundaryEdge],
) -> Result<(), DiagramError> {
    let mut graph: BTreeMap<String, BTreeSet<String>> = nodes
        .keys()
        .map(|id| (id.to_string(), BTreeSet::new()))
        .collect();
    // why: what names a step in the message: its own line, else the first boundary edge to cover it.
    let mut named: BTreeMap<(String, String), String> = BTreeMap::new();
    for (edge, _) in edges.iter().filter(|(_, kind)| counts_for_cycles(**kind)) {
        let (from, to) = (edge.from.to_string(), edge.to.to_string());
        graph.entry(from.clone()).or_default().insert(to.clone());
        if let Some(at) = edge_lines.get(edge) {
            named.insert((from, to), format!("(line {at})"));
        }
    }
    for declared in boundary_edges
        .iter()
        .filter(|declared| counts_for_cycles(declared.kind))
    {
        for pair in declared.pairs(nodes) {
            let (from, to) = (pair.from.to_string(), pair.to.to_string());
            graph.entry(from.clone()).or_default().insert(to.clone());
            named
                .entry((from, to))
                .or_insert_with(|| format!("(via {}, line {})", declared.text(), declared.line));
        }
    }
    let Some(cycle) = find_cycle(&graph) else {
        return Ok(());
    };
    let described: Vec<String> = cycle
        .iter()
        .zip(cycle.iter().cycle().skip(1))
        .map(|(from, to)| {
            named.get(&(from.clone(), to.clone())).map_or_else(
                || format!("{from} --> {to}"),
                |at| format!("{from} --> {to} {at}"),
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
    boundary_edges: &[BoundaryEdge],
) -> Result<(), DiagramError> {
    let pairs: Vec<Edge> = edges
        .iter()
        .filter(|(_, kind)| counts_for_cycles(**kind))
        .map(|(edge, _)| edge.clone())
        .chain(
            boundary_edges
                .iter()
                .filter(|declared| counts_for_cycles(declared.kind))
                .flat_map(|declared| declared.pairs(nodes)),
        )
        .collect();
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for info in nodes.values() {
        for name in info.boundaries() {
            graph.entry(name.to_owned()).or_default();
        }
    }
    // why: only boundaries that hold one node and not the other are joined, so a boundary and one
    // why: around it never are, and two nested features meet at the feature level.
    for edge in &pairs {
        let (Some(from), Some(to)) = (nodes.get(&edge.from), nodes.get(&edge.to)) else {
            continue;
        };
        for x in from.boundaries().filter(|name| !to.within(name)) {
            for y in to.boundaries().filter(|name| !from.within(name)) {
                graph.entry(x.to_owned()).or_default().insert(y.to_owned());
            }
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
