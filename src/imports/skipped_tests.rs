//! Skipped references (`undeclared-package`, `dynamic-specifier`, `embed-non-source`), package
//! discovery, and Rust embeds.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::model::ImportKind::{Embed, Runtime};
use crate::model::SkipReason::{DynamicSpecifier, EmbedNonSource, UndeclaredPackage};
use crate::model::{ImportKind, Language, SkipReason};

fn known(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

/// Every skipped reference as (line, specifier, reason).
fn skips(imports: &FileImports) -> Vec<(u32, &str, SkipReason)> {
    imports
        .skipped
        .iter()
        .map(|s| (s.at.line, s.specifier.as_str(), s.reason))
        .collect()
}

fn declared(entries: &[(&str, &[&str])]) -> BTreeMap<PathBuf, BTreeSet<String>> {
    entries
        .iter()
        .map(|(dir, names)| {
            (
                PathBuf::from(dir),
                names.iter().map(|n| (*n).to_owned()).collect(),
            )
        })
        .collect()
}

/// Extracts `source` as `file` with the given declared packages and no aliases.
fn run(
    source: &str,
    language: Language,
    file: &str,
    packages: &BTreeMap<PathBuf, BTreeSet<String>>,
) -> FileImports {
    let known = known(&["src/a.ts", "src/a.js"]);
    let crates = BTreeMap::new();
    let aliases = BTreeMap::new();
    let env = Environment {
        known: &known,
        assets: &BTreeSet::new(),
        crates: &crates,
        aliases: &aliases,
        packages,
        workspaces: &Workspaces::default(),
        projects: &BTreeMap::new(),
        python: &PythonLayout::default(),
        swift: &SwiftModules::default(),
        vite_roots: &BTreeMap::new(),
        vite_aliases: &BTreeMap::new(),
        test_files: &BTreeSet::new(),
    };
    extract_in(source, language, Path::new(file), &env).expect("extract succeeds")
}

fn ts(source: &str, packages: &[(&str, &[&str])]) -> FileImports {
    run(
        source,
        Language::TypeScript,
        "src/main.ts",
        &declared(packages),
    )
}

#[test]
fn a_declared_package_is_silently_ignored() {
    let got = ts(
        "import a from 'react';\nimport b from 'lodash';",
        &[("", &["react", "lodash"])],
    );
    assert_eq!(got, FileImports::default());
}

#[test]
fn an_undeclared_package_is_skipped_at_its_statement_line() {
    let got = ts(
        "import a from 'react';\nimport {\n  b,\n} from\n  'lodash';\nexport * from 'zod';",
        &[("", &["react"])],
    );
    assert_eq!(
        skips(&got),
        [
            (2, "lodash", UndeclaredPackage),
            (6, "zod", UndeclaredPackage)
        ]
    );
}

#[test]
fn scoped_and_subpath_specifiers_match_their_package() {
    let packages = [("", &["@a/b", "pkg"][..])];
    let got = ts(
        "import a from '@a/b';\nimport b from '@a/b/sub/deep';\nimport c from 'pkg/sub';\nimport d from 'pkg';",
        &packages,
    );
    assert_eq!(got, FileImports::default());
    let got = ts(
        "import a from '@a/c';\nimport b from '@a/c/sub';\nimport c from 'pkg2/sub';\nimport d from '@a';",
        &packages,
    );
    assert_eq!(
        skips(&got),
        [
            (1, "@a/c", UndeclaredPackage),
            (2, "@a/c/sub", UndeclaredPackage),
            (3, "pkg2/sub", UndeclaredPackage),
            (4, "@a", UndeclaredPackage),
        ]
    );
}

#[test]
fn a_package_name_that_only_starts_like_a_declared_one_is_undeclared() {
    let got = ts("import a from 'react-dom';", &[("", &["react"])]);
    assert_eq!(skips(&got), [(1, "react-dom", UndeclaredPackage)]);
}

#[test]
fn node_builtins_are_never_skipped() {
    let got = ts(
        "import a from 'fs';\nimport b from 'node:path';\nimport c from 'fs/promises';\nimport d from 'node:test';\nimport e from 'node:fs/promises';\nimport f from 'child_process';",
        &[],
    );
    assert_eq!(got, FileImports::default());
}

#[test]
fn a_bare_name_that_only_exists_with_the_node_prefix_is_a_package() {
    let got = ts("import a from 'test';", &[]);
    assert_eq!(skips(&got), [(1, "test", UndeclaredPackage)]);
}

#[test]
fn a_dependency_declared_in_any_ancestor_package_json_counts() {
    let packages = [
        ("", &["react", "zod"][..]),
        ("packages/web", &["lodash"][..]),
    ];
    let got = run(
        "import a from 'react';\nimport b from 'lodash';\nimport c from 'left-pad';",
        Language::TypeScript,
        "packages/web/src/page.ts",
        &declared(&packages),
    );
    assert_eq!(skips(&got), [(3, "left-pad", UndeclaredPackage)]);
}

#[test]
fn a_package_json_that_is_not_an_ancestor_declares_nothing() {
    let packages = [("", &["react"][..]), ("packages/web", &["lodash"][..])];
    let got = run(
        "import a from 'react';\nimport b from 'lodash';",
        Language::TypeScript,
        "packages/other/page.ts",
        &declared(&packages),
    );
    assert_eq!(skips(&got), [(2, "lodash", UndeclaredPackage)]);
}

#[test]
fn require_and_dynamic_import_of_a_package_follow_the_same_rule() {
    let got = run(
        "const a = require('left-pad');\nconst b = await import('chalk');\nconst c = require('react');",
        Language::JavaScript,
        "src/main.js",
        &declared(&[("", &["react"])]),
    );
    assert_eq!(
        skips(&got),
        [
            (1, "left-pad", UndeclaredPackage),
            (2, "chalk", UndeclaredPackage)
        ]
    );
}

#[test]
fn an_aliased_specifier_is_never_skipped_even_when_nothing_backs_it() {
    let known = known(&["src/foo.ts"]);
    let crates = BTreeMap::new();
    let aliases = BTreeMap::from([(
        PathBuf::new(),
        AliasConfig {
            base: PathBuf::new(),
            bare_imports: false,
            patterns: Pattern::parse("@/*", &["./src/*".to_owned()])
                .into_iter()
                .collect(),
        },
    )]);
    let packages = BTreeMap::new();
    let env = Environment {
        known: &known,
        assets: &BTreeSet::new(),
        crates: &crates,
        aliases: &aliases,
        packages: &packages,
        workspaces: &Workspaces::default(),
        projects: &BTreeMap::new(),
        python: &PythonLayout::default(),
        swift: &SwiftModules::default(),
        vite_roots: &BTreeMap::new(),
        vite_aliases: &BTreeMap::new(),
        test_files: &BTreeSet::new(),
    };
    let got = extract_in(
        "import a from '@/foo';\nimport b from '@/missing';\nimport c from 'other';",
        Language::TypeScript,
        Path::new("src/main.ts"),
        &env,
    )
    .expect("extract succeeds");
    assert_eq!(targets_of(&got), ["src/foo.ts"]);
    assert_eq!(skips(&got), [(3, "other", UndeclaredPackage)]);
}

fn targets_of(imports: &FileImports) -> Vec<String> {
    imports
        .resolved
        .iter()
        .map(|r| r.target.display().to_string())
        .collect()
}

#[test]
fn computed_specifiers_are_skipped_as_dynamic_with_the_call_line() {
    let got = ts(
        "const a = require(name);\nconst b =\n  await import(\n    `./${x}`,\n  );\nconst c = require('./a' + y);\nrequire(...args);",
        &[],
    );
    assert_eq!(
        skips(&got),
        [
            (1, "name", DynamicSpecifier),
            (3, "`./${x}`", DynamicSpecifier),
            (6, "'./a' + y", DynamicSpecifier),
            (7, "...args", DynamicSpecifier),
        ]
    );
    assert_eq!(got.resolved, Vec::new());
}

#[test]
fn literal_arguments_are_resolved_not_skipped() {
    let got = ts("require('./a');\nimport('./a');\nimport(`./a`);", &[]);
    assert_eq!(got.skipped, Vec::new());
    assert_eq!(got.resolved.len(), 3);
}

#[test]
fn python_dynamic_imports_are_skipped_with_their_first_argument() {
    let got = run(
        "import importlib\nx = importlib.import_module(name)\ny = __import__(f'a.{b}')\nimport_module(name)\nobj.__import__(z)\n",
        Language::Python,
        "main.py",
        &BTreeMap::new(),
    );
    assert_eq!(
        skips(&got),
        [
            (2, "name", DynamicSpecifier),
            (3, "f'a.{b}'", DynamicSpecifier)
        ]
    );
    assert_eq!(got.resolved, Vec::new());
}

#[test]
fn python_dynamic_import_reports_the_call_start_line() {
    let got = run(
        "x = (\n    importlib.import_module(\n        name\n    )\n)\n",
        Language::Python,
        "main.py",
        &BTreeMap::new(),
    );
    assert_eq!(skips(&got), [(2, "name", DynamicSpecifier)]);
}

#[test]
fn skipped_references_are_sorted_and_free_of_duplicates() {
    let got = ts(
        "import b from 'zz';\nimport a from 'aa';\nimport c from 'aa';\nimport d from 'aa';",
        &[],
    );
    assert_eq!(
        skips(&got),
        [
            (1, "zz", UndeclaredPackage),
            (2, "aa", UndeclaredPackage),
            (3, "aa", UndeclaredPackage),
            (4, "aa", UndeclaredPackage)
        ]
    );
    let again = ts(
        "import b from 'zz';\nimport a from 'aa';\nimport c from 'aa';\nimport d from 'aa';",
        &[],
    );
    assert_eq!(got, again);
    let same_line = ts("import a from 'aa'; import b from 'aa';", &[]);
    assert_eq!(skips(&same_line), [(1, "aa", UndeclaredPackage)]);
}

fn write_repo(dir: &Path, files: &[(&str, &str)]) -> BTreeMap<PathBuf, Language> {
    let mut map = BTreeMap::new();
    for (path, content) in files {
        let full = dir.join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(&full, content).expect("write");
        match Path::new(path).extension().and_then(|e| e.to_str()) {
            Some("ts") => map.insert(PathBuf::from(path), Language::TypeScript),
            Some("js") => map.insert(PathBuf::from(path), Language::JavaScript),
            _ => None,
        };
    }
    map
}

#[test]
fn collect_reads_every_dependency_table_of_the_nearest_package_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = r#"{
      "name": "app",
      "dependencies": { "a": "1" },
      "devDependencies": { "b": "1" },
      "peerDependencies": { "c": "1" },
      "optionalDependencies": { "d": "1" },
      "scripts": { "e": "x" }
    }"#;
    let files = write_repo(
        dir.path(),
        &[
            ("package.json", manifest),
            (
                "src/main.ts",
                "import 'a'; import 'b'; import 'c'; import 'd'; import 'e'; import 'f';",
            ),
        ],
    );
    let got = collect(dir.path(), &files).expect("collect");
    let main = got.get(Path::new("src/main.ts")).expect("main.ts");
    let names: Vec<&str> = main.skipped.iter().map(|s| s.specifier.as_str()).collect();
    assert_eq!(names, ["e", "f"]);
}

#[test]
fn collect_skips_a_malformed_package_json_silently_so_the_ancestor_applies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = write_repo(
        dir.path(),
        &[
            ("package.json", r#"{ "dependencies": { "react": "1" } }"#),
            ("web/package.json", "{ this is not json"),
            ("web/main.ts", "import 'react'; import 'zod';"),
            (
                "api/package.json",
                r#"{ "dependencies": ["not", "a", "table"] }"#,
            ),
            ("api/main.ts", "import 'react'; import 'zod';"),
        ],
    );
    let got = collect(dir.path(), &files).expect("collect");
    for file in ["web/main.ts", "api/main.ts"] {
        let imports = got.get(Path::new(file)).expect("file");
        let names: Vec<&str> = imports
            .skipped
            .iter()
            .map(|s| s.specifier.as_str())
            .collect();
        assert_eq!(names, ["zod"], "{file}");
    }
}

#[test]
fn collect_without_any_package_json_declares_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = write_repo(
        dir.path(),
        &[("main.ts", "import 'react'; import 'fs'; import './local';")],
    );
    let got = collect(dir.path(), &files).expect("collect");
    let main = got.get(Path::new("main.ts")).expect("main.ts");
    assert_eq!(skips(main), [(1, "react", UndeclaredPackage)]);
}

#[test]
fn collect_tolerates_a_byte_order_mark_in_package_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = write_repo(
        dir.path(),
        &[
            (
                "package.json",
                "\u{feff}{ \"dependencies\": { \"react\": \"1\" } }",
            ),
            ("main.ts", "import 'react';"),
        ],
    );
    let got = collect(dir.path(), &files).expect("collect");
    assert_eq!(
        got.get(Path::new("main.ts")).map(|f| f.skipped.len()),
        Some(0)
    );
}

const REPO: &[&str] = &[
    "src/lib.rs",
    "src/lint/mod.rs",
    "src/lint/rules.rs",
    "src/lint/data.rs",
    "src/shared.rs",
];

fn rs_at(source: &str, file: &str) -> FileImports {
    let known = known(REPO);
    let crates = BTreeMap::new();
    let aliases = BTreeMap::new();
    let packages = BTreeMap::new();
    let env = Environment {
        known: &known,
        assets: &BTreeSet::new(),
        crates: &crates,
        aliases: &aliases,
        packages: &packages,
        workspaces: &Workspaces::default(),
        projects: &BTreeMap::new(),
        python: &PythonLayout::default(),
        swift: &SwiftModules::default(),
        vite_roots: &BTreeMap::new(),
        vite_aliases: &BTreeMap::new(),
        test_files: &BTreeSet::new(),
    };
    extract_in(source, Language::Rust, Path::new(file), &env).expect("extract succeeds")
}

fn rs(source: &str) -> FileImports {
    rs_at(source, "src/lint/rules.rs")
}

/// (line, target, kind, number of symbols, `test_only`) of every resolved import.
fn resolved(imports: &FileImports) -> Vec<(u32, String, ImportKind, usize, bool)> {
    imports
        .resolved
        .iter()
        .map(|r| {
            (
                r.at.line,
                r.target.display().to_string(),
                r.kind,
                r.symbols.len(),
                r.test_only,
            )
        })
        .collect()
}

#[test]
fn an_embed_of_a_known_source_file_is_an_import_of_kind_embed_with_no_symbol() {
    for source in [
        "const S: &str = include_str!(\"data.rs\");",
        "const S: &[u8] = include_bytes!(\"data.rs\");",
        "include!(\"data.rs\");",
        "const S: &str = std::include_str!(\"data.rs\");",
    ] {
        let got = rs(source);
        assert_eq!(
            resolved(&got),
            [(1, "src/lint/data.rs".to_owned(), Embed, 0, false)],
            "{source}"
        );
        assert_eq!(got.skipped, Vec::new(), "{source}");
        assert_eq!(got.unresolved, Vec::new(), "{source}");
    }
}

#[test]
fn an_embed_path_is_relative_to_the_directory_of_the_file() {
    let got = rs("const S: &str = include_str!(\"../shared.rs\");");
    assert_eq!(
        resolved(&got),
        [(1, "src/shared.rs".to_owned(), Embed, 0, false)]
    );
    let got = rs_at("const S: &str = include_str!(\"shared.rs\");", "src/lib.rs");
    assert_eq!(
        resolved(&got),
        [(1, "src/shared.rs".to_owned(), Embed, 0, false)]
    );
    let got = rs("const S: &str = include_str!(\"./data.rs\");");
    assert_eq!(
        resolved(&got),
        [(1, "src/lint/data.rs".to_owned(), Embed, 0, false)]
    );
}

#[test]
fn an_embed_of_a_non_source_file_is_skipped_with_the_path_as_written() {
    let got = rs(
        "const A: &str = include_str!(\"page.html\");\nconst B: &[u8] = include_bytes!(\"../logo.png\");\nconst C: &str = include_str!(\"style.css\");",
    );
    assert_eq!(got.resolved, Vec::new());
    assert_eq!(
        skips(&got),
        [
            (1, "page.html", EmbedNonSource),
            (2, "../logo.png", EmbedNonSource),
            (3, "style.css", EmbedNonSource)
        ]
    );
}

#[test]
fn an_embed_of_a_missing_source_file_or_a_path_leaving_the_repository_is_skipped() {
    let got = rs(
        "const A: &str = include_str!(\"missing.rs\");\nconst B: &str = include_str!(\"../../../outside.rs\");",
    );
    assert_eq!(got.resolved, Vec::new());
    assert_eq!(
        skips(&got),
        [
            (1, "missing.rs", EmbedNonSource),
            (2, "../../../outside.rs", EmbedNonSource)
        ]
    );
}

#[test]
fn an_embed_reports_the_line_the_macro_starts_on() {
    let got = rs("const S: &str =\n    include_str!(\n        \"data.rs\",\n    );");
    assert_eq!(
        resolved(&got),
        [(2, "src/lint/data.rs".to_owned(), Embed, 0, false)]
    );
}

#[test]
fn an_embed_inside_cfg_test_is_test_only() {
    let got = rs("#[cfg(test)]\nmod tests {\n    const S: &str = include_str!(\"data.rs\");\n}");
    assert_eq!(
        resolved(&got),
        [(3, "src/lint/data.rs".to_owned(), Embed, 0, true)]
    );
}

#[test]
fn an_embed_and_a_use_of_the_same_file_stay_separate_entries() {
    let got = rs("use super::data::X;\nconst S: &str = include_str!(\"data.rs\");");
    let kinds: Vec<ImportKind> = got.resolved.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Runtime, Embed]);
}

#[test]
fn other_macros_and_computed_embeds_are_not_embeds() {
    let got = rs(
        "fn f() {\n    println!(\"data.rs\");\n    format!(\"page.html\");\n    include_str!(concat!(\"a\", \"b\"));\n    my_include!(\"data.rs\");\n}",
    );
    assert_eq!(got.resolved, Vec::new());
    assert_eq!(got.skipped, Vec::new());
}
