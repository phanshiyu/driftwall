//! The per-language table: grammar, import query and statement rules for each supported language.

use super::syntax::spec::LanguageSpec;
use super::{ecmascript, python, rust, shell, swift};
use crate::model::Language;

/// The table entry for `language`, or `None` when extraction is not implemented for it yet.
#[must_use]
pub fn spec(language: Language) -> Option<&'static LanguageSpec> {
    match language {
        Language::TypeScript => Some(&ecmascript::TYPESCRIPT),
        Language::JavaScript => Some(&ecmascript::JAVASCRIPT),
        Language::Python => Some(&python::SPEC),
        Language::Rust => Some(&rust::SPEC),
        Language::Svelte => Some(&ecmascript::SVELTE),
        Language::Html => Some(&ecmascript::HTML),
        Language::Swift => Some(&swift::SPEC),
        Language::Shell => Some(&shell::SPEC),
    }
}
