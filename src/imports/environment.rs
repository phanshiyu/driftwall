//! What resolution may consult besides the importing file, built once per run.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::ecmascript;
use super::python::PythonLayout;
use super::rust;
use super::swift::SwiftModules;
use super::syntax::resolution::Files;

/// What resolvers may consult besides the importing file: every known repository file and what
/// each language reads of the repository's configuration. Built once per run, shared by every
/// file.
#[derive(Debug, Clone, Copy)]
pub struct Environment<'a> {
    /// The repository files: source, assets and the assets nothing owns.
    pub files: Files<'a>,
    /// Files that are test code because a `#[cfg(test)] mod x;` declaration pulls them in, see
    /// the module `test_modules` of `rust`. Every import written in one is `test_only`.
    pub test_files: &'a BTreeSet<PathBuf>,
    /// The configuration of TypeScript, JavaScript, Svelte and HTML.
    pub ecmascript: ecmascript::Env<'a>,
    /// The Python directories, names and configured import roots of the repository.
    pub python: &'a PythonLayout,
    /// The Rust crates, placed modules and dependencies of the repository.
    pub rust: rust::Env<'a>,
    /// The Swift targets of the repository by module name.
    pub swift: &'a SwiftModules,
}
