//! Facade nodes: where an import stops following re-exports, and the stale declaration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::code::{Location, Ownership};
use crate::contract::Contract;
use crate::reading::imports::{Facades, FileImports};
use crate::results::{Violation, ViolationKind};

use super::rules::violation;

/// The files each `%% facade` node owns, with the node's name (see "Facades" in "Re-exports" in
/// `docs/DESIGN.md`).
pub(super) fn files(contract: &Contract, ownership: &Ownership) -> Facades {
    ownership
        .owners
        .iter()
        .filter(|(_, node)| contract.rules.facades.contains_key(*node))
        .map(|(file, node)| (file.clone(), node.to_string()))
        .collect()
}

/// A `facade-without-exports` per `%% facade` node with no `%% exports` line: the declaration
/// makes callers depend on the facade, and with no surface every name it re-exports is public.
pub(super) fn without_exports(contract: &Contract, contract_file: &Path) -> Vec<Violation> {
    let file = contract_file.display();
    contract
        .rules
        .facades
        .iter()
        .filter(|(node, _)| !contract.surfaces.contains_key(*node))
        .map(|(node, &line)| {
            violation(
                ViolationKind::FacadeWithoutExports,
                None,
                format!(
                    "`%% facade {node}` on line {line} of {file} makes `{node}` a facade, but no `%% exports {node}` line states its surface, so every name it re-exports is public"
                ),
            )
            .keyed(node.to_string())
        })
        .collect()
}

/// A `stale-facade` per `%% facade` node that owns no file re-exporting anything.
pub(super) fn findings(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
    contract_file: &Path,
) -> Vec<Violation> {
    contract
        .rules
        .facades
        .iter()
        .filter(|(node, _)| {
            !ownership.owners.iter().any(|(file, owner)| {
                owner == *node && imports.get(file).is_some_and(|i| i.reexports)
            })
        })
        .map(|(node, &line)| {
            violation(
                ViolationKind::StaleFacade,
                Some(Location {
                    file: contract_file.to_path_buf(),
                    line,
                }),
                format!(
                    "`%% facade {node}` on line {line} names a node whose files re-export nothing"
                ),
            )
            .keyed(node.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::parse;

    const NODES: &str = "flowchart LR\n  billing\n  orders\n  search\n";

    fn found(extra: &str) -> Vec<Violation> {
        let contract = parse(&format!("{NODES}{extra}")).unwrap();
        without_exports(&contract, Path::new("architecture.mmd"))
    }

    #[test]
    fn a_facade_without_a_surface_warns_with_its_line_and_no_location() {
        let found = found("  %% facade billing\n");
        let [one] = found.as_slice() else {
            panic!("one finding expected");
        };
        assert_eq!(one.kind, ViolationKind::FacadeWithoutExports);
        assert_eq!(one.at, None);
        assert_eq!(one.key, "billing");
        assert_eq!(
            one.message,
            "`%% facade billing` on line 5 of architecture.mmd makes `billing` a facade, but no `%% exports billing` line states its surface, so every name it re-exports is public"
        );
    }

    #[test]
    fn a_surface_silences_it_and_each_node_is_its_own_finding() {
        let silenced = found("  %% facade billing\n  %% exports billing: Invoice\n");
        assert_eq!(silenced.len(), 0);
        let extra = "  %% facade billing, orders\n  %% facade search\n  %% exports orders: Order\n";
        let keys: Vec<String> = found(extra)
            .into_iter()
            .map(|violation| violation.key)
            .collect();
        assert_eq!(keys, ["billing", "search"]);
    }
}
