//! What `diff` reports beyond nodes and edges: files that changed owner and nodes that changed
//! boundary (see "Diff beyond the contract" in `docs/DESIGN.md`).

use std::path::PathBuf;

use serde::Serialize;

use super::NodeId;

/// A source file whose owning node differs between the base ref and the working tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Moved {
    /// The file's path now, relative to the repository root.
    pub file: PathBuf,
    /// The owner at the base ref, `None` for no owner.
    pub from_node: Option<NodeId>,
    /// The owner now, `None` for no owner.
    pub to_node: Option<NodeId>,
    /// The path the file had at the base ref, when git detects a rename.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<PathBuf>,
    /// Whether the mapping (`%% map` lines or `.arch` tags) would give the path a different owner
    /// than it did at the base ref.
    pub mapping_changed: bool,
}

/// A node that is inside different boundaries in the two contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoundaryMove {
    /// The node.
    pub node: NodeId,
    /// Its boundaries before, outermost first joined by `/`, `None` for no boundary.
    pub from: Option<String>,
    /// Its boundaries now, in the same form.
    pub to: Option<String>,
}
