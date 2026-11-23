//! Extracts imports with tree-sitter and resolves them to repository files.
//!
//! Extraction is driven by the per-language table in [`spec`]: a grammar, a query capturing import
//! specifiers, and a resolver. TypeScript, JavaScript, Python and Rust are filled in.
//!
//! TypeScript and JavaScript resolve relative specifiers, aliases defined by `compilerOptions`
//! `paths` and `baseUrl` of the governing `tsconfig.json` or `jsconfig.json` (see `tsconfig`, and
//! `project` for solution configs that `references` other configs), workspace packages imported by
//! name and package `imports` (see `workspace`). Bundler query suffixes are dropped first and
//! declaration files are probed like source. A specifier that names an existing non-source file (an
//! asset, supplied to [`collect_with_assets`]) resolves to it as an import of kind `asset`, and a
//! relative one that names nothing is unresolved whatever its extension. A specifier that none of these maps to a repository
//! file is a package: ignored when it is a Node builtin or declared by a `package.json` at or above
//! the file (see `package`), and otherwise recorded in `FileImports::skipped`, like every reference
//! that cannot be followed (a computed `require(x)` or `importlib.import_module(x)`, an
//! `include_str!` of a file that is not source), never reported unresolved. A `tsconfig` or
//! `package.json` that cannot be parsed is skipped, like a malformed `Cargo.toml`. Import types
//! (`typeof import('./a')`) are not extracted. Python absolute imports are tried against an ordered
//! list of import roots: the importer's own directory when it is not a package, the roots a project
//! configures (see `python_config`), the repository root and `src/`; see `python`. `.pyi` stubs are
//! not supported. For Rust, see
//! `rust`'s limitations, listed in "Rust resolution" in `docs/DESIGN.md`: `extern crate` renames and names that only a glob import brings in are not followed. A path that
//! starts with a module name the same file brought into scope with `use` or `mod name;` is
//! followed, also in an inline module through `use super::*;`.
//!
//! Every resolved import also records the symbols it uses from its target, from syntax only (see
//! `ResolvedImport::symbols`). Imports of one file on one line collapse by target and union their
//! symbols; a side-effect import records none. Method calls on values are never resolved. Each also
//! records its kind (runtime, type-only, embed) and whether it is written in test code, see
//! `statement`, and sits at the line its statement starts on.

mod alias;
mod ecmascript_symbols;
mod error;
mod extract;
mod glob;
mod jsdoc;
mod manifest;
mod markup;
mod package;
mod project;
mod python;
mod python_call;
mod python_config;
mod python_layout;
mod queries;
mod resolve;
mod rust;
mod rust_module;
mod rust_paths;
mod rust_scope;
mod rust_syntax;
mod rust_test_modules;
mod shell;
mod spec;
mod statement;
mod subpath;
mod swift;
mod swift_package;
mod tsconfig;
mod vite;
mod vite_alias;
mod workspace;

#[cfg(test)]
mod alias_resolve_tests;
#[cfg(test)]
mod alias_tests;
#[cfg(test)]
mod asset_tests;
#[cfg(test)]
mod ecmascript_symbols_tests;
#[cfg(test)]
mod excluded_tests;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod html_tests;
#[cfg(test)]
mod jsdoc_tests;
#[cfg(test)]
mod kind_tests;
#[cfg(test)]
mod markup_tests;
#[cfg(test)]
mod probe_tests;
#[cfg(test)]
mod python_roots_tests;
#[cfg(test)]
mod python_symbols_tests;
#[cfg(test)]
mod python_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod references_tests;
#[cfg(test)]
mod rust_dogfood_tests;
#[cfg(test)]
mod rust_path_tests;
#[cfg(test)]
mod rust_symbols_tests;
#[cfg(test)]
mod rust_test_modules_tests;
#[cfg(test)]
mod rust_tests;
#[cfg(test)]
mod shell_tests;
#[cfg(test)]
mod skipped_tests;
#[cfg(test)]
mod swift_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tsconfig_tests;
#[cfg(test)]
mod vite_alias_tests;
#[cfg(test)]
mod vite_tests;
#[cfg(test)]
mod workspace_tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::model::{Language, ResolvedImport, SkipReason, Skipped, Symbol, UnresolvedImport};

pub use alias::{AliasConfig, Candidate, Pattern, TargetTemplate, nearest};
pub use error::ImportsError;
pub use extract::{extract, extract_in};
pub use manifest::library_crates;
pub use package::package_dependencies;
pub use project::Project;
pub use python_config::python_roots;
pub use python_layout::PythonLayout;
pub use rust_paths::PathModules;
pub use rust_scope::Bindings;
pub use spec::{Docs, Grammar, LanguageSpec, Load, Loads, Region, Scan, spec};
pub use swift::SwiftModules;
pub use swift_package::{SwiftTarget, swift_targets};
pub use tsconfig::{alias_configs, project_configs};
pub use vite::vite_roots;
pub use vite_alias::{ViteAlias, vite_aliases};
pub use workspace::{Workspaces, discover as workspaces};

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
}

/// What a resolver may consult besides the importing file: every known repository file and the
/// library crates of the repository. Built once per run, shared by every file.
#[derive(Debug, Clone, Copy)]
pub struct Environment<'a> {
    /// Repository-relative paths of every repository file.
    pub known: &'a BTreeSet<PathBuf>,
    /// Repository-relative paths of every asset: a walked file that is not source. Only
    /// TypeScript and JavaScript resolution reads it, so that an import of one resolves.
    pub assets: &'a BTreeSet<PathBuf>,
    /// The assets no `.arch` tag covers, a subset of `assets`. Only Rust resolution reads it: an
    /// embed of an asset records an edge only when something owns the asset.
    pub unowned: &'a BTreeSet<PathBuf>,
    /// Library crate name, as imported, to the repository-relative directory holding its `lib.rs`.
    /// Only Rust resolution reads it.
    pub crates: &'a BTreeMap<String, PathBuf>,
    /// Repository-relative directory holding a `tsconfig.json` or `jsconfig.json` to the alias
    /// configuration it defines. A file is governed by the nearest directory at or above it, see
    /// [`nearest`]. Only TypeScript and JavaScript resolution reads it.
    pub aliases: &'a BTreeMap<PathBuf, AliasConfig>,
    /// Repository-relative directory holding a readable `package.json` to the package names that
    /// file declares in `dependencies`, `devDependencies`, `peerDependencies` and
    /// `optionalDependencies`. A file is governed by the nearest directory at or above it. Only
    /// TypeScript and JavaScript resolution reads it.
    pub packages: &'a BTreeMap<PathBuf, BTreeSet<String>>,
    /// The workspace members of the repository by package name, and the `imports` of its
    /// `package.json` files. Only TypeScript and JavaScript resolution reads it.
    pub workspaces: &'a Workspaces,
    /// Repository-relative directory holding a solution-style `tsconfig.json` to the projects its
    /// `references` name, in the order written. Only TypeScript and JavaScript resolution reads it.
    pub projects: &'a BTreeMap<PathBuf, Vec<Project>>,
    /// The Python directories, names and configured import roots of the repository. Only Python
    /// resolution reads it.
    pub python: &'a PythonLayout,
    /// The Swift targets of the repository by module name. Only Swift resolution reads it.
    pub swift: &'a SwiftModules,
    /// Repository-relative directory holding a `vite.config.*` file to the Vite root that config
    /// gives, see [`vite_roots`]. Only the resolution of root-absolute paths in HTML reads it.
    pub vite_roots: &'a BTreeMap<PathBuf, PathBuf>,
    /// Repository-relative directory holding a `vite.config.*` file to the `resolve.alias` entries
    /// that config gives, in the order written, see [`vite_aliases`]. A file is governed by every
    /// config at or above it. Only TypeScript and JavaScript resolution reads it.
    pub vite_aliases: &'a BTreeMap<PathBuf, Vec<ViteAlias>>,
    /// Files that are test code because a `#[cfg(test)] mod x;` declaration pulls them in, see
    /// the module `rust_test_modules`. Every import written in one is `test_only`.
    pub test_files: &'a BTreeSet<PathBuf>,
    /// The Rust modules a `#[path = ".."]` attribute placed at a file of its own choosing, see
    /// the module `rust_paths`.
    pub rust_paths: &'a PathModules,
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
    /// The names the file brings into scope, for languages whose paths can start with one. Empty
    /// for the others.
    pub bindings: &'a Bindings<'a>,
}

/// What a language resolver decided about one specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The specifier names this repository file, and the importing code uses `symbols` of it.
    Resolved {
        /// The repository-relative file.
        target: PathBuf,
        /// What is used from `target`. Empty for a side-effect import.
        symbols: BTreeSet<Symbol>,
    },
    /// The specifier points inside the repository but matches no file.
    Unresolved,
    /// The specifier embeds this repository file as data or source, and uses no item of it.
    Embedded {
        /// The repository-relative file.
        target: PathBuf,
    },
    /// The specifier names this repository asset, a file that is not source, and uses no item of
    /// it.
    Asset {
        /// The repository-relative file.
        target: PathBuf,
    },
    /// Not a repository import: a declared third party package or a builtin.
    Ignored,
    /// A reference that could not be followed, and is reported as such.
    Skipped(SkipReason),
    /// A specifier that a Vite `resolve.alias` entry matches whose target has no file in the
    /// repository: skipped as [`SkipReason::AliasTargetMissing`], naming the alias and the target.
    AliasTargetMissing {
        /// The alias key as written in the config.
        alias: String,
        /// The repository-relative path the alias points at.
        target: PathBuf,
    },
    /// A reference that could not be followed, and is reported as such with an explanation: a
    /// Python module that no import root reaches ([`SkipReason::UnreachableModule`]) or a
    /// root-absolute path of an HTML file that finds no file ([`SkipReason::UnresolvedRootPath`]).
    SkippedWith {
        /// Why it was skipped.
        reason: SkipReason,
        /// The explanation, naming what the reference was compared with.
        message: String,
    },
}

impl Resolution {
    /// A resolution to `target` that uses no symbol yet.
    #[must_use]
    pub const fn resolved(target: PathBuf) -> Self {
        Self::Resolved {
            target,
            symbols: BTreeSet::new(),
        }
    }

    /// This resolution, with `symbols` as what the importing code uses. Resolutions that did not
    /// resolve are returned unchanged.
    #[must_use]
    pub fn using(self, symbols: BTreeSet<Symbol>) -> Self {
        match self {
            Self::Resolved { target, .. } => Self::Resolved { target, symbols },
            other => other,
        }
    }
}

/// [`collect_with_assets`] when no asset or excluded file is known, so an import of a non-source
/// file is unresolved.
///
/// # Errors
/// The same as [`collect_with_assets`].
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
/// and query errors of [`extract`].
pub fn collect_with_assets(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
    assets: &BTreeSet<PathBuf>,
    unowned: &BTreeSet<PathBuf>,
    excluded: &BTreeSet<PathBuf>,
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
    let rust_paths = rust_paths::path_modules(root, files, &known);
    let test_files = rust_test_modules::test_files(root, files, &known, &rust_paths);
    let env = Environment {
        known: &known,
        assets,
        unowned,
        crates: &crates,
        aliases: &aliases,
        packages: &packages,
        workspaces: &workspaces,
        projects: &projects,
        python: &python,
        swift: &swift,
        vite_roots: &vite,
        vite_aliases: &aliases_of_vite,
        test_files: &test_files,
        rust_paths: &rust_paths,
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
