//! The imports found in one file.

use crate::model::{PackageUse, ResolvedImport, Skipped, UnresolvedImport};

/// The imports found in one file. Every list is sorted and free of duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileImports {
    /// Imports that resolved to a repository file.
    pub resolved: Vec<ResolvedImport>,
    /// Relative imports that matched no repository file.
    pub unresolved: Vec<UnresolvedImport>,
    /// References that cannot be followed: undeclared packages, computed specifiers and embeds of
    /// non-source files. Never a lint finding, see `SkipReason`.
    pub skipped: Vec<Skipped>,
    /// Whether the file is test code by its name, its place, or a `#[cfg(test)] mod` declaration
    /// that pulls it in (see "Import kind" in `docs/DESIGN.md`).
    pub test_file: bool,
    /// Imports of code outside the repository, see "Package uses" in `docs/DESIGN.md`.
    pub packages: Vec<PackageUse>,
    /// Whether the file re-exports something from another repository file, see "Re-exports" in
    /// `docs/DESIGN.md`. Set by [`collect_with_facades`](super::collect_with_facades).
    pub reexports: bool,
}

#[cfg(test)]
impl FileImports {
    /// These imports without their package uses, for tests of what is an import or a skip.
    pub(crate) fn without_packages(self) -> Self {
        Self {
            packages: Vec::new(),
            ..self
        }
    }
}
