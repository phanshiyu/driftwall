//! The re-export table of one file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// What one file defines and re-exports. Built from syntax alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// Name this file exports to the file and name it has there: `pub use x::Y as W;` is
    /// `W` to (`x`'s file, `Y`).
    pub names: BTreeMap<String, (PathBuf, String)>,
    /// The files whose every name this file re-exports with a glob.
    pub stars: Vec<PathBuf>,
    /// The names the file defines itself.
    pub local: BTreeSet<String>,
    /// For Python, the names `__all__` lists, which decide what a glob of this file exports.
    pub all: Option<BTreeSet<String>>,
    /// For Python, whether a glob exports only the names that do not start with `_`.
    pub public_only: bool,
    /// The lines of the re-export statements, which keep their own edge.
    pub lines: BTreeSet<u32>,
}
