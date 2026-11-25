//! What the contract says: nodes, edges, boundary edges, rules, maps, surfaces and package rules.
//!
//! This model depends on nothing else in the crate. Every collection here is ordered (`BTreeMap`,
//! `BTreeSet`, sorted `Vec`) so output is deterministic regardless of filesystem or thread
//! ordering.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Serialize;
use thiserror::Error;

mod boundary;
mod map;
mod open_order;
mod package_rule;
mod rules;

pub use boundary::{
    BoundaryEdge, Cover, Declared, EdgeEnds, Endpoint, Template, child_of, children, children_apart,
};
pub use map::{GlobError, MapGlob, MapRule, MapTarget, Specificity};
pub use open_order::{EachLimit, Narrowing, OpenAccess};
pub use package_rule::{PackagePattern, PackageRule, PackageRuleKind};
pub use rules::{Budget, Denial, EachArrow, EachRule, LayerOrder, Rules};

/// Contract path used when none is given, relative to the repository root.
pub const DEFAULT_CONTRACT: &str = "architecture.mmd";

/// Errors raised when constructing contract values.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    /// A node identifier did not match `[A-Za-z][A-Za-z0-9_-]*`.
    #[error("invalid node id `{0}`: expected [A-Za-z][A-Za-z0-9_-]*")]
    InvalidNodeId(String),
    /// A node id prefix did not match `[A-Za-z][A-Za-z0-9_-]*-`.
    #[error("invalid node id prefix `{0}`: expected [A-Za-z][A-Za-z0-9_-]*-")]
    InvalidPrefix(String),
}

/// A validated architecture node identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Validates and wraps `raw`.
    ///
    /// # Errors
    /// Returns [`ModelError::InvalidNodeId`] when `raw` is empty or uses disallowed characters.
    pub fn new(raw: &str) -> Result<Self, ModelError> {
        let mut chars = raw.chars();
        let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
        let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if first_ok && rest_ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ModelError::InvalidNodeId(raw.to_owned()))
        }
    }

    /// The identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The text put in front of the module names under `node: <prefix>*`, such as `ui-`.
///
/// A prefix is either empty (a plain `node: *`) or matches `[A-Za-z][A-Za-z0-9_-]*-`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Prefix(String);

impl Prefix {
    /// Validates and wraps a non-empty prefix: a valid node id spelling that ends in `-`.
    ///
    /// # Errors
    /// Returns [`ModelError::InvalidPrefix`] when `raw` is empty, does not end in `-`, or uses
    /// disallowed characters.
    pub fn new(raw: &str) -> Result<Self, ModelError> {
        if raw.ends_with('-') && NodeId::new(raw).is_ok() {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ModelError::InvalidPrefix(raw.to_owned()))
        }
    }

    /// The prefix as a string slice, empty for none.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The node id of the module called `name` under this prefix: the prefix followed by the name.
    ///
    /// # Errors
    /// Returns [`ModelError::InvalidNodeId`] when `name` is empty or the result is not a valid id.
    pub fn derive(&self, name: &str) -> Result<NodeId, ModelError> {
        let id = format!("{}{name}", self.0);
        if name.is_empty() {
            return Err(ModelError::InvalidNodeId(id));
        }
        NodeId::new(&id)
    }
}

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a declared dependency is enforced today or only intended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    /// Solid arrow (`-->`): the dependency must exist in code, otherwise it is stale.
    Implemented,
    /// Dotted arrow (`-.->`): declared intent, tolerated whether or not code uses it yet.
    Planned,
    /// Solid arrow labelled `test` (`-->|test|`): only test-only imports are allowed, and one must
    /// exist, otherwise it is stale.
    Test,
    /// Dotted arrow labelled `test` (`-.->|test|`): test-only imports are allowed, none is needed.
    PlannedTest,
    /// Solid arrow labelled `type` (`-->|type|`): only type-only imports are allowed, and one must
    /// exist, otherwise it is stale.
    Type,
    /// Dotted arrow labelled `type` (`-.->|type|`): type-only imports are allowed, none is needed.
    PlannedType,
    /// Solid arrow labelled `type, test`: type-only and test-only imports are allowed, and one
    /// must exist, otherwise it is stale.
    TypeTest,
    /// Dotted arrow labelled `type, test`: type-only and test-only imports are allowed, none is
    /// needed.
    PlannedTypeTest,
}

impl EdgeKind {
    /// The kind with the given restrictions: `planned` for a dotted arrow, `type_label` and
    /// `test_label` for the words of the label.
    #[must_use]
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "three independent parts of an arrow; this is where they become one EdgeKind"
    )]
    pub const fn from_parts(planned: bool, type_label: bool, test_label: bool) -> Self {
        match (planned, type_label, test_label) {
            (false, false, false) => Self::Implemented,
            (true, false, false) => Self::Planned,
            (false, false, true) => Self::Test,
            (true, false, true) => Self::PlannedTest,
            (false, true, false) => Self::Type,
            (true, true, false) => Self::PlannedType,
            (false, true, true) => Self::TypeTest,
            (true, true, true) => Self::PlannedTypeTest,
        }
    }

    /// Whether the edge is tolerated without code (dotted).
    #[must_use]
    pub const fn is_planned(self) -> bool {
        matches!(
            self,
            Self::Planned | Self::PlannedTest | Self::PlannedType | Self::PlannedTypeTest
        )
    }

    /// The arrow the contract writes the edge with, label included.
    #[must_use]
    pub const fn arrow(self) -> &'static str {
        match self {
            Self::Implemented => "-->",
            Self::Planned => "-.->",
            Self::Test => "-->|test|",
            Self::PlannedTest => "-.->|test|",
            Self::Type => "-->|type|",
            Self::PlannedType => "-.->|type|",
            Self::TypeTest => "-->|type, test|",
            Self::PlannedTypeTest => "-.->|type, test|",
        }
    }

    /// Whether the label has the word `test`: test-only imports are allowed.
    #[must_use]
    pub const fn is_test(self) -> bool {
        matches!(
            self,
            Self::Test | Self::PlannedTest | Self::TypeTest | Self::PlannedTypeTest
        )
    }

    /// Whether the label has the word `type`: type-only imports are allowed.
    #[must_use]
    pub const fn is_type(self) -> bool {
        matches!(
            self,
            Self::Type | Self::PlannedType | Self::TypeTest | Self::PlannedTypeTest
        )
    }

    /// Whether every site `narrower` allows is allowed by `self` too, planned or solid aside: an
    /// unrestricted edge covers any kind, `type, test` covers `type`, `test` and itself, and any
    /// other kind covers itself.
    #[must_use]
    pub const fn at_least_as_wide_as(self, narrower: Self) -> bool {
        if !self.is_restricted() {
            return true;
        }
        if !narrower.is_restricted() {
            return false;
        }
        (!narrower.is_type() || self.is_type()) && (!narrower.is_test() || self.is_test())
    }

    /// Whether the label restricts the allowed sites (`type` or `test`).
    #[must_use]
    pub const fn is_restricted(self) -> bool {
        self.is_test() || self.is_type()
    }

    /// What the allowed sites are called in a message: `test`, `type-only` or `type-only or test`.
    #[must_use]
    pub const fn allowed_sites(self) -> &'static str {
        match (self.is_type(), self.is_test()) {
            (true, true) => "type-only or test",
            (true, false) => "type-only",
            (false, true) => "test",
            (false, false) => "any",
        }
    }
}

/// A directed dependency between two nodes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Edge {
    /// The importing node.
    pub from: NodeId,
    /// The imported node.
    pub to: NodeId,
}

/// Metadata for a node declared in the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeInfo {
    /// Human readable label, if the diagram gave one.
    pub label: Option<String>,
    /// Innermost enclosing boundary (`subgraph`) id, if any.
    pub boundary: Option<String>,
    /// The boundaries around `boundary`, outermost first: the rest of the nesting of "Nested
    /// boundaries" in `docs/DESIGN.md`. Empty for a node in at most one boundary.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outer: Vec<String>,
}

impl NodeInfo {
    /// Every boundary the node is a member of, outermost first, the innermost last.
    pub fn boundaries(&self) -> impl Iterator<Item = &str> {
        self.outer
            .iter()
            .map(String::as_str)
            .chain(self.boundary.as_deref())
    }

    /// Whether the node is a member of `boundary`, written directly in it or at any depth below.
    #[must_use]
    pub fn within(&self, boundary: &str) -> bool {
        self.boundaries().any(|id| id == boundary)
    }
}

/// The parsed architecture contract: the source of truth.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Contract {
    /// Declared nodes.
    pub nodes: BTreeMap<NodeId, NodeInfo>,
    /// The label of each boundary written with one that differs from its id. A boundary absent
    /// here is drawn with its id as its label.
    #[serde(skip)]
    pub boundary_labels: BTreeMap<String, String>,
    /// Declared edges and whether each is enforced.
    pub edges: BTreeMap<Edge, EdgeKind>,
    /// Edges with a boundary at one end or both, in file order. See "Boundary edges".
    #[serde(skip)]
    pub boundary_edges: Vec<BoundaryEdge>,
    /// Public surface per node, from `%% exports` lines. A node absent here is unrestricted, and a
    /// node present here has at least one name.
    pub surfaces: BTreeMap<NodeId, BTreeSet<String>>,
    /// The `%% map` lines in file order. When any exists, they alone decide ownership.
    #[serde(skip)]
    pub maps: Vec<MapRule>,
    /// The rule directives: layer order and the others of "Contract rules".
    #[serde(skip)]
    pub rules: Rules,
    /// The `%% packages` and `%% forbid-packages` lines in file order.
    #[serde(skip)]
    pub package_rules: Vec<PackageRule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_kinds_round_trip_through_their_parts_and_name_their_arrow() {
        for planned in [false, true] {
            for (is_type, is_test, arrow) in [
                (false, false, ""),
                (true, false, "|type|"),
                (false, true, "|test|"),
                (true, true, "|type, test|"),
            ] {
                let kind = EdgeKind::from_parts(planned, is_type, is_test);
                assert_eq!(kind.is_planned(), planned);
                assert_eq!((kind.is_type(), kind.is_test()), (is_type, is_test));
                let dash = if planned { "-.->" } else { "-->" };
                assert_eq!(kind.arrow(), format!("{dash}{arrow}"));
            }
        }
    }

    #[test]
    fn node_id_accepts_identifiers() {
        assert!(NodeId::new("billing-service_2").is_ok());
    }

    #[test]
    fn node_id_rejects_bad_input() {
        for bad in ["", "1abc", "a b", "a.b"] {
            assert_eq!(
                NodeId::new(bad),
                Err(ModelError::InvalidNodeId(bad.to_owned()))
            );
        }
    }

    #[test]
    fn a_prefix_ends_in_a_dash_and_is_otherwise_an_id() {
        for good in ["ui-", "a-", "ui-kit-", "a_b-", "A1--"] {
            assert_eq!(Prefix::new(good).unwrap().as_str(), good);
        }
        for bad in [
            "", "-", "ui", "ui_", "9-", "-ui-", "u i-", "u.i-", "*-", "ui-*",
        ] {
            assert_eq!(
                Prefix::new(bad),
                Err(ModelError::InvalidPrefix(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_prefix_derives_the_id_as_prefix_then_name() {
        let ui = Prefix::new("ui-").unwrap();
        assert_eq!(ui.derive("components").unwrap().as_str(), "ui-components");
        assert_eq!(ui.derive("9lives").unwrap().as_str(), "ui-9lives");
        assert_eq!(Prefix::default().derive("a").unwrap().as_str(), "a");
        let none = Prefix::default();
        for (prefix, name) in [(&ui, ""), (&ui, "a.b"), (&none, "9"), (&none, "")] {
            assert!(prefix.derive(name).is_err(), "{prefix}{name}");
        }
    }
}
