//! The edges code actually uses, computed once and shared by [`reconcile`](super::reconcile) and
//! the overview.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::code::{ImportKind, Ownership, ResolvedImport, SkipReason, Skipped, Symbol, Via};
use crate::contract::{Contract, Edge, NodeId};
use crate::imports::FileImports;
use crate::results::Site;

/// Every resolved import that crosses from one declared owner to another, with its edge.
///
/// An import counts only when both files have an owner, the owners differ, and both owners are
/// declared in the contract. Anything else (an unmapped file, an owner that names an unknown
/// node, an import within one node) is not a crossing. An asset or an excluded file has an owner
/// like a source file, so an import of one owned by another node is a crossing. The order follows `imports`.
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

/// Makes every Vite alias whose target is a build output inside a node's directory a dependency on
/// that node (see "Vite aliases" in `docs/DESIGN.md`).
///
/// For each [`SkipReason::AliasTargetMissing`] reference whose target `owner` places in a node the
/// contract declares, a [`ResolvedImport`] with [`Via::AliasToBuildOutput`] is added to the file's
/// imports. The reference stays in `skipped`. Returns `ownership` with those target paths owned by
/// their node, which is what [`crossings`] needs to see the import; the scanned ownership is left
/// alone.
pub(super) fn alias_to_build_outputs(
    contract: &Contract,
    ownership: &Ownership,
    imports: &mut BTreeMap<PathBuf, FileImports>,
    owner: impl Fn(&Path) -> Option<NodeId>,
) -> Ownership {
    let mut reach = ownership.clone();
    for found in imports.values_mut() {
        let added: Vec<ResolvedImport> = found
            .skipped
            .iter()
            .filter(|reference| reference.reason == SkipReason::AliasTargetMissing)
            .filter_map(|reference| {
                let target = reference.target.as_ref()?;
                let node = owner(target).filter(|node| contract.nodes.contains_key(node))?;
                reach.owners.insert(target.clone(), node);
                Some(ResolvedImport {
                    at: reference.at.clone(),
                    target: target.clone(),
                    symbols: BTreeSet::new(),
                    kind: ImportKind::Runtime,
                    test_only: reference.test_only,
                    via: Some(Via::AliasToBuildOutput),
                })
            })
            .collect();
        if !added.is_empty() {
            found.resolved.extend(added);
            found.resolved.sort();
            found.resolved.dedup();
        }
    }
    reach
}

/// The imports of an asset or an excluded file whose owner is not a node the contract declares, as
/// skipped references.
///
/// Such an import is no crossing (see [`crossings`]), but it must not vanish: it is recorded when
/// the importing file is owned by a declared node. The specifier is the repository path of the
/// target and the message names it and its owner. Sorted and free of duplicates.
pub(super) fn undeclared_owners(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
) -> Vec<Skipped> {
    let found: BTreeSet<Skipped> = imports
        .values()
        .flat_map(|file_imports| &file_imports.resolved)
        .filter_map(|import| {
            declared_owner(contract, ownership, &import.at.file)?;
            let (kind, owner) = ownership
                .asset_owners
                .get(&import.target)
                .map(|owner| ("asset", owner))
                .or_else(|| {
                    ownership
                        .excluded_owners
                        .get(&import.target)
                        .map(|owner| ("excluded file", owner))
                })?;
            if contract.nodes.contains_key(owner) {
                return None;
            }
            let asset = import.target.display();
            Some(Skipped {
                at: import.at.clone(),
                specifier: asset.to_string(),
                reason: SkipReason::UndeclaredOwner,
                target: None,
                test_only: false,
                message: Some(format!(
                    "{kind} `{asset}` is owned by `{owner}`, which the contract does not declare"
                )),
            })
        })
        .collect();
    found.into_iter().collect()
}

/// The edges code realises, each with the import sites that realise it.
///
/// Whether the contract declares an edge does not matter: this is what the code does. Sites are
/// sorted and free of duplicates, and each carries its kind and test flag: two imports at one
/// location with different kinds are two sites. Pure: the result does not depend on the order of
/// the inputs.
#[must_use]
pub fn observed_edges(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
) -> BTreeMap<Edge, Vec<Site>> {
    let mut observed: BTreeMap<Edge, Vec<Site>> = BTreeMap::new();
    for (edge, import) in crossings(contract, ownership, imports) {
        observed.entry(edge).or_default().push(Site::of(import));
    }
    for sites in observed.values_mut() {
        sites.sort();
        sites.dedup();
    }
    observed
}

/// The symbols the imports behind each observed edge use, over all of its sites.
///
/// The keys are exactly those of [`observed_edges`]. An edge whose imports are all side-effect
/// imports has an empty set. Pure: the result does not depend on the order of the inputs.
#[must_use]
pub fn observed_symbols(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
) -> BTreeMap<Edge, BTreeSet<Symbol>> {
    let mut used: BTreeMap<Edge, BTreeSet<Symbol>> = BTreeMap::new();
    for (edge, import) in crossings(contract, ownership, imports) {
        used.entry(edge)
            .or_default()
            .extend(import.symbols.iter().cloned());
    }
    used
}

/// The owner of `file`, a source file, an asset or an excluded file, only when that owner is a node
/// the contract declares.
pub(super) fn declared_owner<'a>(
    contract: &Contract,
    ownership: &'a Ownership,
    file: &Path,
) -> Option<&'a NodeId> {
    ownership
        .owners
        .get(file)
        .or_else(|| ownership.asset_owners.get(file))
        .or_else(|| ownership.excluded_owners.get(file))
        .filter(|owner| contract.nodes.contains_key(*owner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::Location;
    use crate::contract::EdgeKind;
    use crate::lint::fixtures::{at, contract, edge, imports, imports_using, node, ownership};

    fn places(sites: &[Site]) -> Vec<Location> {
        sites.iter().map(|site| site.at.clone()).collect()
    }

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
            places(&found[&edge("a", "b")]),
            vec![at("a/x.ts", 2), at("a/x.ts", 9), at("a/z.ts", 4)]
        );
        assert_eq!(places(&found[&edge("b", "c")]), vec![at("b/y.ts", 1)]);
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

    fn names(symbols: &BTreeSet<Symbol>) -> Vec<&str> {
        symbols.iter().map(Symbol::as_str).collect()
    }

    #[test]
    fn symbols_are_the_union_over_every_site_of_an_edge_with_all_last() {
        let used = observed_symbols(
            &contract(&["a", "b", "c"], &[]),
            &owners(),
            &imports_using(&[
                ("a/x.ts", 1, "b/y.ts", &["*", "zed"]),
                ("a/z.ts", 2, "b/y.ts", &["alpha", "zed"]),
                ("b/y.ts", 3, "c/w.ts", &["only"]),
            ]),
        );
        assert_eq!(names(&used[&edge("a", "b")]), ["alpha", "zed", "*"]);
        assert_eq!(names(&used[&edge("b", "c")]), ["only"]);
    }

    #[test]
    fn symbols_cover_the_observed_edges_and_a_side_effect_import_adds_none() {
        let c = contract(&["a", "b", "c"], &[]);
        let sites = [
            ("a/x.ts", 1, "b/y.ts", &[][..]),
            ("a/x.ts", 2, "a/z.ts", &["same_node"][..]),
            ("a/x.ts", 3, "loose.ts", &["unowned"][..]),
        ];
        let used = observed_symbols(&c, &owners(), &imports_using(&sites));
        let observed = observed_edges(&c, &owners(), &imports_using(&sites));
        assert_eq!(
            used.keys().collect::<Vec<_>>(),
            observed.keys().collect::<Vec<_>>()
        );
        assert!(used[&edge("a", "b")].is_empty());
    }

    #[test]
    fn symbols_do_not_depend_on_construction_order() {
        let c = contract(&["a", "b"], &[]);
        let sites = [
            ("a/x.ts", 3, "b/y.ts", &["one"][..]),
            ("a/z.ts", 5, "b/y.ts", &["two"][..]),
        ];
        let mut reversed = sites;
        reversed.reverse();
        assert_eq!(
            observed_symbols(&c, &owners(), &imports_using(&sites)),
            observed_symbols(&c, &owners(), &imports_using(&reversed))
        );
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
