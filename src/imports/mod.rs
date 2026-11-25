//! Extracts imports with tree-sitter and resolves them to repository files.
//!
//! Extraction is driven by the per-language table in [`spec()`]: a grammar, a query capturing import
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
//! configures (see `python::python_roots`), the repository root and `src/`; see the `python` module. `.pyi` stubs are
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

mod collect;
mod ecmascript;
mod environment;
mod error;
mod extract;
mod file_imports;
mod language;
mod python;
mod reexport;
mod rust;
mod shell;
mod spec;
mod swift;
mod syntax;

#[cfg(test)]
mod scenarios;

pub use collect::{collect, collect_with_assets, collect_with_facades};
pub use ecmascript::{
    AliasConfig, Candidate, Pattern, Project, TargetTemplate, ViteAlias, Workspaces, alias_configs,
    nearest, package_dependencies, project_configs, vite_aliases, vite_roots, workspaces,
};
pub use environment::Environment;
pub use error::ImportsError;
pub use extract::{extract, extract_in};
pub use file_imports::FileImports;
pub use python::{PythonLayout, python_roots};
pub use reexport::Facades;
pub use rust::{Bindings, PathModules, cargo_dependencies, library_crates};
pub use spec::spec;
pub use swift::{SwiftModules, SwiftTarget, swift_targets};
pub use syntax::resolution::{Files, Reference, Resolution};
pub use syntax::spec::{Docs, Grammar, LanguageSpec, Load, Loads, Region, Scan};
