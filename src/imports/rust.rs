//! Resolution of Rust paths against the known repository files.
//!
//! A path is a dependency when it starts with `crate`, `self`, `super` or the name of a library
//! crate in the repository, and the file that holds the item it names is another repository file.
//! `mod foo;` declarations are containment and never match the query.
//!
//! Known limitations: `#[path = "..."]` module attributes, `extern crate` renames, relative paths
//! that start with a local module name (`use rules::x;`), and macro-generated paths are not
//! followed.

use std::path::Path;

use super::rust_module::Site;
use super::rust_syntax;
use super::{Environment, Reference, Resolution};

/// The table entry. `reference.node` is a `use` argument, a scoped path, or a token inside a macro
/// argument list; each is expanded into the repository files it points at. A reference that
/// resolves to the importing file itself is dropped. Rust paths always fall back to the crate root
/// file, so nothing is ever unresolved.
pub fn resolve(
    reference: &Reference<'_>,
    importer: &Path,
    env: &Environment<'_>,
) -> Vec<Resolution> {
    let paths = rust_syntax::paths(reference.node, reference.source);
    if paths.is_empty() {
        return Vec::new();
    }
    let inline = rust_syntax::enclosing_modules(reference.node, reference.source);
    let site = Site::of(importer, &inline, env.known);
    paths
        .iter()
        .filter_map(|path| path.target(site.as_ref(), env))
        .filter(|target| target != importer)
        .map(Resolution::Resolved)
        .collect()
}
