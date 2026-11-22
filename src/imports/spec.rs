//! The per-language table: grammar, import query and resolver for each supported language.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Resolution, queries, resolve};
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

/// Everything needed to extract and resolve imports for one language. Adding a language is adding
/// one entry to [`spec`].
pub struct LanguageSpec {
    /// Language this entry serves.
    pub language: Language,
    /// Grammar to parse `file` with. Chosen per file because TSX needs its own grammar.
    pub grammar: fn(&Path) -> &'static Grammar,
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
