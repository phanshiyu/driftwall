//! The single table of source file extensions driftwall recognises.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::code::Language;
use crate::markup;

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
    ("swift", Language::Swift),
    ("sh", Language::Shell),
    ("bash", Language::Shell),
    ("zsh", Language::Shell),
];

/// Interpreter name (the last `/` segment of the program a shebang names) to language, for a file
/// with no extension. A name that starts with `python` is Python only when digits and dots follow,
/// see [`interpreter_language`].
const INTERPRETERS: &[(&str, Language)] = &[
    ("sh", Language::Shell),
    ("bash", Language::Shell),
    ("zsh", Language::Shell),
    ("dash", Language::Shell),
    ("python", Language::Python),
];

/// How much of the first line of an extensionless file is read.
const SHEBANG_BYTES: u64 = 256;

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

/// The language of the file at `relative`, a path below `root`, or `None` when it is not source.
///
/// A file with an extension is judged by it, and when that language needs a `<script` element
/// (HTML) the file must hold one. A file with no extension is judged by its shebang line, see
/// "Source languages by file" in `docs/DESIGN.md`. A file that cannot be read, or is not UTF-8 where
/// it is read, is not source, so reading can never fail a scan.
#[must_use]
pub fn language_in(root: &Path, relative: &Path) -> Option<Language> {
    if relative.extension().is_none() {
        return shebang_language(&root.join(relative));
    }
    let language = language_of(relative)?;
    (!NEEDS_SCRIPT.contains(&language)
        || fs::read_to_string(root.join(relative)).is_ok_and(|text| markup::has_script(&text)))
    .then_some(language)
}

/// Whether the file at `relative`, a path below `root`, is a source file: [`language_in`] names
/// a language for it.
#[must_use]
pub fn is_source(root: &Path, relative: &Path) -> bool {
    language_in(root, relative).is_some()
}

/// The language a shebang first line gives the file at `path`, or `None`.
fn shebang_language(path: &Path) -> Option<Language> {
    let mut head = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(SHEBANG_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    let line = head.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?;
    interpreter_language(line.strip_prefix("#!")?)
}

/// The language of the interpreter that the text after `#!` names: its first word, or after `env`
/// the first word that is no option and no `NAME=value`.
fn interpreter_language(after_bang: &str) -> Option<Language> {
    let mut words = after_bang.split_whitespace();
    let mut name = base_name(words.next()?);
    if name == "env" {
        name = base_name(words.find(|word| !word.starts_with('-') && !word.contains('='))?);
    }
    INTERPRETERS
        .iter()
        .find(|(known, language)| {
            name == *known
                || (*language == Language::Python
                    && name.strip_prefix(known).is_some_and(|version| {
                        !version.is_empty()
                            && version.chars().all(|c| c.is_ascii_digit() || c == '.')
                    }))
        })
        .map(|&(_, language)| language)
}

/// The last `/` segment of `program`.
fn base_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
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
            ("a.swift", Language::Swift),
            ("a.sh", Language::Shell),
            ("a.bash", Language::Shell),
            ("a.zsh", Language::Shell),
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

    fn sniffed(first_line: &[u8]) -> Option<Language> {
        let dir = tempfile::tempdir().unwrap();
        let mut text = first_line.to_vec();
        text.extend_from_slice(b"\necho hi\n");
        fs::write(dir.path().join("script"), text).unwrap();
        language_in(dir.path(), Path::new("script"))
    }

    #[test]
    fn a_shebang_names_the_language_of_an_extensionless_file() {
        let cases: [(&str, Option<Language>); 24] = [
            ("#!/bin/sh", Some(Language::Shell)),
            ("#!/bin/bash", Some(Language::Shell)),
            ("#!/usr/bin/env bash", Some(Language::Shell)),
            ("#!/usr/bin/env zsh", Some(Language::Shell)),
            ("#!/bin/zsh -f", Some(Language::Shell)),
            ("#!/bin/dash", Some(Language::Shell)),
            ("#! /bin/bash", Some(Language::Shell)),
            ("#!/usr/bin/env -S bash -e", Some(Language::Shell)),
            ("#!/usr/bin/env FOO=1 sh", Some(Language::Shell)),
            ("#!bash", Some(Language::Shell)),
            ("#!/usr/bin/python", Some(Language::Python)),
            ("#!/usr/bin/python3", Some(Language::Python)),
            ("#!/usr/bin/env python3.12", Some(Language::Python)),
            ("#!/usr/bin/env -S python3 -u", Some(Language::Python)),
            ("#!/usr/bin/env node", None),
            ("#!/usr/bin/perl", None),
            ("#!/bin/ksh", None),
            ("#!/usr/bin/pythonista", None),
            ("#!/usr/bin/python3x", None),
            ("#!/usr/bin/env", None),
            ("#!", None),
            (" #!/bin/bash", None),
            ("# /bin/bash", None),
            ("echo #!/bin/bash", None),
        ];
        for (line, expected) in cases {
            assert_eq!(sniffed(line.as_bytes()), expected, "{line}");
        }
    }

    #[test]
    fn only_the_first_line_and_valid_utf8_are_read() {
        assert_eq!(sniffed(b"\n#!/bin/bash"), None);
        assert_eq!(sniffed(b"#!/bin/bash\r"), Some(Language::Shell));
        assert_eq!(sniffed(b"#!/bin/ba\xffsh"), None);
        assert_eq!(
            sniffed(&[0xef, 0xbb, 0xbf, b'#', b'!', b'/', b's', b'h']),
            None
        );
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("empty"), "").unwrap();
        assert_eq!(language_in(dir.path(), Path::new("empty")), None);
        assert_eq!(language_in(dir.path(), Path::new("missing")), None);
    }

    #[test]
    fn a_long_first_line_is_cut_at_256_bytes() {
        let mut line = b"#!/usr/bin/env ".to_vec();
        line.extend(std::iter::repeat_n(b'-', 300));
        line.extend_from_slice(b" bash");
        assert_eq!(sniffed(&line), None);
    }

    #[test]
    fn a_shebang_is_ignored_in_a_file_with_an_extension() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["run.txt", "deploy.old", "deploy.", "run.PY"] {
            fs::write(dir.path().join(name), "#!/bin/bash\n").unwrap();
            assert_eq!(language_in(dir.path(), Path::new(name)), None, "{name}");
        }
        fs::write(dir.path().join("x.sh"), "no shebang\n").unwrap();
        assert_eq!(
            language_in(dir.path(), Path::new("x.sh")),
            Some(Language::Shell)
        );
        fs::write(dir.path().join(".profile"), "#!/bin/sh\n").unwrap();
        assert_eq!(
            language_in(dir.path(), Path::new(".profile")),
            Some(Language::Shell)
        );
    }

    #[test]
    fn a_scan_owns_scripts_by_their_shebang() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("tools")).unwrap();
        fs::write(dir.path().join("tools/.arch"), "node: tools\n").unwrap();
        fs::write(dir.path().join("tools/deploy"), "#!/bin/bash\n").unwrap();
        fs::write(dir.path().join("tools/lint"), "#!/usr/bin/env python3\n").unwrap();
        fs::write(dir.path().join("tools/Makefile"), "all:\n").unwrap();
        fs::write(dir.path().join("tools/notes"), "#!/usr/bin/env node\n").unwrap();
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
        assert_eq!(sources, ["tools/deploy", "tools/lint"]);
        assert_eq!(assets, ["tools/Makefile", "tools/notes"]);
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
