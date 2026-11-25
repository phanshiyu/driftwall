//! `new URL('<literal>', import.meta.url)` file references.

use super::fixtures::repo;
use super::*;
use crate::code::{ImportKind, SkipReason};

fn summary(imports: &FileImports) -> Vec<(u32, String, ImportKind)> {
    imports
        .resolved
        .iter()
        .map(|i| (i.at.line, i.target.display().to_string(), i.kind))
        .collect()
}

#[test]
fn a_source_target_is_a_runtime_dependency_with_no_symbol() {
    let r = repo(&[
        (
            "test/cli.test.ts",
            "const bin = new URL('../bin/cli.ts', import.meta.url);\nspawn('x', [\n  new URL(`../bin/cli.ts`, import.meta.url).pathname,\n]);",
        ),
        ("bin/cli.ts", ""),
    ]);
    let got = r.imports("test/cli.test.ts");
    assert_eq!(
        summary(&got),
        [
            (1, "bin/cli.ts".to_owned(), ImportKind::Runtime),
            (3, "bin/cli.ts".to_owned(), ImportKind::Runtime),
        ]
    );
    assert!(got.resolved.iter().all(|i| i.symbols.is_empty()));
    assert_eq!(got.skipped, Vec::new());
}

#[test]
fn a_non_source_target_is_an_asset() {
    let r = repo(&[
        (
            "scripts/bus.mjs",
            "const c = new URL('../bus-config.json', import.meta.url);",
        ),
        ("bus-config.json", "{}"),
    ]);
    assert_eq!(
        summary(&r.imports("scripts/bus.mjs")),
        [(1, "bus-config.json".to_owned(), ImportKind::Asset)]
    );
}

#[test]
fn a_bare_literal_is_relative_to_the_file_and_nothing_is_probed() {
    let r = repo(&[
        (
            "src/a.ts",
            "new URL('w.js?x=1', import.meta.url);\nnew URL('w', import.meta.url);",
        ),
        ("src/w.js", ""),
        ("src/w.ts", ""),
    ]);
    let got = r.imports("src/a.ts");
    assert_eq!(
        summary(&got),
        [(1, "src/w.js".to_owned(), ImportKind::Runtime)]
    );
    assert_eq!(got.skipped.len(), 1);
    assert_eq!(got.skipped[0].reason, SkipReason::FileReferenceMissing);
    assert_eq!(got.skipped[0].at.line, 2);
}

#[test]
fn a_directory_missing_or_outside_target_is_skipped_not_unresolved() {
    let r = repo(&[
        (
            "src/a.ts",
            "new URL('./dir', import.meta.url);\nnew URL('./gone.png', import.meta.url);\nnew URL('../../x', import.meta.url);\nnew URL('/abs.png', import.meta.url);",
        ),
        ("src/dir/f.ts", ""),
    ]);
    let got = r.imports("src/a.ts");
    assert_eq!(got.resolved, Vec::new());
    assert_eq!(got.unresolved, Vec::new());
    let lines: Vec<_> = got.skipped.iter().map(|s| (s.at.line, s.reason)).collect();
    assert_eq!(
        lines,
        [1, 2, 3, 4].map(|l| (l, SkipReason::FileReferenceMissing))
    );
    let messages: Vec<_> = got
        .skipped
        .iter()
        .map(|s| s.message.as_deref().unwrap())
        .collect();
    assert_eq!(
        messages,
        [
            "`src/dir` is a directory",
            "`src/gone.png` has no file in the repository",
            "`../../x` leaves the repository",
            "`/abs.png` leaves the repository",
        ]
    );
}

#[test]
fn a_computed_first_argument_is_dynamic_and_a_url_scheme_is_ignored() {
    let r = repo(&[(
        "a.ts",
        "new URL(name, import.meta.url);\nnew URL(`./${x}`, import.meta.url);\nnew URL('https://e.com/a.js', import.meta.url);",
    )]);
    let got = r.imports("a.ts");
    assert_eq!(got.resolved, Vec::new());
    let reasons: Vec<_> = got.skipped.iter().map(|s| (s.at.line, s.reason)).collect();
    assert_eq!(reasons, [1, 2].map(|l| (l, SkipReason::DynamicSpecifier)));
}

#[test]
fn only_new_url_with_import_meta_url_counts() {
    let r = repo(&[
        (
            "a.ts",
            "new URL('./b.ts', base);\nnew Other('./b.ts', import.meta.url);\nnew URL('./b.ts');\nnew URL('./b.ts', import.meta.url, 1);",
        ),
        ("b.ts", ""),
    ]);
    let got = r.imports("a.ts");
    assert_eq!(
        (got.resolved, got.skipped, got.unresolved),
        (vec![], vec![], vec![])
    );
}

#[test]
fn svelte_scripts_are_read_too() {
    let r = repo(&[
        (
            "src/C.svelte",
            "<script>\nconst w = new URL('./w.ts', import.meta.url);\n</script>",
        ),
        ("src/w.ts", ""),
    ]);
    assert_eq!(
        summary(&r.imports("src/C.svelte")),
        [(2, "src/w.ts".to_owned(), ImportKind::Runtime)]
    );
}
