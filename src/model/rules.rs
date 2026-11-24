//! The rule directives of a contract (see "Contract rules" in `docs/DESIGN.md`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use super::{EdgeKind, NodeId, NodeInfo, children};

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

/// Where a node sits in the layer order: its boundary and the boundary's place among those listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Layer {
    /// The node's boundary, which the `%% order` line lists.
    pub boundary: String,
    /// 0 for the top boundary of the order.
    pub rank: usize,
    /// How many boundaries the order lists.
    pub of: usize,
}

/// What the contract's rule directives say, as the overview presents them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RulesSummary {
    /// The chains of the `%% order` lines in file order, each with its boundaries top first.
    /// Empty, and omitted from JSON, without one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<Vec<String>>,
    /// The chains of the `%% order open:` lines, each with its boundaries top first. They are in
    /// `order` too. Empty, and omitted from JSON, without one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub open_order: Vec<Vec<String>>,
    /// Whether the contract has `%% no-cycles`. Omitted from JSON when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_cycles: bool,
    /// Whether the contract has `%% no-cycles: boundaries`. Omitted from JSON when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_cycles_boundaries: bool,
    /// The `%% isolated` boundaries with their children, by boundary id. Empty, and omitted from
    /// JSON, without one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub isolated: Vec<IsolatedSummary>,
    /// Whether the contract has `%% tests: isolated`. Omitted from JSON when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tests_isolated: bool,
    /// The nodes of the `%% facade` lines, sorted. Empty, and omitted from JSON, without one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub facades: Vec<String>,
    /// The `%% each` edge templates in file order. Empty, and omitted from JSON, without one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub each: Vec<EachSummary>,
}

/// An `%% each` template as the overview presents it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EachSummary {
    /// The boundary whose children the template applies to.
    pub boundary: String,
    /// Line of the directive.
    pub line: u32,
    /// The arrows as written, `controller --> service`.
    pub arrows: Vec<String>,
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

/// An `%% isolated` boundary with the children that may not use each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IsolatedSummary {
    /// The isolated boundary.
    pub boundary: String,
    /// Its children in id order: child boundaries and nodes written directly in it.
    pub children: Vec<String>,
}

impl RulesSummary {
    /// Whether the contract has no rule directive at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.order.is_empty()
            && !self.no_cycles
            && !self.no_cycles_boundaries
            && !self.tests_isolated
            && self.isolated.is_empty()
            && self.facades.is_empty()
            && self.each.is_empty()
    }
}

impl Rules {
    /// The directives as the overview presents them.
    #[must_use]
    pub fn summary(&self, nodes: &BTreeMap<NodeId, NodeInfo>) -> RulesSummary {
        RulesSummary {
            order: self
                .order
                .iter()
                .map(|order| order.boundaries.clone())
                .collect(),
            open_order: self
                .order
                .iter()
                .filter(|order| order.open)
                .map(|order| order.boundaries.clone())
                .collect(),
            no_cycles: self.no_cycles.is_some(),
            no_cycles_boundaries: self.no_cycles_boundaries.is_some(),
            tests_isolated: self.tests_isolated.is_some(),
            isolated: self
                .isolated
                .keys()
                .map(|boundary| IsolatedSummary {
                    boundary: boundary.clone(),
                    children: children(nodes, boundary),
                })
                .collect(),
            facades: self.facades.keys().map(ToString::to_string).collect(),
            each: self
                .each
                .iter()
                .map(|rule| EachSummary {
                    boundary: rule.boundary.clone(),
                    line: rule.line,
                    arrows: rule.arrows.iter().map(EachArrow::text).collect(),
                })
                .collect(),
        }
    }

    /// Where a node sits in the layer order: its place in the first chain that lists a boundary
    /// around it, if any does.
    #[must_use]
    pub fn layer(&self, info: &NodeInfo) -> Option<Layer> {
        self.order.iter().find_map(|order| {
            let (boundary, rank) = order.place(info)?;
            Some(Layer {
                boundary: boundary.to_owned(),
                rank,
                of: order.boundaries.len(),
            })
        })
    }
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

/// One measure of a node against its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Measure {
    /// What the budget allows.
    pub limit: usize,
    /// What the node has.
    pub actual: usize,
}

impl Measure {
    /// Whether the node exceeds the limit. Equal to it is within.
    #[must_use]
    pub const fn over(self) -> bool {
        self.actual > self.limit
    }
}

/// A node's budget with the current values, for the measures that have a limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct NodeBudget {
    /// Line of the `%% budget` directive, for findings. Not serialized.
    #[serde(skip)]
    pub line: u32,
    /// Distinct nodes depended on in production code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_out: Option<Measure>,
    /// Source files owned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Measure>,
    /// Lines in those files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Measure>,
    /// Distinct symbols of the node used from outside in production code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<Measure>,
}

/// One entry of a node's interface: a symbol that code outside the node uses, or a whole module
/// used without naming a symbol (see "Budgets" in `docs/DESIGN.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterfaceEntry {
    /// The file that defines the symbol, or the module used as a whole.
    pub file: PathBuf,
    /// The symbol's name, or `None` for a whole-module entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// How many import sites of other nodes use the entry.
    pub sites: usize,
    /// The sites per using node.
    pub users: BTreeMap<NodeId, usize>,
}

impl NodeBudget {
    /// The measures that have a limit, as `(name, measure)` in the order `fan-out`, `files`,
    /// `lines`, `interface`.
    #[must_use]
    pub fn measures(&self) -> Vec<(&'static str, Measure)> {
        [
            ("fan-out", self.fan_out),
            ("files", self.files),
            ("lines", self.lines),
            ("interface", self.interface),
        ]
        .into_iter()
        .filter_map(|(name, measure)| Some((name, measure?)))
        .collect()
    }
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
    }
}
