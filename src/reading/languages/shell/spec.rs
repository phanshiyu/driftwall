//! The shell entry of the language table.

use std::path::Path;

use super::statement;
use crate::code::Language;
use crate::reading::syntax::queries;
use crate::reading::syntax::spec::{Grammar, LanguageSpec, Scan, whole_file};

/// The grammar of shell files.
pub static GRAMMAR: Grammar = Grammar {
    name: "bash",
    language: || tree_sitter_bash::LANGUAGE.into(),
    query: queries::SHELL,
};

/// How shell files are scanned, located and flagged.
pub static SPEC: LanguageSpec = LanguageSpec {
    language: Language::Shell,
    scan,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: None,
    docs: None,
    known_gap: Some(statement::case_pattern_gap),
};

fn scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&GRAMMAR)
}
