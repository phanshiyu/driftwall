//! The entries of TypeScript, JavaScript, Svelte and HTML in the language table.

use std::path::Path;

use super::{jsdoc, markup, statement};
use crate::imports::syntax::queries;
use crate::imports::syntax::spec::{Docs, Grammar, LanguageSpec, Loads, Scan, whole_file};
use crate::model::Language;

/// The grammar of TypeScript.
pub static TYPESCRIPT_GRAMMAR: Grammar = Grammar {
    name: "typescript",
    language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    query: queries::TYPESCRIPT,
};

static TSX_GRAMMAR: Grammar = Grammar {
    name: "tsx",
    language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
    query: queries::TYPESCRIPT,
};

/// The grammar of JavaScript.
pub static JAVASCRIPT_GRAMMAR: Grammar = Grammar {
    name: "javascript",
    language: || tree_sitter_javascript::LANGUAGE.into(),
    query: queries::JAVASCRIPT,
};

/// The [`LanguageSpec::docs`] of TypeScript, JavaScript and Svelte.
static DOCS: Docs = Docs {
    read: jsdoc::read,
    flags: statement::doc_flags,
};

/// How TypeScript files are scanned, located and flagged.
pub static TYPESCRIPT: LanguageSpec = LanguageSpec {
    language: Language::TypeScript,
    scan: typescript_scan,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: None,
    docs: Some(&DOCS),
};

/// How Svelte files are scanned, located and flagged.
pub static SVELTE: LanguageSpec = LanguageSpec {
    language: Language::Svelte,
    scan: markup::svelte,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: None,
    docs: Some(&DOCS),
};

/// How HTML files are scanned, located and flagged.
pub static HTML: LanguageSpec = LanguageSpec {
    language: Language::Html,
    scan: markup::html,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: Some(Loads {
        flags: statement::load_flags,
    }),
    docs: None,
};

/// How JavaScript files are scanned, located and flagged.
pub static JAVASCRIPT: LanguageSpec = LanguageSpec {
    language: Language::JavaScript,
    scan: javascript_scan,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: None,
    docs: Some(&DOCS),
};

fn typescript_scan(_source: &str, file: &Path) -> Scan {
    whole_file(
        if file.extension().is_some_and(|extension| extension == "tsx") {
            &TSX_GRAMMAR
        } else {
            &TYPESCRIPT_GRAMMAR
        },
    )
}

fn javascript_scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&JAVASCRIPT_GRAMMAR)
}
