//! Extracts imports with tree-sitter and resolves them to repository files.
//!
//! Extraction is driven by the per-language table in [`spec`]: a grammar, a query capturing import
//! specifiers, and a resolver. TypeScript and JavaScript are filled in; other languages yield no
//! imports until their table entries exist.
//!
//! Known limitation: only relative specifiers are resolved. Bare packages, scoped packages,
//! `node:` builtins and aliases such as `@/x` are ignored, because `tsconfig` `paths` support is
//! not implemented yet. Import types (`typeof import('./a')`) are not extracted.

mod error;
mod extract;
mod queries;
mod resolve;
mod spec;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::model::{Language, ResolvedImport, UnresolvedImport};

pub use error::ImportsError;
pub use extract::extract;
pub use spec::{LanguageSpec, spec};

/// The imports found in one file. Both lists are sorted and free of duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileImports {
    /// Imports that resolved to a repository file.
    pub resolved: Vec<ResolvedImport>,
    /// Relative imports that matched no repository file.
    pub unresolved: Vec<UnresolvedImport>,
}

/// What a language resolver decided about one specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The specifier names this repository file.
    Resolved(PathBuf),
    /// The specifier points inside the repository but matches no file.
    Unresolved,
    /// Not a repository import: a third party package, a builtin, or a non-source asset.
    Ignored,
}

/// Reads and extracts every file in `files`, relative to `root`.
///
/// The key set of `files` is the set of known repository files used for resolution. When several
/// files fail, the error for the first path in sorted order is returned.
///
/// # Errors
/// Returns [`ImportsError::Io`] for a file that cannot be read or is not UTF-8, and the grammar
/// and query errors of [`extract`].
pub fn collect(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> Result<BTreeMap<PathBuf, FileImports>, ImportsError> {
    let known: BTreeSet<PathBuf> = files.keys().cloned().collect();
    let entries: Vec<(&PathBuf, &Language)> = files.iter().collect();
    // why: collected into an indexed Vec so the reported error never depends on thread scheduling.
    let results: Vec<Result<(PathBuf, FileImports), ImportsError>> = entries
        .par_iter()
        .map(|&(path, &language)| {
            let source =
                fs::read_to_string(root.join(path)).map_err(|source| ImportsError::Io {
                    path: path.clone(),
                    source,
                })?;
            let imports = extract(&source, language, path, &known)?;
            Ok((path.clone(), imports))
        })
        .collect();
    results.into_iter().collect()
}
