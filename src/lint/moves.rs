//! Who owned each source file in two trees, and which files changed owner between them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::{Contract, MapRule, MoveCause, Moved, NodeId, Ownership};
use crate::ownership::{self, MapContext};

use super::{LintError, read_contract, scan_ownership};

/// The owner of every source file of one tree, with the rules that decided it.
#[derive(Debug)]
pub struct Owners {
    /// Every source file with its owner, `None` when nothing owns it or it is a folded entry file.
    files: BTreeMap<PathBuf, Option<NodeId>>,
    ownership: Ownership,
    contract: PathBuf,
    maps: Vec<MapRule>,
}

impl Owners {
    /// The node this tree's rules give `path`, whether or not the file exists here.
    fn rule_owner(&self, path: &Path) -> Option<NodeId> {
        if self.maps.is_empty() {
            ownership::owner_in(&self.ownership.tag_dirs, path)
        } else {
            let context = MapContext {
                contract: &self.contract,
                rules: &self.maps,
            };
            ownership::Mapper::new(context).owner(path, &self.ownership.tag_dirs)
        }
    }
}

/// Who owns each source file of the tree at `root`, exactly as a lint of that tree would decide.
///
/// With `declared` the contract at `contract` is read and its `%% map` lines apply; without, the
/// tree has no contract and its `.arch` files decide.
///
/// # Errors
/// The contract cannot be read or parsed, or the scan fails.
pub fn owners(root: &Path, contract: &Path, declared: bool) -> Result<Owners, LintError> {
    let parsed = if declared {
        read_contract(root, contract)?
    } else {
        Contract::default()
    };
    let ownership = scan_ownership(root, contract, &parsed)?;
    let files = ownership
        .owners
        .iter()
        .map(|(path, node)| (path.clone(), Some(node.clone())))
        .chain(ownership.unmapped.iter().map(|path| (path.clone(), None)))
        .chain(ownership.folded.iter().map(|path| (path.clone(), None)))
        .collect();
    Ok(Owners {
        files,
        ownership,
        contract: contract.to_path_buf(),
        maps: parsed.maps,
    })
}

/// The files whose owner differs between `old` and `new`, sorted by file. `renames` are the
/// `(old path, new path)` pairs git detected between the two trees.
///
/// A path in both trees is a move when its owner changed (the mapping did). A renamed file is a
/// move when the new path's owner differs from the old path's, and the mapping changed too when the
/// old rules would have given the new path another owner. Files only in one tree are not moves.
#[must_use]
pub fn moved(old: &Owners, new: &Owners, renames: &[(PathBuf, PathBuf)]) -> Vec<Moved> {
    let origins: BTreeMap<&PathBuf, &PathBuf> =
        renames.iter().map(|(from, to)| (to, from)).collect();
    new.files
        .iter()
        .filter_map(|(path, to_node)| {
            let from_path = if old.files.contains_key(path) {
                path
            } else {
                *origins.get(path)?
            };
            let from_node = old.files.get(from_path)?;
            (from_node != to_node).then(|| Moved {
                file: path.clone(),
                from_node: from_node.clone(),
                to_node: to_node.clone(),
                cause: MoveCause::of(from_path, path, old.rule_owner(path) != *to_node),
            })
        })
        .collect()
}
