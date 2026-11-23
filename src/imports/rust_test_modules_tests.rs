//! Rust files that a `#[cfg(test)] mod x;` declaration pulls in are test code.

use super::fixtures::{Repo, repo};

/// (line, target, `test_only`) of every resolved import of `file`.
fn flagged(repo: &Repo, file: &str) -> Vec<(u32, String, bool)> {
    repo.imports(file)
        .resolved
        .iter()
        .map(|import| {
            (
                import.at.line,
                import.target.display().to_string(),
                import.test_only,
            )
        })
        .collect()
}

#[test]
fn a_file_declared_by_an_attributed_mod_is_test_only() {
    let repo = repo(&[
        ("Cargo.toml", ""),
        ("src/lib.rs", "pub mod a;\n#[cfg(test)]\nmod fixtures;\n"),
        ("src/a.rs", "pub fn f() {}\n"),
        ("src/fixtures.rs", "use crate::a::f;\n"),
    ]);
    assert_eq!(
        flagged(&repo, "src/fixtures.rs"),
        [(1, "src/a.rs".to_owned(), true)]
    );
}

#[test]
fn the_name_of_the_module_does_not_matter_and_mod_rs_files_count() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "pub mod a;\n#[cfg(test)]\nmod anything_at_all;\n",
        ),
        ("src/a.rs", ""),
        ("src/anything_at_all/mod.rs", "use crate::a;\n"),
    ]);
    assert_eq!(
        flagged(&repo, "src/anything_at_all/mod.rs"),
        [(1, "src/a.rs".to_owned(), true)]
    );
}

#[test]
fn a_declaration_in_a_nested_module_follows_the_module_path() {
    let repo = repo(&[
        ("src/lib.rs", "pub mod a;\npub mod b;\n"),
        ("src/a.rs", "#[cfg(test)]\nmod tests;\n"),
        ("src/a/tests.rs", "use crate::b::g;\n"),
        ("src/b.rs", "pub fn g() {}\n#[cfg(test)]\nmod helpers;\n"),
        ("src/b/helpers.rs", "use crate::a;\n"),
    ]);
    assert_eq!(
        flagged(&repo, "src/a/tests.rs"),
        [(1, "src/b.rs".to_owned(), true)]
    );
    assert_eq!(
        flagged(&repo, "src/b/helpers.rs"),
        [(1, "src/a.rs".to_owned(), true)]
    );
}

#[test]
fn a_declaration_inside_an_inline_module_follows_its_name() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "pub mod a;\n#[cfg(test)]\nmod inner {\n    mod deep;\n}\n",
        ),
        ("src/a.rs", ""),
        ("src/inner/deep.rs", "use crate::a;\n"),
    ]);
    assert_eq!(
        flagged(&repo, "src/inner/deep.rs"),
        [(1, "src/a.rs".to_owned(), true)]
    );
}

#[test]
fn the_modules_of_a_test_module_are_test_code_too() {
    let repo = repo(&[
        ("src/lib.rs", "pub mod a;\n#[cfg(test)]\nmod tests;\n"),
        ("src/a.rs", ""),
        ("src/tests.rs", "mod support;\n"),
        ("src/tests/support.rs", "use crate::a;\n"),
    ]);
    assert_eq!(
        flagged(&repo, "src/tests/support.rs"),
        [(1, "src/a.rs".to_owned(), true)]
    );
}

#[test]
fn a_file_named_tests_rs_is_test_only_without_any_declaration() {
    let repo = repo(&[
        ("src/lib.rs", "pub mod a;\nmod tests;\n"),
        ("src/a.rs", ""),
        ("src/tests.rs", "use crate::a;\n"),
    ]);
    assert_eq!(
        flagged(&repo, "src/tests.rs"),
        [(1, "src/a.rs".to_owned(), true)]
    );
}

#[test]
fn modules_without_the_attribute_or_with_another_predicate_stay_runtime() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "pub mod a;\nmod plain;\n#[cfg(not(test))]\nmod negated;\n#[cfg(any(test, unix))]\nmod either;\n#[cfg(test)]\nmod missing;\n",
        ),
        ("src/a.rs", ""),
        ("src/plain.rs", "use crate::a;\n"),
        ("src/negated.rs", "use crate::a;\n"),
        ("src/either.rs", "use crate::a;\n"),
    ]);
    for file in ["src/plain.rs", "src/negated.rs", "src/either.rs"] {
        assert_eq!(
            flagged(&repo, file),
            [(1, "src/a.rs".to_owned(), false)],
            "{file}"
        );
    }
}

#[test]
fn a_test_module_does_not_make_its_declarer_test_only() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "use crate::a::f;\n#[cfg(test)]\nmod tests;\npub mod a;\n",
        ),
        ("src/a.rs", "pub fn f() {}\n"),
        ("src/tests.rs", ""),
    ]);
    assert_eq!(
        flagged(&repo, "src/lib.rs"),
        [(1, "src/a.rs".to_owned(), false)]
    );
}
