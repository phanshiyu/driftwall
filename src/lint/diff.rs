//! Compares two versions of the contract, for reviewers of a contract change.

use crate::diagram;
use crate::model::{Contract, ContractDiff, Edge, EdgeKind};

use super::LintError;

/// The contract that declares nothing. Stands in for a contract that did not exist yet.
pub const EMPTY_CONTRACT: &str = "flowchart LR\n";

/// Parses both contract texts and reports what the new one adds, removes and changes.
///
/// Pure: both texts come from the caller. Only nodes and edge kinds are compared; labels and
/// boundaries are display only. Every list is sorted.
///
/// # Errors
/// [`LintError::OldContract`] or [`LintError::NewContract`] when a text is not a valid contract.
pub fn contract_diff(old_src: &str, new_src: &str) -> Result<ContractDiff, LintError> {
    let old = diagram::parse(old_src).map_err(|source| LintError::OldContract { source })?;
    let new = diagram::parse(new_src).map_err(|source| LintError::NewContract { source })?;
    Ok(diff(&old, &new))
}

fn diff(old: &Contract, new: &Contract) -> ContractDiff {
    let edges_only_in = |from: &Contract, other: &Contract| -> Vec<(Edge, EdgeKind)> {
        from.edges
            .iter()
            .filter(|(edge, _)| !other.edges.contains_key(*edge))
            .map(|(edge, kind)| (edge.clone(), *kind))
            .collect()
    };
    ContractDiff {
        added_nodes: new
            .nodes
            .keys()
            .filter(|id| !old.nodes.contains_key(*id))
            .cloned()
            .collect(),
        removed_nodes: old
            .nodes
            .keys()
            .filter(|id| !new.nodes.contains_key(*id))
            .cloned()
            .collect(),
        added_edges: edges_only_in(new, old),
        removed_edges: edges_only_in(old, new),
        changed_edges: old
            .edges
            .iter()
            .filter_map(|(edge, old_kind)| {
                let new_kind = new.edges.get(edge)?;
                (old_kind != new_kind).then(|| (edge.clone(), *old_kind, *new_kind))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::fixtures::{edge, node};

    const BASE: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a --> b\n  b -.-> c\n";

    fn diff_of(old: &str, new: &str) -> ContractDiff {
        contract_diff(old, new).unwrap()
    }

    #[test]
    fn identical_contracts_have_an_empty_diff() {
        assert!(diff_of(BASE, BASE).is_empty());
    }

    #[test]
    fn labels_boundaries_and_order_are_not_changes() {
        let reordered = "flowchart LR\n  subgraph g[\"G\"]\n    c[\"renamed\"]\n  end\n  b\n  a\n  b -.-> c\n  a -->|why| b\n";
        assert!(diff_of(BASE, reordered).is_empty());
    }

    #[test]
    fn added_node_is_reported() {
        let new = format!("{BASE}  d[\"d\"]\n");
        let d = diff_of(BASE, &new);
        assert_eq!(d.added_nodes, vec![node("d")]);
        assert!(d.removed_nodes.is_empty() && d.added_edges.is_empty());
    }

    #[test]
    fn removed_node_and_its_edges_are_reported() {
        let new = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";
        let d = diff_of(BASE, new);
        assert_eq!(d.removed_nodes, vec![node("c")]);
        assert_eq!(d.removed_edges, vec![(edge("b", "c"), EdgeKind::Planned)]);
        assert!(d.added_nodes.is_empty() && d.changed_edges.is_empty());
    }

    #[test]
    fn added_edge_keeps_its_kind() {
        let new = format!("{BASE}  a -.-> c\n  c --> a\n");
        let d = diff_of(BASE, &new);
        assert_eq!(
            d.added_edges,
            vec![
                (edge("a", "c"), EdgeKind::Planned),
                (edge("c", "a"), EdgeKind::Implemented)
            ]
        );
        assert!(d.removed_edges.is_empty() && d.changed_edges.is_empty());
    }

    #[test]
    fn removed_edge_keeps_its_kind() {
        let new = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  b -.-> c\n";
        let d = diff_of(BASE, new);
        assert_eq!(
            d.removed_edges,
            vec![(edge("a", "b"), EdgeKind::Implemented)]
        );
        assert!(d.added_edges.is_empty() && d.changed_edges.is_empty());
    }

    #[test]
    fn planned_to_implemented_is_a_change_not_an_add_and_remove() {
        let new = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a --> b\n  b --> c\n";
        let d = diff_of(BASE, new);
        assert_eq!(
            d.changed_edges,
            vec![(edge("b", "c"), EdgeKind::Planned, EdgeKind::Implemented)]
        );
        assert!(d.added_edges.is_empty() && d.removed_edges.is_empty());
    }

    #[test]
    fn implemented_to_planned_is_a_change() {
        let new = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a -.-> b\n  b -.-> c\n";
        let d = diff_of(BASE, new);
        assert_eq!(
            d.changed_edges,
            vec![(edge("a", "b"), EdgeKind::Implemented, EdgeKind::Planned)]
        );
    }

    #[test]
    fn direction_matters() {
        let new = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  b --> a\n  b -.-> c\n";
        let d = diff_of(BASE, new);
        assert_eq!(d.added_edges, vec![(edge("b", "a"), EdgeKind::Implemented)]);
        assert_eq!(
            d.removed_edges,
            vec![(edge("a", "b"), EdgeKind::Implemented)]
        );
    }

    #[test]
    fn empty_contract_makes_everything_added() {
        let d = diff_of(EMPTY_CONTRACT, BASE);
        assert_eq!(d.added_nodes, vec![node("a"), node("b"), node("c")]);
        assert_eq!(d.added_edges.len(), 2);
        assert!(d.removed_nodes.is_empty() && d.removed_edges.is_empty());
        let back = diff_of(BASE, EMPTY_CONTRACT);
        assert_eq!(back.removed_nodes.len(), 3);
        assert_eq!(back.removed_edges.len(), 2);
    }

    #[test]
    fn result_is_sorted_and_deterministic() {
        let new = "flowchart LR\n  z\n  m\n  m --> z\n  z --> m\n";
        let first = diff_of(EMPTY_CONTRACT, new);
        assert_eq!(first.added_nodes, vec![node("m"), node("z")]);
        assert_eq!(first.added_edges[0].0, edge("m", "z"));
        assert_eq!(first, diff_of(EMPTY_CONTRACT, new));
    }

    #[test]
    fn an_invalid_old_contract_names_the_old_side() {
        let error = contract_diff("not a diagram", BASE).unwrap_err();
        assert!(matches!(error, LintError::OldContract { .. }), "{error:?}");
    }

    #[test]
    fn an_invalid_new_contract_names_the_new_side() {
        let error = contract_diff(BASE, "").unwrap_err();
        assert!(matches!(error, LintError::NewContract { .. }), "{error:?}");
    }
}
