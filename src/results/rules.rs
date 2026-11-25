//! The rule directives as the overview presents them, and the budget measures of a node.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::contract::{EachArrow, NodeId, NodeInfo, Rules, children};

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
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is one independent directive of the contract; every combination is valid"
)]
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
    /// The `%% deny` denials in file order, one per `(from, to)` pair. Empty, and omitted from
    /// JSON, without one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<DenySummary>,
}

/// One denial as the overview presents it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DenySummary {
    /// The importing end as written, a node or a boundary.
    pub from: String,
    /// The imported end as written, a node or a boundary.
    pub to: String,
    /// Line of the directive.
    pub line: u32,
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
            && self.deny.is_empty()
    }
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
            deny: self
                .deny
                .iter()
                .map(|denial| DenySummary {
                    from: denial.from.to_string(),
                    to: denial.to.to_string(),
                    line: denial.line,
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
