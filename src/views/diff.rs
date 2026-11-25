//! Compares two versions of the contract, for reviewers of a contract change.

use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{Contract, Edge, EdgeEnds, EdgeKind, NodeId};
use crate::diagram;
use crate::results::{BoundaryMove, ContractDiff};

use super::ViewsError;

/// The contract that declares nothing. Stands in for a contract that did not exist yet.
pub const EMPTY_CONTRACT: &str = "flowchart LR\n";

/// Parses both contract texts and reports what the new one adds, removes and changes.
///
/// Pure: both texts come from the caller. Nodes, edge kinds and exported names are compared;
/// labels are display only. Rule directive lines and the boundaries around a node are compared as
/// well. Every list is sorted. `moved` is left empty: it needs the files, not just the texts.
///
/// # Errors
/// [`ViewsError::OldContract`] or [`ViewsError::NewContract`] when a text is not a valid contract.
pub fn contract_diff(old_src: &str, new_src: &str) -> Result<ContractDiff, ViewsError> {
    let old = diagram::parse(old_src).map_err(|source| ViewsError::OldContract { source })?;
    let new = diagram::parse(new_src).map_err(|source| ViewsError::NewContract { source })?;
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
    let kinds_of = |contract: &Contract| -> BTreeMap<EdgeEnds, EdgeKind> {
        contract
            .boundary_edges
            .iter()
            // why: templates are rules, which `diff` does not model, and so are their expansions.
            .filter(|declared| declared.template.is_none())
            .map(|declared| (declared.ends(), declared.kind))
            .collect()
    };
    let (old_ends, new_ends) = (kinds_of(old), kinds_of(new));
    let ends_only_in = |from: &BTreeMap<EdgeEnds, EdgeKind>,
                        other: &BTreeMap<EdgeEnds, EdgeKind>|
     -> Vec<(EdgeEnds, EdgeKind)> {
        from.iter()
            .filter(|(ends, _)| !other.contains_key(*ends))
            .map(|(ends, kind)| (ends.clone(), *kind))
            .collect()
    };
    ContractDiff {
        added_boundary_edges: ends_only_in(&new_ends, &old_ends),
        removed_boundary_edges: ends_only_in(&old_ends, &new_ends),
        changed_boundary_edges: old_ends
            .iter()
            .filter_map(|(ends, old_kind)| {
                let new_kind = new_ends.get(ends)?;
                (old_kind != new_kind).then(|| (ends.clone(), *old_kind, *new_kind))
            })
            .collect(),
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
        added_exports: names_only_in(new, old),
        removed_exports: names_only_in(old, new),
        rules_added: rule_lines(new)
            .difference(&rule_lines(old))
            .cloned()
            .collect(),
        rules_removed: rule_lines(old)
            .difference(&rule_lines(new))
            .cloned()
            .collect(),
        boundary_moves: boundary_moves(old, new),
        moved: Vec::new(),
    }
}

/// The directive keywords whose rendered lines `diff` compares. `exports` and `map` are left out:
/// surfaces have their own lines and a map change shows as the files it moves.
const RULE_KEYWORDS: [&str; 10] = [
    "order",
    "isolated",
    "no-cycles",
    "tests",
    "facade",
    "each",
    "budget",
    "packages",
    "forbid-packages",
    "deny",
];

/// The rule directive lines of `contract` as `render` writes them, trimmed.
// why: the rendered text is canonical (sorted nodes, one line per rule), so comparing it ignores
// why: spacing and the order of the source lines without a second description of each rule.
fn rule_lines(contract: &Contract) -> BTreeSet<String> {
    diagram::render(contract)
        .lines()
        .map(str::trim)
        .filter(|line| {
            let word = line
                .strip_prefix("%% ")
                .and_then(|rest| rest.split([':', ' ']).next());
            word.is_some_and(|word| RULE_KEYWORDS.contains(&word))
        })
        .map(str::to_owned)
        .collect()
}

/// The nodes both contracts declare that sit in different boundaries, sorted by node.
fn boundary_moves(old: &Contract, new: &Contract) -> Vec<BoundaryMove> {
    let chain = |info: &crate::contract::NodeInfo| {
        let boundaries: Vec<&str> = info.boundaries().collect();
        (!boundaries.is_empty()).then(|| boundaries.join("/"))
    };
    old.nodes
        .iter()
        .filter_map(|(node, old_info)| {
            let (from, to) = (chain(old_info), chain(new.nodes.get(node)?));
            (from != to).then(|| BoundaryMove {
                node: node.clone(),
                from,
                to,
            })
        })
        .collect()
}

/// The `(node, name)` pairs that `from` exports and `other` does not, sorted. A node with no
/// surface exports nothing, so its absence and an empty surface are the same.
fn names_only_in(from: &Contract, other: &Contract) -> Vec<(NodeId, String)> {
    let none = BTreeSet::new();
    from.surfaces
        .iter()
        .flat_map(|(node, names)| {
            let theirs = other.surfaces.get(node).unwrap_or(&none);
            names
                .difference(theirs)
                .map(|name| (node.clone(), name.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::fixtures::{edge, node};

    const BASE: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a --> b\n  b -.-> c\n";

    fn diff_of(old: &str, new: &str) -> ContractDiff {
        contract_diff(old, new).unwrap()
    }

    #[test]
    fn identical_contracts_have_an_empty_diff() {
        assert!(diff_of(BASE, BASE).is_empty());
    }

    #[test]
    fn an_added_and_a_removed_denial_are_rule_changes() {
        let layers = "flowchart LR\n  subgraph up[\"Up\"]\n    a\n  end\n  subgraph down[\"Down\"]\n    b\n    c\n  end\n  %% order open: up > down\n";
        let old = format!("{layers}  %% deny a: b\n");
        let new = format!("{layers}  %% deny a: c\n");
        let d = diff_of(&old, &new);
        assert_eq!(d.rules_added, ["%% deny a: c"]);
        assert_eq!(d.rules_removed, ["%% deny a: b"]);
        assert!(!d.is_empty());
        assert!(diff_of(&old, &old).is_empty());
        assert!(!diff_of(layers, &old).is_empty());
    }

    #[test]
    fn labels_and_order_are_not_changes() {
        let reordered = "flowchart LR\n  c[\"renamed\"]\n  b\n  a\n  b -.-> c\n  a -->|why| b\n";
        assert!(diff_of(BASE, reordered).is_empty());
    }

    #[test]
    fn a_node_entering_a_boundary_is_a_boundary_move() {
        let new =
            "flowchart LR\n  subgraph g[\"G\"]\n    c\n  end\n  a\n  b\n  a --> b\n  b -.-> c\n";
        let d = diff_of(BASE, new);
        assert_eq!(
            d.boundary_moves,
            vec![BoundaryMove {
                node: node("c"),
                from: None,
                to: Some("g".to_owned())
            }]
        );
        assert!(!d.is_empty());
    }

    #[test]
    fn rule_lines_are_compared_as_rendered() {
        let old = "flowchart LR\n  a\n  b\n  %% facade a, b\n  %% budget a: files 3\n";
        let new = "flowchart LR\n  a\n  b\n  %% facade b,   a\n  %% budget a: files 4\n  %% exports a: X\n";
        let d = diff_of(old, new);
        assert_eq!(d.rules_added, vec!["%% budget a: files 4".to_owned()]);
        assert_eq!(d.rules_removed, vec!["%% budget a: files 3".to_owned()]);
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

    fn pair(node_id: &str, name: &str) -> (NodeId, String) {
        (node(node_id), name.to_owned())
    }

    const EXPORTING: &str = "flowchart LR\n  a\n  b\n  %% exports a: X, Y\n";

    #[test]
    fn a_new_surface_lists_every_name_as_added() {
        let d = diff_of("flowchart LR\n  a\n  b\n", EXPORTING);
        assert_eq!(d.added_exports, vec![pair("a", "X"), pair("a", "Y")]);
        assert_eq!(d.removed_exports, Vec::new());
        assert!(!d.is_empty());
    }

    #[test]
    fn a_dropped_surface_lists_every_name_as_removed() {
        let d = diff_of(EXPORTING, "flowchart LR\n  a\n  b\n");
        assert_eq!(d.removed_exports, vec![pair("a", "X"), pair("a", "Y")]);
        assert_eq!(d.added_exports, Vec::new());
    }

    #[test]
    fn a_changed_surface_lists_only_the_names_that_differ() {
        let new = "flowchart LR\n  a\n  b\n  %% exports a: Y, Z\n  %% exports b: W\n";
        let d = diff_of(EXPORTING, new);
        assert_eq!(d.added_exports, vec![pair("a", "Z"), pair("b", "W")]);
        assert_eq!(d.removed_exports, vec![pair("a", "X")]);
    }

    #[test]
    fn the_order_of_names_and_lines_is_not_a_change() {
        let new = "flowchart LR\n  b\n  a\n  %% exports a: Y, X\n";
        assert!(diff_of(EXPORTING, new).is_empty());
    }

    #[test]
    fn surfaces_of_added_and_removed_nodes_count() {
        let d = diff_of(
            "flowchart LR\n  a\n  %% exports a: X\n",
            "flowchart LR\n  b\n  %% exports b: Y\n",
        );
        assert_eq!(d.removed_exports, vec![pair("a", "X")]);
        assert_eq!(d.added_exports, vec![pair("b", "Y")]);
    }

    #[test]
    fn an_invalid_old_contract_names_the_old_side() {
        let error = contract_diff("not a diagram", BASE).unwrap_err();
        assert!(matches!(error, ViewsError::OldContract { .. }), "{error:?}");
    }

    #[test]
    fn an_invalid_new_contract_names_the_new_side() {
        let error = contract_diff(BASE, "").unwrap_err();
        assert!(matches!(error, ViewsError::NewContract { .. }), "{error:?}");
    }
}
