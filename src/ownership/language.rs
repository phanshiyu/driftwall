//! The single table of source file extensions driftwall recognises.

use std::path::Path;

use crate::model::Language;

/// Extension (without the dot) to language. The only place that decides what counts as source.
const EXTENSIONS: &[(&str, Language)] = &[
    ("ts", Language::TypeScript),
    ("tsx", Language::TypeScript),
    ("mts", Language::TypeScript),
    ("cts", Language::TypeScript),
    ("js", Language::JavaScript),
    ("jsx", Language::JavaScript),
    ("mjs", Language::JavaScript),
    ("cjs", Language::JavaScript),
    ("py", Language::Python),
    ("rs", Language::Rust),
    ("svelte", Language::Svelte),
];

/// The language of `path`, judged by its extension, or `None` when it is not a source file.
///
/// Matching is case-sensitive: `main.PY` is not Python source.
#[must_use]
pub fn language_of(path: &Path) -> Option<Language> {
    let extension = path.extension()?.to_str()?;
    EXTENSIONS
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|&(_, language)| language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_supported_extension() {
        let cases = [
            ("a.ts", Language::TypeScript),
            ("a.tsx", Language::TypeScript),
            ("a.mts", Language::TypeScript),
            ("a.cts", Language::TypeScript),
            ("a.js", Language::JavaScript),
            ("a.jsx", Language::JavaScript),
            ("a.mjs", Language::JavaScript),
            ("a.cjs", Language::JavaScript),
            ("a.py", Language::Python),
            ("a.rs", Language::Rust),
            ("a.svelte", Language::Svelte),
        ];
        for (name, expected) in cases {
            assert_eq!(language_of(Path::new(name)), Some(expected), "{name}");
        }
    }

    #[test]
    fn rejects_everything_else() {
        for name in [
            "README.md",
            "data.json",
            "Makefile",
            ".arch",
            "a.d",
            "ts",
            "a.TS",
        ] {
            assert_eq!(language_of(Path::new(name)), None, "{name}");
        }
    }

    #[test]
    fn uses_only_the_final_extension() {
        assert_eq!(
            language_of(Path::new("dir.py/file.json")),
            None,
            "directory names must not matter"
        );
        assert_eq!(
            language_of(Path::new("dir/a.test.ts")),
            Some(Language::TypeScript)
        );
    }
}
