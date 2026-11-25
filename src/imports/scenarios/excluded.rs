//! Excluded files are known files: an import of one resolves to it and is never unresolved.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;

use super::*;
use crate::code::ImportKind;

/// Writes `files`, and collects the imports of the source ones among them, with `excluded` known
/// but not read.
fn collect_with(files: &[(&str, &str)], excluded: &[&str]) -> BTreeMap<PathBuf, FileImports> {
    let dir = TempDir::new().expect("tempdir");
    let mut sources = BTreeMap::new();
    for (path, contents) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(&full, contents).expect("write");
        let language = if path.ends_with("ts") {
            Language::TypeScript
        } else {
            Language::JavaScript
        };
        sources.insert(PathBuf::from(path), language);
    }
    let excluded: BTreeSet<PathBuf> = excluded.iter().map(PathBuf::from).collect();
    collect_with_assets(
        dir.path(),
        &sources,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &excluded,
    )
    .expect("collect")
}

#[test]
fn an_import_of_an_excluded_file_resolves_to_it_as_a_runtime_import_with_its_symbols() {
    let found = collect_with(
        &[(
            "a/x.mjs",
            "import {g} from './g.mjs'; import {h} from '../b/h.mjs';\n",
        )],
        &["a/g.mjs", "b/h.mjs"],
    );
    let imports = &found[Path::new("a/x.mjs")];
    assert_eq!(imports.unresolved, Vec::new());
    assert_eq!(imports.skipped, Vec::new());
    let got: Vec<_> = imports
        .resolved
        .iter()
        .map(|import| {
            (
                import.at.line,
                import.target.display().to_string(),
                import
                    .symbols
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                import.kind,
                import.test_only,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                1,
                "a/g.mjs".to_owned(),
                vec!["g".to_owned()],
                ImportKind::Runtime,
                false
            ),
            (
                1,
                "b/h.mjs".to_owned(),
                vec!["h".to_owned()],
                ImportKind::Runtime,
                false
            ),
        ]
    );
}

#[test]
fn an_excluded_mjs_wins_over_a_declaration_file_when_written_with_its_extension() {
    let files = [
        ("a/textfit.mjs", "import { w } from './font-metrics.mjs';\n"),
        ("a/font-metrics.d.mts", "export const w: number;\n"),
    ];
    let with = collect_with(&files, &["a/font-metrics.mjs"]);
    let targets: Vec<_> = with[Path::new("a/textfit.mjs")]
        .resolved
        .iter()
        .map(|import| import.target.display().to_string())
        .collect();
    assert_eq!(targets, ["a/font-metrics.mjs"]);

    let without = collect_with(&files, &[]);
    let targets: Vec<_> = without[Path::new("a/textfit.mjs")]
        .resolved
        .iter()
        .map(|import| import.target.display().to_string())
        .collect();
    assert_eq!(targets, ["a/font-metrics.d.mts"]);
}

#[test]
fn an_excluded_file_is_probed_by_extension_and_as_a_directory_index() {
    let found = collect_with(
        &[(
            "a/x.ts",
            "import './g';\nimport './dir';\nimport './h.js';\n",
        )],
        &["a/g.js", "a/dir/index.js", "a/h.ts"],
    );
    let targets: Vec<_> = found[Path::new("a/x.ts")]
        .resolved
        .iter()
        .map(|import| import.target.display().to_string())
        .collect();
    assert_eq!(targets, ["a/g.js", "a/dir/index.js", "a/h.ts"]);
}

#[test]
fn an_excluded_file_is_never_read() {
    // why: the excluded file is not on disk at all, so reading it would fail the collection.
    let found = collect_with(&[("a/x.js", "import './g.js';\n")], &["a/g.js"]);
    assert_eq!(found.len(), 1);
}

#[test]
fn an_import_that_names_no_file_stays_unresolved() {
    let found = collect_with(&[("a/x.js", "import './nope.js';\n")], &["a/g.js"]);
    assert_eq!(found[Path::new("a/x.js")].unresolved.len(), 1);
}
