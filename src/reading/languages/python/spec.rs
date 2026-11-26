//! The Python entry of the language table.

use std::path::Path;

use super::statement;
use crate::code::Language;
use crate::reading::syntax::queries;
use crate::reading::syntax::spec::{Grammar, LanguageSpec, Scan, whole_file};

/// The grammar of Python files.
pub static GRAMMAR: Grammar = Grammar {
    name: "python",
    language: || tree_sitter_python::LANGUAGE.into(),
    query: queries::PYTHON,
};

/// How Python files are scanned, located and flagged.
pub static SPEC: LanguageSpec = LanguageSpec {
    language: Language::Python,
    scan,
    anchor: statement::anchor,
    flags: statement::flags,
    loads: None,
    docs: None,
};

fn scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&GRAMMAR)
}
