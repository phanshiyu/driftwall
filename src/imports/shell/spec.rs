//! The shell entry of the language table.

use std::path::Path;

use super::statement;
use crate::imports::syntax::queries;
use crate::imports::syntax::spec::{Grammar, LanguageSpec, Scan, whole_file};
use crate::model::Language;

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
};

fn scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&GRAMMAR)
}
