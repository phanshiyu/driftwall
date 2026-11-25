//! `JSDoc` `import('x')` types in `/**` comments.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::*;
use crate::code::ImportKind;

fn known(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

/// (line, target, symbols, kind) of every resolved import of `source` written in `file`.
fn docs(
    source: &str,
    language: Language,
    file: &str,
) -> Vec<(u32, String, Vec<String>, ImportKind)> {
    let repo = known(&["src/types.ts", "src/x.ts", "src/util/y.ts"]);
    let got = extract(source, language, Path::new(file), &repo).expect("extract succeeds");
    got.resolved
        .iter()
        .map(|import| {
            (
                import.at.line,
                import.target.display().to_string(),
                import.symbols.iter().map(ToString::to_string).collect(),
                import.kind,
            )
        })
        .collect()
}

fn js(source: &str) -> Vec<(u32, String, Vec<String>, ImportKind)> {
    docs(source, Language::JavaScript, "src/util/main.js")
}

#[test]
fn a_type_import_with_a_member_is_type_only_and_uses_the_member() {
    let source = "/** @type {import('../../src/types').ImageManifest} */\nexport const m = {};\n";
    assert_eq!(
        js(source),
        [(
            1,
            "src/types.ts".to_owned(),
            vec!["ImageManifest".to_owned()],
            ImportKind::TypeOnly
        )]
    );
}

#[test]
fn a_type_import_without_a_member_uses_no_symbol() {
    let source = "\n/**\n * @param {import(\"../x\")} p\n */\nfunction f(p) {}\n";
    assert_eq!(
        js(source),
        [(3, "src/x.ts".to_owned(), Vec::new(), ImportKind::TypeOnly)]
    );
}

#[test]
fn returns_and_several_imports_in_one_comment_are_read_at_their_own_lines() {
    let source = "x;\n/**\n * @returns {import('../x').T}\n * @param {import('../types').A | import('../types').B} p\n */\n";
    assert_eq!(
        js(source),
        [
            (
                3,
                "src/x.ts".to_owned(),
                vec!["T".to_owned()],
                ImportKind::TypeOnly
            ),
            (
                4,
                "src/types.ts".to_owned(),
                vec!["A".to_owned(), "B".to_owned()],
                ImportKind::TypeOnly
            ),
        ]
    );
}

#[test]
fn typescript_files_read_jsdoc_too() {
    let source = "/** @type {import('../x').T} */\nconst a = 1;\n";
    assert_eq!(
        docs(source, Language::TypeScript, "src/util/main.ts"),
        [(
            1,
            "src/x.ts".to_owned(),
            vec!["T".to_owned()],
            ImportKind::TypeOnly
        )]
    );
}

#[test]
fn only_comments_that_start_with_two_stars_are_read() {
    for source in [
        "/* @type {import('../x').T} */\n",
        "// @type {import('../x').T}\n",
        "/*\n * @type {import('../x').T}\n */\n",
        "/**/ const a = 1;\n",
        "const s = \"/** @type {import('../x').T} */\";\n",
        "const t = `/** import('../x').T */`;\n",
    ] {
        assert_eq!(js(source), Vec::new(), "{source}");
    }
}

#[test]
fn a_mention_that_is_not_the_import_type_form_is_not_read() {
    for source in [
        "/** the important('../x') case */\n",
        "/** x.import('../x').T */\n",
        "/** import(name).T */\n",
        "/** import('../x' */\n",
        "/** import('../\\x').T */\n",
    ] {
        assert_eq!(js(source), Vec::new(), "{source}");
    }
}

#[test]
fn an_unresolvable_relative_type_import_is_unresolved_and_a_package_is_skipped() {
    let source = "/**\n * @type {import('./nope').T}\n * @type {import('left-pad').P}\n */\n";
    let got = extract(
        source,
        Language::JavaScript,
        Path::new("src/util/main.js"),
        &known(&[]),
    )
    .expect("extract succeeds");
    assert_eq!(got.resolved, Vec::new());
    assert_eq!(
        got.unresolved
            .iter()
            .map(|u| (u.at.line, u.specifier.as_str()))
            .collect::<Vec<_>>(),
        [(2, "./nope")]
    );
    assert_eq!(
        got.skipped
            .iter()
            .map(|s| (s.at.line, s.specifier.as_str(), s.reason))
            .collect::<Vec<_>>(),
        [(3, "left-pad", crate::code::SkipReason::UndeclaredPackage)]
    );
}

#[test]
fn a_jsdoc_import_in_a_test_file_is_test_only() {
    let repo = known(&["src/x.ts"]);
    let got = extract(
        "/** @type {import('../x').T} */\n",
        Language::JavaScript,
        Path::new("src/util/main.test.js"),
        &repo,
    )
    .expect("extract succeeds");
    assert_eq!(got.resolved.len(), 1);
    assert!(got.resolved[0].test_only);
}

#[test]
fn a_jsdoc_import_beside_a_real_import_of_the_same_file_stays_separate() {
    let source = "import { v } from '../x';\n/** @type {import('../x').T} */\n";
    let got = js(source);
    assert_eq!(
        got,
        [
            (
                1,
                "src/x.ts".to_owned(),
                vec!["v".to_owned()],
                ImportKind::Runtime
            ),
            (
                2,
                "src/x.ts".to_owned(),
                vec!["T".to_owned()],
                ImportKind::TypeOnly
            ),
        ]
    );
}
