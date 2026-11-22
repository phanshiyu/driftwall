//! The per-language table: grammar, import query and resolver for each supported language.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Reference, Resolution, queries, resolve};
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
    /// Tree-sitter query capturing each import's module reference as `@specifier` and, when one
    /// statement imports several things from it, each imported name as `@name`.
    pub query: &'static str,
    /// Resolves one match, written in `importer`, against the known repository files. Returns one
    /// [`Resolution`] per target the match refers to, so a statement naming several imports can
    /// resolve each independently.
    pub resolve: fn(&Reference<'_>, &Path, &BTreeSet<PathBuf>) -> Vec<Resolution>,
}

static TYPESCRIPT: LanguageSpec = LanguageSpec {
    language: Language::TypeScript,
    grammar: typescript_grammar,
    query: queries::TYPESCRIPT,
    resolve: resolve::ecmascript,
};

static JAVASCRIPT: LanguageSpec = LanguageSpec {
    language: Language::JavaScript,
    grammar: javascript_grammar,
    query: queries::JAVASCRIPT,
    resolve: resolve::ecmascript,
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
