//! The rule directives of a contract (see "Contract rules" in `docs/DESIGN.md`).

use std::collections::BTreeMap;

use serde::Serialize;

use super::{Contract, Edge, EdgeKind, Endpoint, NodeId, NodeInfo};

/// `%% order: a > b > c`: boundaries from the top (entry points) to the bottom (foundations).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LayerOrder {
    /// Line of the directive.
    pub line: u32,
    /// Whether the chain is `%% order open:`, which allows downward imports implicitly, see "Open
    /// order" in `docs/DESIGN.md`.
    pub open: bool,
    /// Boundary ids, top first.
    pub boundaries: Vec<String>,
}

impl LayerOrder {
    /// The position of `boundary` in the order, 0 for the top, or `None` when it is not listed.
    #[must_use]
    pub fn rank(&self, boundary: &str) -> Option<usize> {
        self.boundaries.iter().position(|listed| listed == boundary)
    }

    /// The boundary of the chain that holds the node, with its rank. A chain never lists a
    /// boundary together with one nested in it, so there is at most one.
    #[must_use]
    pub fn place<'a>(&self, info: &'a NodeInfo) -> Option<(&'a str, usize)> {
        info.boundaries()
            .find_map(|boundary| Some((boundary, self.rank(boundary)?)))
    }
}

/// `%% deny <from>: <to>`: no node of `from` may use a node of `to`, whatever an open chain allows,
/// see "Denial" in `docs/DESIGN.md`. A line naming several targets is one denial per target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Denial {
    /// Line of the directive.
    pub line: u32,
    /// The importing end: a node, or a boundary with every node inside it at any depth.
    pub from: Endpoint,
    /// The imported end.
    pub to: Endpoint,
}

impl Denial {
    /// Whether the denial covers the pair `edge`: its source is on the importing side, its target
    /// on the imported side, and the two nodes differ.
    #[must_use]
    pub fn covers(&self, nodes: &BTreeMap<NodeId, NodeInfo>, edge: &Edge) -> bool {
        edge.from != edge.to
            && self.from.contains(nodes, &edge.from)
            && self.to.contains(nodes, &edge.to)
    }

    /// The denial as written, `%% deny ui: core`.
    #[must_use]
    pub fn text(&self) -> String {
        format!("%% deny {}: {}", self.from, self.to)
    }
}

impl Contract {
    /// The first denial, in file order, that covers the pair `edge`.
    #[must_use]
    pub fn denial(&self, edge: &Edge) -> Option<&Denial> {
        self.rules
            .deny
            .iter()
            .find(|denial| denial.covers(&self.nodes, edge))
    }
}

/// One arrow of an `%% each` template: two roles and the kind of edge between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EachArrow {
    /// The role of the importing node.
    pub from: String,
    /// The role of the imported node.
    pub to: String,
    /// What the arrow and its label say.
    pub kind: EdgeKind,
}

impl EachArrow {
    /// The arrow as written, `controller -->|type| service`.
    #[must_use]
    pub fn text(&self) -> String {
        format!("{} {} {}", self.from, self.kind.arrow(), self.to)
    }
}

/// `%% each <boundary>: <arrow>, ...`: edges declared for every child boundary of `boundary`, see
/// "Edge templates" in `docs/DESIGN.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EachRule {
    /// Line of the directive.
    pub line: u32,
    /// The boundary whose direct child boundaries the arrows apply to.
    pub boundary: String,
    /// The arrows, in the order written.
    pub arrows: Vec<EachArrow>,
}

/// `%% budget <node>: fan-out <n>, files <n>, lines <n>`: the limits of one node, each optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Budget {
    /// Line of the directive.
    pub line: u32,
    /// The most distinct nodes the node may depend on in production code.
    pub fan_out: Option<usize>,
    /// The most source files the node may own.
    pub files: Option<usize>,
    /// The most lines its source files may hold together.
    pub lines: Option<usize>,
    /// The most distinct symbols of the node that code outside it may use in production code.
    pub interface: Option<usize>,
}

/// The rule directives of a contract. Every field is absent when the contract has no such line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Rules {
    /// The `%% order: ...` lines in file order, each an independent chain, see "Layer order".
    pub order: Vec<LayerOrder>,
    /// The line of `%% no-cycles`, see "No cycles".
    pub no_cycles: Option<u32>,
    /// The line of `%% no-cycles: boundaries`, see "No cycles".
    pub no_cycles_boundaries: Option<u32>,
    /// The `%% budget` lines by node.
    pub budgets: BTreeMap<NodeId, Budget>,
    /// The line of `%% tests: isolated`, see "Test isolation".
    pub tests_isolated: Option<u32>,
    /// The boundaries of the `%% isolated` lines with the line of each, see "Isolated boundaries".
    pub isolated: BTreeMap<String, u32>,
    /// The nodes of the `%% facade` lines with the line of each, see "Re-exports".
    pub facades: BTreeMap<NodeId, u32>,
    /// The `%% each` edge templates in file order, see "Edge templates".
    pub each: Vec<EachRule>,
    /// The `%% deny` denials in file order, one per `(from, to)` pair, see "Denial".
    pub deny: Vec<Denial>,
}

impl Rules {
    /// Whether the contract has no rule directive at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
            && self.no_cycles.is_none()
            && self.no_cycles_boundaries.is_none()
            && self.budgets.is_empty()
            && self.tests_isolated.is_none()
            && self.isolated.is_empty()
            && self.facades.is_empty()
            && self.each.is_empty()
            && self.deny.is_empty()
    }
}
