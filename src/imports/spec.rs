//! The per-language table: grammar, import query and resolver for each supported language.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Resolution, queries, resolve};
use crate::model::Language;

/// Everything needed to extract and resolve imports for one language. Adding a language is adding
/// one entry to [`spec`].
pub struct LanguageSpec {
    /// Language this entry serves.
    pub language: Language,
    /// Grammar to parse `file` with. Chosen per file because TSX needs its own grammar.
    pub grammar: fn(&Path) -> tree_sitter::Language,
    /// Tree-sitter query capturing each import specifier string as `@specifier`.
    pub query: &'static str,
    /// Resolves a specifier written in `importer` against the known repository files.
    pub resolve: fn(&str, &Path, &BTreeSet<PathBuf>) -> Resolution,
}

static TYPESCRIPT: LanguageSpec = LanguageSpec {
    language: Language::TypeScript,
    grammar: typescript_grammar,
    query: queries::TYPESCRIPT,
    resolve: resolve::resolve,
};

static JAVASCRIPT: LanguageSpec = LanguageSpec {
    language: Language::JavaScript,
    grammar: javascript_grammar,
    query: queries::JAVASCRIPT,
    resolve: resolve::resolve,
};

/// The table entry for `language`, or `None` when extraction is not implemented for it yet.
#[must_use]
pub fn spec(language: Language) -> Option<&'static LanguageSpec> {
    match language {
        Language::TypeScript => Some(&TYPESCRIPT),
        Language::JavaScript => Some(&JAVASCRIPT),
        Language::Python | Language::Rust => None,
    }
}

fn typescript_grammar(file: &Path) -> tree_sitter::Language {
    if file.extension().is_some_and(|extension| extension == "tsx") {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }
}

fn javascript_grammar(_file: &Path) -> tree_sitter::Language {
    tree_sitter_javascript::LANGUAGE.into()
}
