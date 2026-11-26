//! `FileImports::test_file`: which files count as test code by name and place.

use std::path::Path;

use super::fixtures::repo;
use crate::code::Language;
use crate::reading::imports::language::test_file;

#[test]
fn files_are_test_code_by_the_rules_of_their_language() {
    let cases = [
        (Language::TypeScript, "a/x.test.ts", true),
        (Language::TypeScript, "a/x.spec.tsx", true),
        (Language::JavaScript, "a/__tests__/x.js", true),
        (Language::TypeScript, "test/x.ts", true),
        (Language::Svelte, "a/b.test.svelte", true),
        (Language::Html, "a/tests/page.html", true),
        (Language::TypeScript, "a/x.ts", false),
        (Language::TypeScript, "a/test.ts", false),
        (Language::Python, "a/test_x.py", true),
        (Language::Python, "a/x_test.py", true),
        (Language::Python, "a/conftest.py", true),
        (Language::Python, "tests/helpers.py", true),
        (Language::Python, "a/x.py", false),
        (Language::Rust, "src/tests.rs", true),
        (Language::Rust, "src/a_tests.rs", true),
        (Language::Rust, "tests/common/mod.rs", true),
        (Language::Rust, "src/a.rs", false),
        (Language::Swift, "Tests/AppTests/A.swift", true),
        (Language::Swift, "Sources/A.swift", false),
        (Language::Shell, "scripts/test_x.sh", true),
        (Language::Shell, "scripts/x_test.sh", true),
        (Language::Shell, "test/run.sh", true),
        (Language::Shell, "scripts/x.sh", false),
    ];
    for (language, file, expected) in cases {
        assert_eq!(test_file(language, Path::new(file)), expected, "{file}");
    }
}

#[test]
fn the_flag_is_on_the_file_imports_of_every_scanned_file() {
    let repo = repo(&[
        ("a/x.ts", "import './x.test';\n"),
        ("a/x.test.ts", "export const t = 1;\n"),
    ]);
    assert!(!repo.imports("a/x.ts").test_file);
    assert!(repo.imports("a/x.test.ts").test_file);
}

#[test]
fn a_file_a_cfg_test_module_declaration_pulls_in_is_test_code() {
    let repo = repo(&[
        ("src/lib.rs", "#[cfg(test)]\nmod fixtures;\nmod plain;\n"),
        ("src/fixtures.rs", "pub fn f() {}\n"),
        ("src/plain.rs", "pub fn p() {}\n"),
    ]);
    assert!(repo.imports("src/fixtures.rs").test_file);
    assert!(!repo.imports("src/plain.rs").test_file);
    assert!(!repo.imports("src/lib.rs").test_file);
}
