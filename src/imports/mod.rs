//! Extracts imports with tree-sitter and resolves them to repository files.
//!
//! Extraction is driven by the per-language table in [`spec`]: a grammar, a query capturing import
//! specifiers, and a resolver. TypeScript, JavaScript, Python and Rust are filled in.
//!
//! Known limitation: for TypeScript and JavaScript only relative specifiers are resolved. Bare
//! packages, scoped packages, `node:` builtins and aliases such as `@/x` are ignored, because
//! `tsconfig` `paths` support is not implemented yet. Import types (`typeof import('./a')`) are
//! not extracted. For Python, `__import__`, `importlib.import_module`, `.pyi` stubs and import
//! roots other than the repository root and `src/` are not supported. For Rust, see `rust`'s
//! limitations: `#[path]` modules, `extern crate` renames and relative paths that start with a
//! local module name are not followed.

mod error;
mod extract;
mod manifest;
mod python;
mod queries;
mod resolve;
mod rust;
mod rust_module;
mod rust_syntax;
mod spec;

#[cfg(test)]
mod python_tests;
#[cfg(test)]
mod rust_dogfood_tests;
#[cfg(test)]
mod rust_tests;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::model::{Language, ResolvedImport, UnresolvedImport};

pub use error::ImportsError;
pub use extract::{extract, extract_in};
pub use manifest::library_crates;
pub use spec::{LanguageSpec, spec};

/// The imports found in one file. Both lists are sorted and free of duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileImports {
    /// Imports that resolved to a repository file.
    pub resolved: Vec<ResolvedImport>,
    /// Relative imports that matched no repository file.
    pub unresolved: Vec<UnresolvedImport>,
}

/// What a resolver may consult besides the importing file: every known repository file and the
/// library crates of the repository. Built once per run, shared by every file.
#[derive(Debug, Clone, Copy)]
pub struct Environment<'a> {
    /// Repository-relative paths of every repository file.
    pub known: &'a BTreeSet<PathBuf>,
    /// Library crate name, as imported, to the repository-relative directory holding its `lib.rs`.
    /// Only Rust resolution reads it.
    pub crates: &'a BTreeMap<String, PathBuf>,
}

/// One query match handed to a language resolver: the module reference as written and, for
/// languages whose statements import several things from one module, the imported names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference<'a> {
    /// The text of the `@specifier` capture. For string-based languages the string contents.
    pub specifier: &'a str,
    /// The text of every `@name` capture of the same match, in source order. Usually empty.
    pub names: &'a [&'a str],
    /// The syntax node of the `@specifier` capture, for resolvers that need its surroundings.
    pub node: tree_sitter::Node<'a>,
    /// The source text `node` was parsed from.
    pub source: &'a str,
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
/// The key set of `files` is the set of known repository files used for resolution. Library
/// crates are discovered once from the `Cargo.toml` files next to the known `lib.rs` files. When several
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
    let crates = library_crates(root, &known);
    let env = Environment {
        known: &known,
        crates: &crates,
    };
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
            let imports = extract_in(&source, language, path, &env)?;
            Ok((path.clone(), imports))
        })
        .collect();
    results.into_iter().collect()
}
