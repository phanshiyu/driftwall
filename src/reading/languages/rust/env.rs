//! What Rust resolution reads besides the repository files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::cargo::Dependencies;
use super::path_modules::PathModules;

/// The library crates, the placed modules and the declared dependencies of the repository.
#[derive(Debug, Clone, Copy)]
pub struct Env<'a> {
    /// Library crate name, as imported, to the repository-relative directory holding its `lib.rs`.
    pub crates: &'a BTreeMap<String, PathBuf>,
    /// The Rust modules a `#[path = ".."]` attribute placed at a file of its own choosing, see
    /// the module `path_modules`.
    pub paths: &'a PathModules,
    /// Repository-relative directory holding a readable `Cargo.toml` to the dependencies it
    /// lists, as code imports them (`-` written `_`). A file is governed by the nearest directory
    /// at or above it.
    pub dependencies: &'a BTreeMap<PathBuf, Dependencies>,
}

impl Env<'_> {
    /// The dependencies of the nearest manifest at or above `importer`.
    pub fn manifest_of(&self, importer: &Path) -> Option<&Dependencies> {
        importer
            .ancestors()
            .skip(1)
            .find_map(|dir| self.dependencies.get(dir))
    }

    /// The directory of the library crate `name` stands for in `importer`: a dependency the
    /// nearest manifest renames (`alpha = { package = "a-lib" }`) is the crate it renames, for
    /// that manifest's files only. Any other name is the crate of that name.
    pub fn crate_dir(&self, name: &str, importer: &Path) -> Option<&PathBuf> {
        let renamed = self
            .manifest_of(importer)
            .and_then(|found| found.renames.get(name));
        self.crates.get(renamed.map_or(name, String::as_str))
    }
}
