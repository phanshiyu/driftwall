//! The single table of source file extensions driftwall recognises.

use std::fs;
use std::path::Path;

use crate::model::{Language, markup};

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
    ("html", Language::Html),
    ("htm", Language::Html),
];

/// Languages whose files are source only when they hold a `<script` element, see
/// [`is_source`]. Every other file of such a language is an asset.
const NEEDS_SCRIPT: &[Language] = &[Language::Html];

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

/// Whether the file at `relative`, a path below `root`, is a source file: its extension names a
/// language, and when that language needs a `<script` element (HTML), the file holds one. A file
/// that cannot be read as UTF-8 holds none, so reading it can never fail a scan.
#[must_use]
pub fn is_source(root: &Path, relative: &Path) -> bool {
    let Some(language) = language_of(relative) else {
        return false;
    };
    !NEEDS_SCRIPT.contains(&language)
        || fs::read_to_string(root.join(relative)).is_ok_and(|text| markup::has_script(&text))
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
            ("a.html", Language::Html),
            ("a.htm", Language::Html),
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

    #[test]
    fn html_is_source_only_with_a_script_element() {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            ("with.html", "<p>x</p><script>1</script>", true),
            ("src.htm", "<SCRIPT SRC=a.js></SCRIPT>", true),
            ("none.html", "<p>no script here</p>", false),
            ("mention.html", "<p>write &lt;script&gt;</p>", false),
            ("commented.html", "<!-- <script>1</script> -->", false),
            ("empty.html", "", false),
            ("code.ts", "", true),
        ];
        for (name, text, _) in files {
            fs::write(dir.path().join(name), text).unwrap();
        }
        fs::write(dir.path().join("binary.html"), [0xff, 0xfe, b'<']).unwrap();
        for (name, _, expected) in files {
            assert_eq!(is_source(dir.path(), Path::new(name)), expected, "{name}");
        }
        assert!(!is_source(dir.path(), Path::new("binary.html")));
        assert!(!is_source(dir.path(), Path::new("missing.html")));
        assert!(!is_source(dir.path(), Path::new("README.md")));
    }

    #[test]
    fn a_scan_separates_html_sources_from_html_assets() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("site")).unwrap();
        fs::write(dir.path().join("site/.arch"), "node: site\n").unwrap();
        fs::write(
            dir.path().join("site/page.html"),
            "<script src=a.js></script>",
        )
        .unwrap();
        fs::write(dir.path().join("site/doc.html"), "<h1>docs</h1>").unwrap();
        let ownership = crate::ownership::scan(dir.path()).unwrap();
        let sources: Vec<_> = ownership
            .owners
            .keys()
            .map(|p| p.display().to_string())
            .collect();
        let assets: Vec<_> = ownership
            .asset_owners
            .keys()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(sources, ["site/page.html"]);
        assert_eq!(assets, ["site/doc.html"]);
        assert!(ownership.unmapped.is_empty());
    }
}
