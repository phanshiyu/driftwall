//! The edges code actually uses, computed once and shared by [`reconcile`](super::reconcile) and
//! the overview.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::imports::FileImports;
use crate::model::{Contract, Edge, Location, NodeId, Ownership, ResolvedImport};

/// Every resolved import that crosses from one declared owner to another, with its edge.
///
/// An import counts only when both files have an owner, the owners differ, and both owners are
/// declared in the contract. Anything else (an unmapped file, an owner that names an unknown
/// node, an import within one node) is not a crossing. The order follows `imports`.
pub(super) fn crossings<'a>(
    contract: &Contract,
    ownership: &Ownership,
    imports: &'a BTreeMap<PathBuf, FileImports>,
) -> Vec<(Edge, &'a ResolvedImport)> {
    imports
        .values()
        .flat_map(|file_imports| &file_imports.resolved)
        .filter_map(|import| {
            let from = declared_owner(contract, ownership, &import.at.file)?;
            let to = declared_owner(contract, ownership, &import.target)?;
            (from != to).then(|| {
                (
                    Edge {
                        from: from.clone(),
                        to: to.clone(),
                    },
                    import,
                )
            })
        })
        .collect()
}

/// The edges code realises, each with the import sites that realise it.
///
/// Whether the contract declares an edge does not matter: this is what the code does. Sites are
/// sorted and free of duplicates. Pure: the result does not depend on the order of the inputs.
#[must_use]
pub fn observed_edges(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
) -> BTreeMap<Edge, Vec<Location>> {
    let mut observed: BTreeMap<Edge, Vec<Location>> = BTreeMap::new();
    for (edge, import) in crossings(contract, ownership, imports) {
        observed.entry(edge).or_default().push(import.at.clone());
    }
    for sites in observed.values_mut() {
        sites.sort();
        sites.dedup();
    }
    observed
}

/// The owner of `file`, only when that owner is a node the contract declares.
fn declared_owner<'a>(
    contract: &Contract,
    ownership: &'a Ownership,
    file: &Path,
) -> Option<&'a NodeId> {
    ownership
        .owners
        .get(file)
        .filter(|owner| contract.nodes.contains_key(*owner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::fixtures::{at, contract, edge, imports, node, ownership};
    use crate::model::EdgeKind;

    fn owners() -> Ownership {
        ownership(
            &[
                ("a/x.ts", "a"),
                ("a/z.ts", "a"),
                ("b/y.ts", "b"),
                ("c/w.ts", "c"),
            ],
            &[],
        )
    }

    #[test]
    fn groups_import_sites_per_edge_sorted_and_deduped() {
        let found = observed_edges(
            &contract(&["a", "b", "c"], &[]),
            &owners(),
            &imports(&[
                ("a/z.ts", 4, "b/y.ts"),
                ("a/x.ts", 9, "b/y.ts"),
                ("a/x.ts", 2, "b/y.ts"),
                ("a/x.ts", 2, "b/y.ts"),
                ("b/y.ts", 1, "c/w.ts"),
            ]),
        );
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[&edge("a", "b")],
            vec![at("a/x.ts", 2), at("a/x.ts", 9), at("a/z.ts", 4)]
        );
        assert_eq!(found[&edge("b", "c")], vec![at("b/y.ts", 1)]);
    }

    #[test]
    fn declared_kind_does_not_matter() {
        let found = observed_edges(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Planned)]),
            &owners(),
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
        );
        assert_eq!(found.keys().collect::<Vec<_>>(), vec![&edge("a", "b")]);
    }

    #[test]
    fn direction_is_kept() {
        let found = observed_edges(
            &contract(&["a", "b"], &[]),
            &owners(),
            &imports(&[("a/x.ts", 1, "b/y.ts"), ("b/y.ts", 1, "a/x.ts")]),
        );
        assert_eq!(
            found.keys().collect::<Vec<_>>(),
            vec![&edge("a", "b"), &edge("b", "a")]
        );
    }

    #[test]
    fn same_owner_unowned_and_undeclared_owners_are_not_observed() {
        let mut own = owners();
        own.owners.insert("g/g.ts".into(), node("ghost"));
        let found = observed_edges(
            &contract(&["a", "b"], &[]),
            &own,
            &imports(&[
                ("a/x.ts", 1, "a/z.ts"),
                ("a/x.ts", 2, "loose/u.ts"),
                ("loose/u.ts", 3, "a/x.ts"),
                ("a/x.ts", 4, "g/g.ts"),
                ("g/g.ts", 5, "a/x.ts"),
            ]),
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn construction_order_does_not_matter() {
        let own = owners();
        let c = contract(&["a", "b", "c"], &[]);
        let sites = [
            ("a/x.ts", 3, "b/y.ts"),
            ("b/y.ts", 9, "c/w.ts"),
            ("a/z.ts", 5, "b/y.ts"),
        ];
        let mut reversed = sites;
        reversed.reverse();
        assert_eq!(
            observed_edges(&c, &own, &imports(&sites)),
            observed_edges(&c, &own, &imports(&reversed))
        );
    }
}
