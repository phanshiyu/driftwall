//! What Rust resolution reads besides the repository files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::path_modules::PathModules;

/// The library crates, the placed modules and the declared dependencies of the repository.
#[derive(Debug, Clone, Copy)]
pub struct Env<'a> {
    /// Library crate name, as imported, to the repository-relative directory holding its `lib.rs`.
    pub crates: &'a BTreeMap<String, PathBuf>,
    /// The Rust modules a `#[path = ".."]` attribute placed at a file of its own choosing, see
    /// the module `path_modules`.
    pub paths: &'a PathModules,
    /// Repository-relative directory holding a readable `Cargo.toml` to the crate names it lists
    /// as dependencies, as code imports them (`-` written `_`). A file is governed by the nearest
    /// directory at or above it.
    pub dependencies: &'a BTreeMap<PathBuf, BTreeSet<String>>,
}
