//! References that are recorded and never dropped: `import.meta.glob`, `require.resolve`,
//! `createRequire`, an aliased `require`; and the ones that are followed: `/// <reference path>`
//! and `import(..)` in Svelte markup.

use super::fixtures::repo;
use crate::code::{ImportKind, SkipReason};

/// The (line, message) of every dynamic-specifier skip of `file` in a repository of `files`.
fn notes(files: &[(&str, &str)], file: &str) -> Vec<(u32, String)> {
    repo(files)
        .imports(file)
        .skipped
        .iter()
        .filter(|skip| skip.reason == SkipReason::DynamicSpecifier)
        .map(|skip| (skip.at.line, skip.message.clone().unwrap_or_default()))
        .collect()
}

fn targets(files: &[(&str, &str)], file: &str) -> Vec<(u32, String, ImportKind)> {
    repo(files)
        .imports(file)
        .resolved
        .iter()
        .map(|i| (i.at.line, i.target.display().to_string(), i.kind))
        .collect()
}

#[test]
fn import_meta_glob_is_a_skipped_reference() {
    let got = notes(
        &[(
            "src/a.ts",
            "const m = import.meta.glob('./x/*.ts');\nconst e = import.meta.globEager('./y/*.ts');\n",
        )],
        "src/a.ts",
    );
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0, 1);
    assert!(got[0].1.contains("import.meta.glob"), "{got:?}");
    assert_eq!(got[1].0, 2);
}

#[test]
fn require_resolve_is_a_skipped_reference() {
    let got = notes(
        &[
            ("src/a.js", "const p = require.resolve('./b');\n"),
            ("src/b.js", ""),
        ],
        "src/a.js",
    );
    assert_eq!(got.len(), 1);
    assert!(got[0].1.contains("require.resolve"), "{got:?}");
}

#[test]
fn create_require_and_an_aliased_require_are_skipped_references() {
    let source = "import { createRequire } from 'node:module';\nconst req = createRequire(import.meta.url);\nconst r = require;\nconst once = createRequire(import.meta.url)('./b');\nreq('./b');\n";
    let got = notes(&[("src/a.js", source), ("src/b.js", "")], "src/a.js");
    let lines: Vec<u32> = got.iter().map(|(line, _)| *line).collect();
    assert_eq!(lines, [2, 3, 4]);
    assert!(got[0].1.contains("`req`"), "{got:?}");
    assert!(got[1].1.contains("`r`"), "{got:?}");
}

#[test]
fn a_require_made_by_create_require_and_called_require_is_followed() {
    let source = "import { createRequire } from 'node:module';\nconst require = createRequire(import.meta.url);\nrequire('./b');\n";
    let files = [("src/a.js", source), ("src/b.js", "")];
    assert_eq!(notes(&files, "src/a.js"), []);
    assert_eq!(
        targets(&files, "src/a.js"),
        [(3, "src/b.js".to_owned(), ImportKind::Runtime)]
    );
}

#[test]
fn a_plain_require_import_and_new_url_are_not_skipped() {
    let source = "const b = require('./b');\nimport('./b');\nnew URL('./b.js', import.meta.url);\n";
    assert_eq!(
        notes(&[("src/a.js", source), ("src/b.js", "")], "src/a.js"),
        []
    );
}

#[test]
fn a_triple_slash_reference_path_is_a_type_only_import() {
    let files = [
        (
            "src/a.ts",
            "/// <reference path=\"./types.d.ts\" />\n/// <reference path=\"more.d.ts\" />\n/// <reference types=\"node\" />\nexport {};\n",
        ),
        ("src/types.d.ts", "declare const x: number;\n"),
        ("src/more.d.ts", "declare const y: number;\n"),
    ];
    assert_eq!(
        targets(&files, "src/a.ts"),
        [
            (1, "src/types.d.ts".to_owned(), ImportKind::TypeOnly),
            (2, "src/more.d.ts".to_owned(), ImportKind::TypeOnly),
        ]
    );
    assert_eq!(repo(&files).imports("src/a.ts").skipped, []);
}

#[test]
fn a_missing_reference_path_is_unresolved() {
    let files = [("src/a.ts", "/// <reference path=\"./gone.d.ts\" />\n")];
    assert_eq!(repo(&files).imports("src/a.ts").unresolved.len(), 1);
}

#[test]
fn svelte_markup_expressions_are_read_for_import_calls() {
    let page = "<script>\n  let x = 1;\n</script>\n\n{#await import('./b')}\n  <p>loading</p>\n{:then m}\n  <m.default />\n{/await}\n<button on:click={() => import('./c.js')}>go</button>\n<p>{x} {'{'}</p>\n<style>\n  p { color: red; }\n</style>\n";
    let files = [
        ("src/Page.svelte", page),
        ("src/b.ts", ""),
        ("src/c.ts", ""),
    ];
    let got = targets(&files, "src/Page.svelte");
    assert_eq!(
        got,
        [
            (5, "src/b.ts".to_owned(), ImportKind::Runtime),
            (10, "src/c.ts".to_owned(), ImportKind::Runtime),
        ]
    );
    // why: a template fragment that is not a whole program is no syntax error of the file.
    assert_eq!(repo(&files).imports("src/Page.svelte").skipped, []);
}

#[test]
fn a_computed_import_in_svelte_markup_is_a_dynamic_specifier() {
    let page = "<div>{#await import(name) then m}{/await}</div>\n";
    let got = notes(&[("src/Page.svelte", page)], "src/Page.svelte");
    assert_eq!(got.len(), 1);
}

#[test]
fn braces_inside_script_style_and_comments_are_not_expressions() {
    let page = "<!-- {import('./b')} -->\n<script>\n  const f = () => { return {}; };\n</script>\n<style>\n  a { b: c }\n</style>\n";
    let files = [("src/Page.svelte", page), ("src/b.ts", "")];
    assert_eq!(targets(&files, "src/Page.svelte"), []);
}
