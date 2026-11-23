//! The symbols TypeScript and JavaScript imports use.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::*;
use crate::model::ImportKind;

fn known(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

/// The (line, target, symbols) of every resolved import of `source` written in `src/main.<ext>`.
fn used(source: &str, language: Language, ext: &str) -> Vec<(u32, String, Vec<String>)> {
    let file = format!("src/main.{ext}");
    let repo = known(&["src/a.ts", "src/b.ts", "src/a.js", "src/b.js"]);
    let got = extract(source, language, Path::new(&file), &repo).expect("extract succeeds");
    got.resolved
        .iter()
        .map(|import| {
            (
                import.at.line,
                import.target.display().to_string(),
                import.symbols.iter().map(ToString::to_string).collect(),
            )
        })
        .collect()
}

fn ts(source: &str) -> Vec<String> {
    let got = used(source, Language::TypeScript, "ts");
    assert_eq!(got.len(), 1, "{source}: {got:?}");
    got.into_iter()
        .flat_map(|(_, _, symbols)| symbols)
        .collect()
}

fn js(source: &str) -> Vec<String> {
    let got = used(source, Language::JavaScript, "js");
    assert_eq!(got.len(), 1, "{source}: {got:?}");
    got.into_iter()
        .flat_map(|(_, _, symbols)| symbols)
        .collect()
}

#[test]
fn named_imports_record_the_exported_name() {
    assert_eq!(ts("import { x } from './a';"), ["x"]);
    assert_eq!(ts("import { y, x } from './a';"), ["x", "y"]);
    assert_eq!(ts("import { x as local } from './a';"), ["x"]);
    assert_eq!(ts("import { x as y, z as w } from './a';"), ["x", "z"]);
}

#[test]
fn default_import_records_default() {
    assert_eq!(ts("import x from './a';"), ["default"]);
    assert_eq!(js("import x from './a';"), ["default"]);
    assert_eq!(ts("import { default as x } from './a';"), ["default"]);
}

#[test]
fn default_and_named_import_record_both() {
    assert_eq!(ts("import x, { y } from './a';"), ["default", "y"]);
    assert_eq!(
        ts("import x, { y as z, w } from './a';"),
        ["default", "w", "y"]
    );
}

#[test]
fn namespace_import_records_all() {
    assert_eq!(ts("import * as ns from './a';"), ["*"]);
    assert_eq!(js("import * as ns from './a';"), ["*"]);
    assert_eq!(ts("import x, * as ns from './a';"), ["default", "*"]);
}

#[test]
fn type_imports_record_the_type_names() {
    assert_eq!(ts("import type { T } from './a';"), ["T"]);
    assert_eq!(ts("import { type T, U } from './a';"), ["T", "U"]);
    assert_eq!(ts("import type X from './a';"), ["default"]);
    assert_eq!(ts("import type * as ns from './a';"), ["*"]);
}

#[test]
fn side_effect_import_records_nothing() {
    assert_eq!(ts("import './a';"), Vec::<String>::new());
    assert_eq!(js("import './a';"), Vec::<String>::new());
    let got = used("import './a';", Language::TypeScript, "ts");
    assert_eq!(got, [(1, "src/a.ts".to_owned(), Vec::new())]);
}

#[test]
fn named_re_exports_record_the_exported_name() {
    assert_eq!(ts("export { x } from './a';"), ["x"]);
    assert_eq!(ts("export { x as y, z } from './a';"), ["x", "z"]);
    assert_eq!(ts("export { default } from './a';"), ["default"]);
    assert_eq!(ts("export { default as x } from './a';"), ["default"]);
    assert_eq!(ts("export type { T } from './a';"), ["T"]);
}

#[test]
fn export_star_records_all() {
    assert_eq!(ts("export * from './a';"), ["*"]);
    assert_eq!(js("export * from './a';"), ["*"]);
    assert_eq!(ts("export * as ns from './a';"), ["*"]);
}

#[test]
fn require_and_dynamic_import_record_all() {
    assert_eq!(js("const a = require('./a');"), ["*"]);
    assert_eq!(ts("const m = await import('./a');"), ["*"]);
    assert_eq!(js("import('./a').then(f);"), ["*"]);
    assert_eq!(ts("import a = require('./a');"), ["*"]);
}

#[test]
fn a_destructured_require_records_the_exported_names() {
    assert_eq!(js("const { a, b: c } = require('./a');"), ["a", "b"]);
    assert_eq!(js("const { x } = require('./a');"), ["x"]);
    assert_eq!(js("const { 'x-y': z } = require('./a');"), ["x-y"]);
    assert_eq!(js("const { x = 1 } = require('./a');"), ["x"]);
    assert_eq!(js("var { y } = require('./a');"), ["y"]);
}

#[test]
fn a_destructured_dynamic_import_records_the_exported_names() {
    assert_eq!(ts("const { a } = await import('./a');"), ["a"]);
    assert_eq!(ts("const { a, b: c } = (await import('./a'));"), ["a", "b"]);
    assert_eq!(js("const { a } = await import('./a');"), ["a"]);
}

#[test]
fn a_rest_element_or_a_computed_key_in_the_pattern_adds_all() {
    assert_eq!(js("const { a, ...rest } = require('./a');"), ["a", "*"]);
    assert_eq!(js("const { [k]: v } = require('./a');"), ["*"]);
}

#[test]
fn a_require_that_is_not_destructured_stays_all() {
    assert_eq!(js("const m = require('./a');"), ["*"]);
    assert_eq!(js("const { x } = require('./a').y;"), ["*"]);
    assert_eq!(js("const [x] = require('./a');"), ["*"]);
    assert_eq!(js("f({ x } = require('./a'));"), ["*"]);
}

#[test]
fn two_statements_on_one_line_for_one_file_union_their_symbols() {
    let got = used(
        "import { x } from './a'; import { y } from './a'; import { z } from './b';",
        Language::TypeScript,
        "ts",
    );
    assert_eq!(
        got,
        [
            (
                1,
                "src/a.ts".to_owned(),
                vec!["x".to_owned(), "y".to_owned()]
            ),
            (1, "src/b.ts".to_owned(), vec!["z".to_owned()]),
        ]
    );
}

#[test]
fn a_side_effect_import_joins_a_named_one_on_the_same_line() {
    let got = used(
        "import './a'; import { x } from './a';",
        Language::TypeScript,
        "ts",
    );
    assert_eq!(got, [(1, "src/a.ts".to_owned(), vec!["x".to_owned()])]);
}

#[test]
fn different_lines_stay_separate_entries() {
    let got = used(
        "import { x } from './a';\nimport { y } from './a';\nimport { x } from './a';\n",
        Language::TypeScript,
        "ts",
    );
    assert_eq!(
        got,
        [
            (1, "src/a.ts".to_owned(), vec!["x".to_owned()]),
            (2, "src/a.ts".to_owned(), vec!["y".to_owned()]),
            (3, "src/a.ts".to_owned(), vec!["x".to_owned()]),
        ]
    );
}

#[test]
fn symbols_are_sorted_with_all_last() {
    assert_eq!(ts("import b, * as ns from './a';"), ["default", "*"]);
    assert_eq!(ts("import { z, a, m } from './a';"), ["a", "m", "z"]);
}

fn kinds(source: &str) -> Vec<ImportKind> {
    let file = Path::new("src/main.ts");
    let got = extract(source, Language::TypeScript, file, &known(&["src/a.ts"])).unwrap();
    got.resolved.iter().map(|import| import.kind).collect()
}

#[test]
fn an_import_in_a_type_is_type_only_and_uses_its_member() {
    for (source, symbols) in [
        ("let x: import('./a').T = 1;", &["T"][..]),
        ("let x: import('./a').A.B = 1;", &["A"]),
        ("type Z = typeof import('./a');", &["*"]),
        ("interface I { s: import('./a').Scene; }", &["Scene"]),
        ("class C { s: import('./a').Scene = x; }", &["Scene"]),
        ("let y = z as import('./a').T;", &["T"]),
    ] {
        let got = used(source, Language::TypeScript, "ts");
        assert_eq!(got.len(), 1, "{source}: {got:?}");
        assert_eq!(got[0].2, symbols, "{source}");
        assert_eq!(kinds(source), [ImportKind::TypeOnly], "{source}");
    }
}

#[test]
fn an_import_call_in_an_expression_stays_runtime() {
    for source in [
        "const m = import('./a');",
        "let x: T = await import('./a');",
        "const a = (await import('./a')).T;",
        "import('./a').then(f);",
    ] {
        assert_eq!(kinds(source), [ImportKind::Runtime], "{source}");
    }
}
