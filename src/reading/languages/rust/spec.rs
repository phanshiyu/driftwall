//! The Rust entry of the language table.

use std::path::Path;

use super::statement;
use crate::code::Language;
use crate::reading::syntax::queries;
use crate::reading::syntax::spec::{Grammar, LanguageSpec, Scan, whole_file};

/// The grammar of Rust files.
pub static GRAMMAR: Grammar = Grammar {
    name: "rust",
    language: || tree_sitter_rust::LANGUAGE.into(),
    query: queries::RUST,
};

/// How Rust files are scanned, located and flagged.
pub static SPEC: LanguageSpec = LanguageSpec {
    language: Language::Rust,
    scan,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: None,
    docs: None,
};

fn scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&GRAMMAR)
}
