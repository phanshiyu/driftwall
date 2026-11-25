//! The overview and node views: every node and every declared or observed edge.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;

use crate::code::{ImportKind, Location, PackageKind, ResolvedImport, SkipReason, Symbol, Via};
use crate::contract::{EdgeKind, Endpoint, NodeId};

use super::{
    BlindRef, BlindSpots, BlindSummary, BlindTally, EdgeStatus, InterfaceEntry, Layer, NodeBudget,
    RulesSummary,
};

/// How many example import sites a text report shows per edge unless it is asked for all of them.
/// The overview keeps every site.
pub const MAX_SITES: usize = 3;

/// One import statement behind an observed edge.
///
/// Serialized flat: `file`, `line`, `kind`, `test_only`. Sites sort by location, then kind, then
/// test flag, so an edge lists its sites in the same order on every run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Site {
    /// Where the import statement starts.
    #[serde(flatten)]
    pub at: Location,
    /// What the import is for.
    pub kind: ImportKind,
    /// Whether the import is written in test code.
    pub test_only: bool,
    /// Set for an import that depends on a node without a file behind it, see [`Via`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
    /// Whether the import uses the target module itself: it uses no symbol and is neither an embed
    /// nor an asset. Not serialized: the edge says whether any of its sites does.
    #[serde(skip)]
    pub module: bool,
}

impl Site {
    /// A plain runtime import at `at` that is not test code and uses a named symbol.
    #[must_use]
    pub const fn plain(at: Location) -> Self {
        Self {
            at,
            kind: ImportKind::Runtime,
            test_only: false,
            via: None,
            module: false,
        }
    }

    /// The site of `import`.
    #[must_use]
    pub fn of(import: &ResolvedImport) -> Self {
        let names_nothing = import.symbols.is_empty();
        Self {
            at: import.at.clone(),
            kind: import.kind,
            test_only: import.test_only,
            via: import.via,
            module: names_nothing
                && matches!(import.kind, ImportKind::Runtime | ImportKind::TypeOnly),
        }
    }
}

/// One declared node as the overview presents it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeSummary {
    /// The node identifier.
    pub id: NodeId,
    /// Human readable label, if the diagram gave one.
    pub label: Option<String>,
    /// Where the node's code lives, sorted: the directories of the plain `node:` tags that name it
    /// and the module paths of its files that a `node: *` tag owns (see "Node directories" in
    /// `docs/DESIGN.md`). The repository root is `.`.
    pub dirs: Vec<PathBuf>,
    /// Source files this node owns.
    pub files: usize,
    /// Asset files this node owns (see "Asset files" in `docs/DESIGN.md`); they are no source
    /// files and are not in `files`.
    pub assets: usize,
    /// Excluded files this node owns (see "Excluded files"); neither source files nor assets.
    pub excluded: usize,
    /// The node's declared public surface, or `None` when it declares none and is unrestricted.
    /// Omitted from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<BTreeSet<String>>,
    /// What the node is blind to: its unread, excluded and ignored files and its skipped and
    /// unresolved references.
    pub blind_spots: BlindTally,
    /// Where the node's boundary sits in `%% order`, or `None` when it is not listed. Omitted from
    /// JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<Layer>,
    /// The node's `%% budget` with its current values, or `None` without one. Omitted from JSON
    /// when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<NodeBudget>,
    /// The packages the node's files use, third party first, then by name. Omitted from JSON when
    /// the node uses none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<PackageSummary>,
}

/// One package as the overview presents it: how often a node's files use it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PackageSummary {
    /// Third party or standard library. Sorts first so third party packages lead.
    pub kind: PackageKind,
    /// The package, see [`crate::code::PackageUse::name`].
    pub name: String,
    /// Number of uses, in production and test code.
    pub count: usize,
    /// How many of `count` are written in test code.
    pub tests: usize,
}

/// One edge as the overview presents it: declared, observed in code, or both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdgeSummary {
    /// The importing node.
    pub from: NodeId,
    /// The imported node.
    pub to: NodeId,
    /// The declared kind, or `None` when code uses the edge and the contract does not declare it.
    pub kind: Option<EdgeKind>,
    /// Where the edge stands, the same status `lint` reports a finding for (see "Edge status" in
    /// `docs/DESIGN.md`). `undeclared` exactly when `kind` is `None`.
    pub status: EdgeStatus,
    /// Number of import sites in code that realise the edge.
    pub realised: usize,
    /// Every import site that realises the edge, in sorted order.
    pub sites: Vec<Site>,
    /// Every symbol the imports behind the edge use, over all sites, sorted with `*` last. Empty
    /// when nothing uses the edge or every import is a side-effect import.
    pub symbols: BTreeSet<Symbol>,
    /// Whether some import behind the edge uses the target module itself rather than a named
    /// symbol (`use crate::gitlab;`, `from x import sub`, `import './x'`).
    pub module: bool,
    /// Whether the edge is declared solid and unlabelled and every one of its sites, at least one,
    /// is type-only: it could be declared `-->|type|`. Omitted from JSON when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub type_candidate: bool,
    /// The boundary edge that allows the pair when it wins the precedence of "Boundary edges", as
    /// opposed to a node edge or no edge. Omitted from JSON when none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<ViaEdge>,
}

/// A boundary edge as `via` names it: its text and its line in the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViaEdge {
    /// The edge as written, `features --> core`.
    pub edge: String,
    /// Its line in the contract.
    pub line: u32,
}

/// One boundary edge as the overview presents it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoundaryEdgeSummary {
    /// The importing end.
    pub from: Endpoint,
    /// The imported end.
    pub to: Endpoint,
    /// The declared kind.
    pub kind: EdgeKind,
    /// Where the boundary edge stands over all the pairs it covers.
    pub status: EdgeStatus,
    /// Its line in the contract.
    pub line: u32,
    /// How many pairs of nodes it covers.
    pub covers: usize,
    /// How many of those pairs code realises with a site the edge allows.
    pub realised: usize,
}

/// The whole architecture at a glance: every node and every declared or observed edge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Overview {
    /// Declared nodes, sorted by id.
    pub nodes: Vec<NodeSummary>,
    /// Declared and observed edges, sorted by `(from, to)`.
    pub edges: Vec<EdgeSummary>,
    /// The boundary edges in file order. Omitted from JSON when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub boundary_edges: Vec<BoundaryEdgeSummary>,
    /// Findings of error severity in the lint run behind this overview.
    pub errors: usize,
    /// Findings of warning severity in the lint run behind this overview.
    pub warnings: usize,
    /// Everything the analysis could not see, as counts.
    pub blind_spots: BlindSummary,
    /// The blind spots no node owns, as counts.
    pub unmapped: BlindTally,
    /// The blind spots no node owns, each one by name.
    pub unmapped_items: BlindSpots,
    /// Every skipped reference, by reason, sorted: what `brief` lists.
    pub skipped_references: BTreeMap<SkipReason, Vec<BlindRef>>,
    /// The contract's rule directives.
    pub rules: RulesSummary,
}

/// One node with the edges that touch it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeView {
    /// The node itself.
    pub node: NodeSummary,
    /// Edges leaving the node, sorted by target.
    pub outgoing: Vec<EdgeSummary>,
    /// Edges entering the node, sorted by source.
    pub incoming: Vec<EdgeSummary>,
    /// The source files the node owns, sorted.
    pub files: Vec<PathBuf>,
    /// The asset files the node owns, sorted.
    pub asset_files: Vec<PathBuf>,
    /// The excluded files the node owns, sorted.
    pub excluded_files: Vec<PathBuf>,
    /// Every item the node is blind to.
    pub blind_spots: BlindSpots,
    /// The node's interface entries, sorted by site count (most first), then file and symbol.
    /// `Some` only when the view shows them: the node has an `interface` limit or the command was
    /// given `--all-sites`.
    pub interface: Option<Vec<InterfaceEntry>>,
}
