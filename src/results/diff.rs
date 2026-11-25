//! What changed between two versions of the contract.

use serde::Serialize;

use crate::contract::{Edge, EdgeEnds, EdgeKind, NodeId};

use super::{BoundaryMove, Moved};

/// What changed between two versions of the contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ContractDiff {
    /// Nodes only the new contract declares, sorted.
    pub added_nodes: Vec<NodeId>,
    /// Nodes only the old contract declares, sorted.
    pub removed_nodes: Vec<NodeId>,
    /// Edges only the new contract declares, with their kind, sorted.
    pub added_edges: Vec<(Edge, EdgeKind)>,
    /// Edges only the old contract declares, with their kind, sorted.
    pub removed_edges: Vec<(Edge, EdgeKind)>,
    /// Edges both declare with a different kind, as `(edge, old kind, new kind)`, sorted.
    pub changed_edges: Vec<(Edge, EdgeKind, EdgeKind)>,
    /// Boundary edges only the new contract declares, by their ends, sorted.
    pub added_boundary_edges: Vec<(EdgeEnds, EdgeKind)>,
    /// Boundary edges only the old contract declares, by their ends, sorted.
    pub removed_boundary_edges: Vec<(EdgeEnds, EdgeKind)>,
    /// Boundary edges both declare with a different kind, as `(ends, old kind, new kind)`, sorted.
    pub changed_boundary_edges: Vec<(EdgeEnds, EdgeKind, EdgeKind)>,
    /// Names only the new contract exports, as `(node, name)`, sorted.
    pub added_exports: Vec<(NodeId, String)>,
    /// Names only the old contract exports, as `(node, name)`, sorted. A node that lost its whole
    /// surface lists every name it had: it is unrestricted from then on.
    pub removed_exports: Vec<(NodeId, String)>,
    /// Rule directive lines only the new contract renders, sorted. See "Diff beyond the contract".
    pub rules_added: Vec<String>,
    /// Rule directive lines only the old contract renders, sorted.
    pub rules_removed: Vec<String>,
    /// Nodes both contracts declare whose enclosing boundaries differ, sorted by node.
    pub boundary_moves: Vec<BoundaryMove>,
    /// Source files whose owning node differs from the one at the base ref, sorted by file. Not
    /// filled by the pure contract comparison: the caller adds it from two scans.
    pub moved: Vec<Moved>,
}

impl ContractDiff {
    /// True when the two contracts declare the same nodes, edges with the same kinds, surfaces,
    /// rules and boundaries, and no file changed owner.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
            && self.changed_edges.is_empty()
            && self.added_boundary_edges.is_empty()
            && self.removed_boundary_edges.is_empty()
            && self.changed_boundary_edges.is_empty()
            && self.added_exports.is_empty()
            && self.removed_exports.is_empty()
            && self.rules_added.is_empty()
            && self.rules_removed.is_empty()
            && self.boundary_moves.is_empty()
            && self.moved.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_diff_is_empty_only_when_nothing_changed() {
        let id = NodeId::new("a").unwrap();
        let edge = Edge {
            from: id.clone(),
            to: NodeId::new("b").unwrap(),
        };
        assert!(ContractDiff::default().is_empty());
        let cases = [
            ContractDiff {
                added_nodes: vec![id.clone()],
                ..ContractDiff::default()
            },
            ContractDiff {
                removed_nodes: vec![id],
                ..ContractDiff::default()
            },
            ContractDiff {
                added_edges: vec![(edge.clone(), EdgeKind::Planned)],
                ..ContractDiff::default()
            },
            ContractDiff {
                removed_edges: vec![(edge.clone(), EdgeKind::Planned)],
                ..ContractDiff::default()
            },
            ContractDiff {
                changed_edges: vec![(edge, EdgeKind::Planned, EdgeKind::Implemented)],
                ..ContractDiff::default()
            },
            ContractDiff {
                added_exports: vec![(NodeId::new("a").unwrap(), "X".to_owned())],
                ..ContractDiff::default()
            },
            ContractDiff {
                removed_exports: vec![(NodeId::new("a").unwrap(), "X".to_owned())],
                ..ContractDiff::default()
            },
        ];
        for diff in cases {
            assert!(!diff.is_empty(), "{diff:?}");
        }
    }
}
