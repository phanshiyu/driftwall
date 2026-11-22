//! Resolution of Rust paths against the known repository files.
//!
//! A path is a dependency when it starts with `crate`, `self`, `super` or the name of a library
//! crate in the repository, and the file that holds the item it names is another repository file.
//! Each resolution also records the symbol it uses: the first item segment after the module file,
//! or all of the module when the path is a glob that ends there. A `use` that only binds the module
//! name (`use crate::gitlab;`, `as gl`, `self` in a group) records no symbol. `mod foo;` is containment and is not itself an
//! edge.
//!
//! A path may also start with a module name the same file brought into scope: `use crate::gitlab;`
//! or `use crate::gitlab as gl;` followed by `gitlab::Client` or `gl::Client`, or `mod gitlab;`
//! followed by `gitlab::Client`. Such a path resolves through the binding, see `rust_scope`. Only
//! names that stand for a repository module file are followed: `use crate::gitlab::Client;` binds
//! `Client`, and a later `Client::new()` is not a second reference to `gitlab`, the `use` line
//! already is. A binding is visible only in the module block that declares it, a `use` inside a
//! function counts for its whole module, a name bound twice to different paths is not followed,
//! and a binding is never followed through another binding.
//!
//! `include_str!`, `include_bytes!` and `include!` with a string literal embed a file relative to the
//! importing file: a known repository file is an [`Embedded`](Resolution::Embedded) resolution that
//! uses no symbol, anything else is skipped as a non-source embed.
//!
//! Known limitations: `#[path = "..."]` module attributes, `extern crate` renames, names that
//! only a glob import brings in (`use crate::*; gitlab::x()`), local names that come from an
//! inline `mod name { }`, and macro-generated paths are not followed.

use std::collections::BTreeSet;
use std::path::Path;

use super::resolve::join_normalised;
use super::rust_module::{Site, WrittenPath};
use super::rust_syntax;
use super::{Environment, Reference, Resolution};
use crate::model::{SkipReason, Symbol};

/// The table entry. `reference.node` is a `use` argument, a scoped path, or a token inside a macro
/// argument list; each is expanded into the repository files it points at. A reference that
/// resolves to the importing file itself is dropped. Rust paths always fall back to the crate root
/// file, so nothing is ever unresolved.
pub fn resolve(
    reference: &Reference<'_>,
    importer: &Path,
    env: &Environment<'_>,
) -> Vec<Resolution> {
    if reference.node.kind() == "string_literal" {
        return vec![embed(reference.specifier, importer, env)];
    }
    let paths = rust_syntax::paths(reference.node, reference.source);
    if paths.is_empty() {
        return Vec::new();
    }
    let inline = rust_syntax::enclosing_modules(reference.node, reference.source);
    let site = Site::of(importer, &inline, env.known);
    let mut resolutions = Vec::new();
    for path in &paths {
        let path = through_bindings(path, reference, &inline, site.as_ref(), env);
        let Some((target, symbol)) = path.target(site.as_ref(), env) else {
            continue;
        };
        if target == importer {
            continue;
        }
        // why: `use crate::gitlab;` binds a module name. Its items are recorded where they are used.
        let used = if symbol == Symbol::All && path.binds_module() {
            BTreeSet::new()
        } else {
            BTreeSet::from([symbol])
        };
        resolutions.push(Resolution::resolved(target).using(used));
    }
    resolutions
}

/// An `include_str!("..")`, `include_bytes!("..")` or `include!("..")` of `path`, relative to the
/// directory of `importer`. A known repository file is embedded, anything else is skipped.
fn embed(path: &str, importer: &Path, env: &Environment<'_>) -> Resolution {
    match join_normalised(importer.parent(), path) {
        Some(target) if env.known.contains(&target) => Resolution::Embedded { target },
        _ => Resolution::Skipped(SkipReason::EmbedNonSource),
    }
}

/// `path` with its first segment replaced by the module path the file bound that name to, when
/// the name is bound in the module block `inline` to a path that is a repository module file.
/// Otherwise `path` unchanged.
fn through_bindings<'s>(
    path: &WrittenPath<'s>,
    reference: &Reference<'s>,
    inline: &[&'s str],
    site: Option<&Site>,
    env: &Environment<'_>,
) -> WrittenPath<'s> {
    let bound = path
        .first_name()
        .and_then(|name| reference.bindings.get(inline, name));
    match bound {
        Some(prefix)
            if prefix
                .target(site, env)
                .is_some_and(|(_, used)| used == Symbol::All) =>
        {
            path.with_first_replaced(prefix)
        }
        _ => path.clone(),
    }
}
