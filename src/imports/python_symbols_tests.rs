//! The symbols Python imports use.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::*;

/// The (line, target, symbols) of every resolved import of `source` written in `file`.
fn used(source: &str, file: &str, repo: &[&str]) -> Vec<(u32, String, Vec<String>)> {
    let known: BTreeSet<PathBuf> = repo.iter().map(PathBuf::from).collect();
    let got = extract(source, Language::Python, Path::new(file), &known).expect("extract succeeds");
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

fn entry(line: u32, target: &str, symbols: &[&str]) -> (u32, String, Vec<String>) {
    (
        line,
        target.to_owned(),
        symbols.iter().map(|s| (*s).to_owned()).collect(),
    )
}

#[test]
fn from_import_names_inside_a_module_record_the_names() {
    let got = used("from a import b, c", "app/main.py", &["a.py"]);
    assert_eq!(got, [entry(1, "a.py", &["b", "c"])]);
}

#[test]
fn from_import_alias_records_the_original_name() {
    let got = used("from a import b as x, c", "app/main.py", &["a.py"]);
    assert_eq!(got, [entry(1, "a.py", &["b", "c"])]);
}

#[test]
fn from_import_in_a_package_init_records_the_names() {
    let got = used("from a import b", "app/main.py", &["a/__init__.py"]);
    assert_eq!(got, [entry(1, "a/__init__.py", &["b"])]);
}

#[test]
fn from_import_of_a_submodule_records_no_symbol() {
    let got = used(
        "from a import b",
        "app/main.py",
        &["a/__init__.py", "a/b.py"],
    );
    assert_eq!(got, [entry(1, "a/b.py", &[])]);
}

#[test]
fn from_import_of_a_subpackage_records_no_symbol() {
    let got = used(
        "from a import b",
        "app/main.py",
        &["a.py", "a/b/__init__.py"],
    );
    assert_eq!(got, [entry(1, "a/b/__init__.py", &[])]);
}

#[test]
fn from_import_mixing_a_submodule_and_a_name_records_each_against_its_file() {
    let got = used(
        "from a import b, c, d",
        "app/main.py",
        &["a/__init__.py", "a/c.py"],
    );
    assert_eq!(
        got,
        [
            entry(1, "a/__init__.py", &["b", "d"]),
            entry(1, "a/c.py", &[]),
        ]
    );
}

#[test]
fn from_import_star_records_all() {
    let got = used("from a.b import *", "app/main.py", &["a/b.py", "a/b/c.py"]);
    assert_eq!(got, [entry(1, "a/b.py", &["*"])]);
}

#[test]
fn plain_import_records_no_symbol() {
    assert_eq!(
        used("import a.b", "app/main.py", &["a/b.py"]),
        [entry(1, "a/b.py", &[])]
    );
    assert_eq!(
        used("import a.b as x", "app/main.py", &["a/b.py"]),
        [entry(1, "a/b.py", &[])]
    );
    assert_eq!(
        used("import a, b", "app/main.py", &["a.py", "b.py"]),
        [entry(1, "a.py", &[]), entry(1, "b.py", &[])]
    );
}

#[test]
fn relative_from_import_follows_the_same_rule() {
    let repo = ["pkg/__init__.py", "pkg/x.py", "pkg/a.py"];
    assert_eq!(
        used("from . import x", "pkg/m.py", &repo),
        [entry(1, "pkg/x.py", &[])]
    );
    assert_eq!(
        used("from . import y", "pkg/m.py", &repo),
        [entry(1, "pkg/__init__.py", &["y"])]
    );
    assert_eq!(
        used("from .a import f", "pkg/m.py", &repo),
        [entry(1, "pkg/a.py", &["f"])]
    );
    assert_eq!(
        used("from . import *", "pkg/m.py", &repo),
        [entry(1, "pkg/__init__.py", &["*"])]
    );
}

#[test]
fn parenthesised_names_on_several_lines_are_one_entry_at_the_first_line() {
    let got = used(
        "from a import (\n    b,\n    c as d,\n)\n",
        "app/main.py",
        &["a.py"],
    );
    assert_eq!(got, [entry(1, "a.py", &["b", "c"])]);
}

#[test]
fn separate_statements_stay_separate_entries() {
    let got = used(
        "from a import b\nfrom a import c\n",
        "app/main.py",
        &["a.py"],
    );
    assert_eq!(got, [entry(1, "a.py", &["b"]), entry(2, "a.py", &["c"])]);
}

#[test]
fn a_star_import_is_the_only_python_import_that_records_all() {
    let repo = ["a/__init__.py", "a/b.py"];
    assert_eq!(
        used(
            "from a import *\nfrom a import b\nimport a.b",
            "app/main.py",
            &repo
        ),
        [
            entry(1, "a/__init__.py", &["*"]),
            entry(2, "a/b.py", &[]),
            entry(3, "a/b.py", &[]),
        ]
    );
}
