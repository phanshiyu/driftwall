//! The Swift entry of the language table.

use std::path::Path;

use super::statement;
use crate::imports::syntax::queries;
use crate::imports::syntax::spec::{Grammar, LanguageSpec, Scan, whole_file};
use crate::model::Language;

/// The grammar of Swift files.
pub static GRAMMAR: Grammar = Grammar {
    name: "swift",
    language: || tree_sitter_swift::LANGUAGE.into(),
    query: queries::SWIFT,
};

/// How Swift files are scanned, located and flagged.
pub static SPEC: LanguageSpec = LanguageSpec {
    language: Language::Swift,
    scan,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: None,
    docs: None,
};

fn scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&GRAMMAR)
}
