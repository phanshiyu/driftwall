//! Facade nodes: where an import stops following re-exports, and the stale declaration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::imports::{Facades, FileImports};
use crate::model::{Contract, Location, Ownership, Violation, ViolationKind};

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
        })
        .collect()
}
