//! The rule directives of a contract (see "Contract rules" in `docs/DESIGN.md`).

use std::collections::BTreeMap;

use serde::Serialize;

use super::NodeId;

/// `%% order: a > b > c`: boundaries from the top (entry points) to the bottom (foundations).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LayerOrder {
    /// Line of the directive.
    pub line: u32,
    /// Boundary ids, top first.
    pub boundaries: Vec<String>,
}

impl LayerOrder {
    /// The position of `boundary` in the order, 0 for the top, or `None` when it is not listed.
    #[must_use]
    pub fn rank(&self, boundary: &str) -> Option<usize> {
        self.boundaries.iter().position(|listed| listed == boundary)
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
    /// Whether the contract has `%% no-cycles`. Omitted from JSON when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_cycles: bool,
    /// Whether the contract has `%% no-cycles: boundaries`. Omitted from JSON when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_cycles_boundaries: bool,
    /// Whether the contract has `%% tests: isolated`. Omitted from JSON when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tests_isolated: bool,
}

impl RulesSummary {
    /// Whether the contract has no rule directive at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.order.is_empty()
            && !self.no_cycles
            && !self.no_cycles_boundaries
            && !self.tests_isolated
    }
}

impl Rules {
    /// The directives as the overview presents them.
    #[must_use]
    pub fn summary(&self) -> RulesSummary {
        RulesSummary {
            order: self
                .order
                .iter()
                .map(|order| order.boundaries.clone())
                .collect(),
            no_cycles: self.no_cycles.is_some(),
            no_cycles_boundaries: self.no_cycles_boundaries.is_some(),
            tests_isolated: self.tests_isolated.is_some(),
        }
    }

    /// Where a node declared in `boundary` sits in the layer order: its place in the first chain
    /// that lists the boundary, if any does.
    #[must_use]
    pub fn layer(&self, boundary: Option<&str>) -> Option<Layer> {
        let boundary = boundary?;
        self.order.iter().find_map(|order| {
            Some(Layer {
                boundary: boundary.to_owned(),
                rank: order.rank(boundary)?,
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
}

impl NodeBudget {
    /// The measures that have a limit, as `(name, measure)` in the order `fan-out`, `files`,
    /// `lines`.
    #[must_use]
    pub fn measures(&self) -> Vec<(&'static str, Measure)> {
        [
            ("fan-out", self.fan_out),
            ("files", self.files),
            ("lines", self.lines),
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
    }
}
