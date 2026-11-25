//! Probing: declaration files, and bundler query suffixes that are not part of the file name.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::*;
use crate::code::SkipReason;

const NONE: [&str; 0] = [];

fn run(source: &str, file: &str, repo: &[&str]) -> FileImports {
    let known: BTreeSet<PathBuf> = repo.iter().map(PathBuf::from).collect();
    extract(source, Language::TypeScript, Path::new(file), &known).expect("extract succeeds")
}

fn targets(imports: &FileImports) -> Vec<String> {
    imports
        .resolved
        .iter()
        .map(|r| r.target.display().to_string())
        .collect()
}

fn unresolved(imports: &FileImports) -> Vec<&str> {
    imports
        .unresolved
        .iter()
        .map(|u| u.specifier.as_str())
        .collect()
}

fn skipped(imports: &FileImports) -> Vec<(&str, SkipReason)> {
    imports
        .skipped
        .iter()
        .map(|s| (s.specifier.as_str(), s.reason))
        .collect()
}

#[test]
fn a_declaration_file_resolves_a_type_import() {
    let got = run(
        "import type { D } from '../types/decl';",
        "src/main.ts",
        &["types/decl.d.ts"],
    );
    assert_eq!(targets(&got), ["types/decl.d.ts"]);
    assert_eq!(unresolved(&got), NONE);
}

#[test]
fn declaration_files_resolve_for_every_module_flavour() {
    for (specifier, file) in [
        ("./a", "src/a.d.ts"),
        ("./a", "src/a.d.mts"),
        ("./a", "src/a.d.cts"),
        ("./a.js", "src/a.d.ts"),
        ("./a.mjs", "src/a.d.mts"),
        ("./a.cjs", "src/a.d.cts"),
        ("./dir", "src/dir/index.d.ts"),
    ] {
        let got = run(&format!("import '{specifier}';"), "src/main.ts", &[file]);
        assert_eq!(targets(&got), [file], "{specifier} -> {file}");
    }
}

#[test]
fn a_declaration_file_comes_after_the_typescript_files_of_its_kind() {
    let got = run(
        "import './a';\nimport './b';",
        "src/main.ts",
        &["src/a.d.ts", "src/a.ts", "src/b.d.ts", "src/b.tsx"],
    );
    assert_eq!(targets(&got), ["src/a.ts", "src/b.tsx"]);
}

#[test]
fn a_declaration_file_is_not_found_for_a_jsx_specifier() {
    let got = run("import './a.jsx';", "src/main.ts", &["src/a.d.ts"]);
    assert_eq!(unresolved(&got), ["./a.jsx"]);
}

#[test]
fn query_and_fragment_suffixes_are_not_part_of_the_file_name() {
    let repo = ["src/a.ts", "src/w.ts", "src/b.js"];
    for specifier in [
        "./a?raw",
        "./a?url",
        "./a?inline",
        "./w.ts?worker",
        "./a?x=1&y=2",
        "./a#frag",
        "./a?raw#frag",
    ] {
        let got = run(&format!("import '{specifier}';"), "src/main.ts", &repo);
        assert_eq!(targets(&got).len(), 1, "{specifier}: {got:?}");
        assert_eq!(unresolved(&got), NONE, "{specifier}");
    }
}

#[test]
fn a_stripped_specifier_that_resolves_to_nothing_is_reported_as_written() {
    let got = run("import './gone?raw';", "src/main.ts", &["src/a.ts"]);
    assert_eq!(unresolved(&got), ["./gone?raw"]);
}

#[test]
fn a_leading_hash_is_a_package_imports_key_not_a_fragment() {
    let got = run("import '#internal/x';", "src/main.ts", &["src/a.ts"]);
    assert_eq!(
        skipped(&got),
        [("#internal/x", SkipReason::UndeclaredPackage)]
    );
}

#[test]
fn a_query_does_not_hide_a_package_from_the_package_check() {
    let got = run("import 'pkg/x.css?inline';", "src/main.ts", &[]);
    assert_eq!(
        skipped(&got),
        [("pkg/x.css?inline", SkipReason::UndeclaredPackage)]
    );
}
