//! Reading and extracting every file of a repository, then following re-exports.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::environment::Environment;
use super::extract::extract_in;
use super::{Facades, FileImports, ImportsError, reexport};
use crate::code::Language;
use crate::reading::languages::python::{PythonLayout, python_roots};
use crate::reading::languages::rust::{self, cargo_dependencies, library_crates};
use crate::reading::languages::swift::{SwiftModules, swift_targets};
use crate::reading::languages::web::{
    self, alias_configs, package_dependencies, project_configs, vite_aliases, vite_roots,
    workspaces,
};
use crate::reading::syntax::resolution::Files;
use crate::reading::syntax::table::Table;

/// A file's path, its imports and its re-export table when it may have one.
type Collected = (PathBuf, FileImports, Option<Table>);

/// [`collect_with_assets`] when no asset or excluded file is known, so an import of a non-source
/// file is unresolved.
///
/// # Errors
/// The same as [`collect_with_assets`].
#[cfg(test)]
pub fn collect(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> Result<BTreeMap<PathBuf, FileImports>, ImportsError> {
    collect_with_assets(
        root,
        files,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
}

/// Reads and extracts every file in `files`, relative to `root`.
///
/// The key set of `files` is the set of known repository files used for resolution, and `assets`
/// the set of repository files that are not source, which a TypeScript or JavaScript import can
/// also resolve to, and a Rust embed too unless it is in `unowned`. `excluded` is the set of source files the scan left out (vendored, minified,
/// generated): they are never read, but they are known files, so an import of one resolves to it
/// exactly like an import of any source file. Library
/// crates are discovered once from the `Cargo.toml` files next to the known `lib.rs` files, and
/// import aliases once from the `tsconfig.json` and `jsconfig.json` files at or above the
/// TypeScript and JavaScript files, and declared packages from the `package.json` files there.
/// When several files fail, the error for the first path in sorted order is returned.
///
/// # Errors
/// Returns [`ImportsError::Io`] for a file that cannot be read or is not UTF-8, and the grammar
/// and query errors of `extract`.
pub fn collect_with_assets(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
    assets: &BTreeSet<PathBuf>,
    unowned: &BTreeSet<PathBuf>,
    excluded: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, FileImports>, ImportsError> {
    collect_with_facades(root, files, assets, unowned, excluded, &Facades::new())
}

/// [`collect_with_assets`] with facade files, where re-exports stop being followed.
///
/// `facades` holds each file of a facade node with the node's name: an import from outside that
/// node goes no further than it, see "Facades" in "Re-exports" in `docs/DESIGN.md`.
///
/// # Errors
/// The same as [`collect_with_assets`].
pub fn collect_with_facades(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
    assets: &BTreeSet<PathBuf>,
    unowned: &BTreeSet<PathBuf>,
    excluded: &BTreeSet<PathBuf>,
    facades: &Facades,
) -> Result<BTreeMap<PathBuf, FileImports>, ImportsError> {
    // why: an excluded file is not read, but it is a repository file an import can name.
    let known: BTreeSet<PathBuf> = files.keys().chain(excluded).cloned().collect();
    let crates = library_crates(root, &known);
    let aliases = alias_configs(root, files);
    let packages = package_dependencies(root, files);
    let workspaces = workspaces(root, files);
    let projects = project_configs(root, files);
    let python = PythonLayout::new(&known, python_roots(root, files));
    let swift = SwiftModules::new(swift_targets(root, files));
    let vite = vite_roots(root, files);
    let aliases_of_vite = vite_aliases(root, files);
    let rust_paths = rust::path_modules(root, files, &known);
    let rust_dependencies = cargo_dependencies(root, files);
    let test_files = rust::test_files(root, files, &known, &rust_paths);
    let env = Environment {
        files: Files {
            known: &known,
            assets,
            unowned,
        },
        test_files: &test_files,
        web: web::Env {
            aliases: &aliases,
            packages: &packages,
            workspaces: &workspaces,
            projects: &projects,
            vite_roots: &vite,
            vite_aliases: &aliases_of_vite,
        },
        python: &python,
        rust: rust::Env {
            crates: &crates,
            paths: &rust_paths,
            dependencies: &rust_dependencies,
        },
        swift: &swift,
    };
    let entries: Vec<(&PathBuf, &Language)> = files.iter().collect();
    // why: collected into an indexed Vec so the reported error never depends on thread scheduling.
    let results: Vec<Result<Collected, ImportsError>> = entries
        .par_iter()
        .map(|&(path, &language)| {
            let source =
                fs::read_to_string(root.join(path)).map_err(|source| ImportsError::Io {
                    path: path.clone(),
                    source,
                })?;
            let imports = extract_in(&source, language, path, &env)?;
            let table = reexport::facade(&source, language, path, &env);
            Ok((path.clone(), imports, table))
        })
        .collect();
    let mut imports = BTreeMap::new();
    let mut tables = reexport::Tables::new();
    for result in results {
        let (path, found, table) = result?;
        tables.extend(table.map(|table| (path.clone(), table)));
        imports.insert(path, found);
    }
    reexport::complete(&mut tables, root, files, &env);
    reexport::apply(&mut imports, &tables, facades);
    Ok(imports)
}
