//! The one place that knows every language: which resolver answers a match, and how a file is
//! recognised as test code.

use std::path::Path;

use super::syntax::resolution::{Reference, Resolution};
use super::syntax::spec::Load;
use super::{Environment, ecmascript, python, rust, shell, swift};
use crate::model::Language;

/// What a language collects once per file before its matches are resolved.
pub enum Scope<'a> {
    /// The language resolves a match from the match alone.
    Plain,
    /// The names a Rust file brings into scope.
    Rust(rust::Bindings<'a>),
}

/// The scope of the tree rooted at `root`, parsed from `source`.
#[must_use]
pub fn scope<'a>(language: Language, root: tree_sitter::Node<'a>, source: &'a str) -> Scope<'a> {
    match language {
        Language::Rust => Scope::Rust(rust::Bindings::of_tree(root, source)),
        _ => Scope::Plain,
    }
}

/// Resolves one match, written in `importer`, to one [`Resolution`] per target it refers to.
#[must_use]
pub fn resolve(
    language: Language,
    reference: &Reference<'_>,
    scope: &Scope<'_>,
    importer: &Path,
    env: &Environment<'_>,
) -> Vec<Resolution> {
    match language {
        Language::TypeScript | Language::JavaScript | Language::Svelte => {
            ecmascript::resolve(reference, importer, &env.files, &env.ecmascript)
        }
        Language::Html => {
            ecmascript::resolve_inline(reference, importer, &env.files, &env.ecmascript)
        }
        Language::Python => python::resolve(reference, importer, &env.files, env.python),
        Language::Rust => match scope {
            Scope::Rust(bindings) => {
                rust::resolve(reference, bindings, importer, &env.files, &env.rust)
            }
            Scope::Plain => rust::resolve(
                reference,
                &rust::Bindings::default(),
                importer,
                &env.files,
                &env.rust,
            ),
        },
        Language::Swift => swift::resolve(reference, importer, &env.files, env.swift),
        Language::Shell => shell::resolve(reference, importer, &env.files),
    }
}

/// Resolves one file that the markup of `language` loads with an attribute.
#[must_use]
pub fn resolve_load(
    language: Language,
    load: &Load,
    importer: &Path,
    env: &Environment<'_>,
) -> Resolution {
    match language {
        Language::Html => ecmascript::resolve_load(load, importer, &env.files, &env.ecmascript),
        _ => Resolution::Ignored,
    }
}

/// Resolves the specifier of one `JSDoc` type import written in `importer`.
#[must_use]
pub fn resolve_doc(
    language: Language,
    specifier: &str,
    importer: &Path,
    env: &Environment<'_>,
) -> Resolution {
    match language {
        Language::TypeScript | Language::JavaScript | Language::Svelte | Language::Html => {
            ecmascript::specifier(specifier, importer, &env.files, &env.ecmascript)
        }
        _ => Resolution::Ignored,
    }
}

/// Whether the name or place of `file`, written in `language`, says it is test code. The file
/// rule behind every import's test flag, without the part that needs the statement (a Rust
/// `#[cfg(test)]` item) or the module declarations of other files (see `test_modules` of `rust`).
#[must_use]
pub fn test_file(language: Language, file: &Path) -> bool {
    match language {
        Language::TypeScript | Language::JavaScript | Language::Svelte | Language::Html => {
            ecmascript::test_file(file)
        }
        Language::Python => python::test_file(file),
        Language::Rust => rust::is_test_file(file),
        Language::Swift => swift::test_file(file),
        Language::Shell => shell::test_file(file),
    }
}
