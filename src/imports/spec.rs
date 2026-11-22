//! The per-language table: grammar, import query and resolver for each supported language.

use std::path::Path;

use super::{
    Bindings, Environment, Reference, Resolution, python, queries, resolve, rust, statement,
};
use crate::model::Language;

/// One tree-sitter grammar with a stable name that keys the compiled-query cache.
pub struct Grammar {
    /// Unique across all grammars.
    pub name: &'static str,
    /// Builds the tree-sitter language.
    pub language: fn() -> tree_sitter::Language,
}

static TYPESCRIPT_GRAMMAR: Grammar = Grammar {
    name: "typescript",
    language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
};

static TSX_GRAMMAR: Grammar = Grammar {
    name: "tsx",
    language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
};

static JAVASCRIPT_GRAMMAR: Grammar = Grammar {
    name: "javascript",
    language: || tree_sitter_javascript::LANGUAGE.into(),
};

static PYTHON_GRAMMAR: Grammar = Grammar {
    name: "python",
    language: || tree_sitter_python::LANGUAGE.into(),
};

static RUST_GRAMMAR: Grammar = Grammar {
    name: "rust",
    language: || tree_sitter_rust::LANGUAGE.into(),
};

/// Everything needed to extract and resolve imports for one language. Adding a language is adding
/// one entry to [`spec`].
pub struct LanguageSpec {
    /// Language this entry serves.
    pub language: Language,
    /// Grammar to parse `file` with. Chosen per file because TSX needs its own grammar.
    pub grammar: fn(&Path) -> &'static Grammar,
    /// Tree-sitter query capturing each import's module reference as `@specifier` and, when one
    /// statement imports several things from it, each imported name as `@name`.
    pub query: &'static str,
    /// Collects, once per file, the names the file brings into scope. Languages whose resolver
    /// does not follow such names use a function that returns no bindings.
    pub bindings: for<'a> fn(tree_sitter::Node<'a>, &'a str) -> Bindings<'a>,
    /// The node whose first line is the location of the import that `@specifier` is part of: the
    /// statement, or the call expression, around it.
    pub anchor: for<'a> fn(tree_sitter::Node<'a>) -> tree_sitter::Node<'a>,
    /// Resolves one match, written in `importer`, against the [`Environment`]. Returns one
    /// [`Resolution`] per target the match refers to, so a statement naming several imports can
    /// resolve each independently.
    pub resolve: fn(&Reference<'_>, &Path, &Environment<'_>) -> Vec<Resolution>,
}

static TYPESCRIPT: LanguageSpec = LanguageSpec {
    language: Language::TypeScript,
    grammar: typescript_grammar,
    query: queries::TYPESCRIPT,
    bindings: no_bindings,
    anchor: statement::ecmascript_anchor,
    resolve: resolve::ecmascript,
};

static PYTHON: LanguageSpec = LanguageSpec {
    language: Language::Python,
    grammar: python_grammar,
    query: queries::PYTHON,
    bindings: no_bindings,
    anchor: statement::python_anchor,
    resolve: python::resolve,
};

static JAVASCRIPT: LanguageSpec = LanguageSpec {
    language: Language::JavaScript,
    grammar: javascript_grammar,
    query: queries::JAVASCRIPT,
    bindings: no_bindings,
    anchor: statement::ecmascript_anchor,
    resolve: resolve::ecmascript,
};

static RUST: LanguageSpec = LanguageSpec {
    language: Language::Rust,
    grammar: rust_grammar,
    query: queries::RUST,
    bindings: rust_bindings,
    anchor: statement::rust_anchor,
    resolve: rust::resolve,
};

/// The table entry for `language`, or `None` when extraction is not implemented for it yet.
#[must_use]
pub fn spec(language: Language) -> Option<&'static LanguageSpec> {
    match language {
        Language::TypeScript => Some(&TYPESCRIPT),
        Language::JavaScript => Some(&JAVASCRIPT),
        Language::Python => Some(&PYTHON),
        Language::Rust => Some(&RUST),
    }
}

/// The [`LanguageSpec::bindings`] of a language that does not follow names brought into scope.
fn no_bindings<'a>(_root: tree_sitter::Node<'a>, _source: &'a str) -> Bindings<'a> {
    Bindings::default()
}

/// The [`LanguageSpec::bindings`] of Rust: its `use` leaves and `mod name;` declarations.
fn rust_bindings<'a>(root: tree_sitter::Node<'a>, source: &'a str) -> Bindings<'a> {
    Bindings::of_tree(root, source)
}

fn typescript_grammar(file: &Path) -> &'static Grammar {
    if file.extension().is_some_and(|extension| extension == "tsx") {
        &TSX_GRAMMAR
    } else {
        &TYPESCRIPT_GRAMMAR
    }
}

const fn javascript_grammar(_file: &Path) -> &'static Grammar {
    &JAVASCRIPT_GRAMMAR
}

const fn python_grammar(_file: &Path) -> &'static Grammar {
    &PYTHON_GRAMMAR
}

const fn rust_grammar(_file: &Path) -> &'static Grammar {
    &RUST_GRAMMAR
}
