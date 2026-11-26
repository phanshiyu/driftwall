//! The per-language table: grammar, import query and statement rules for each supported language.

use crate::code::Language;
use crate::reading::languages::{python, rust, shell, swift, web};
use crate::reading::syntax::spec::LanguageSpec;

/// The table entry for `language`: every language has one.
#[must_use]
pub fn spec(language: Language) -> &'static LanguageSpec {
    match language {
        Language::TypeScript => &web::TYPESCRIPT,
        Language::JavaScript => &web::JAVASCRIPT,
        Language::Python => &python::SPEC,
        Language::Rust => &rust::SPEC,
        Language::Svelte => &web::SVELTE,
        Language::Html => &web::HTML,
        Language::Swift => &swift::SPEC,
        Language::Shell => &shell::SPEC,
    }
}
