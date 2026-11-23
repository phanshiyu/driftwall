//! The rule directives of a contract (see "Contract rules" in `docs/DESIGN.md`).

use serde::Serialize;

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
    /// The boundaries of `%% order`, top first. Empty, and omitted from JSON, without one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
}

impl RulesSummary {
    /// Whether the contract has no rule directive at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

impl Rules {
    /// The directives as the overview presents them.
    #[must_use]
    pub fn summary(&self) -> RulesSummary {
        RulesSummary {
            order: self
                .order
                .as_ref()
                .map(|order| order.boundaries.clone())
                .unwrap_or_default(),
        }
    }

    /// Where a node declared in `boundary` sits in the layer order, if it is listed.
    #[must_use]
    pub fn layer(&self, boundary: Option<&str>) -> Option<Layer> {
        let order = self.order.as_ref()?;
        let boundary = boundary?;
        Some(Layer {
            boundary: boundary.to_owned(),
            rank: order.rank(boundary)?,
            of: order.boundaries.len(),
        })
    }
}

/// The rule directives of a contract. Every field is absent when the contract has no such line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Rules {
    /// `%% order: ...`, see "Layer order".
    pub order: Option<LayerOrder>,
}

impl Rules {
    /// Whether the contract has no rule directive at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.order.is_none()
    }
}
