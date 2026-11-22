//! Import kind (`runtime`, `type-only`) and the `test_only` flag, per language.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;
use crate::model::ImportKind::{Runtime, TypeOnly};
use crate::model::{ImportKind, Language};

/// (line, kind, `test_only`) of every resolved import, in output order.
type Flags = Vec<(u32, ImportKind, bool)>;

fn known(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

fn flags_of(imports: &FileImports) -> Flags {
    imports
        .resolved
        .iter()
        .map(|import| (import.at.line, import.kind, import.test_only))
        .collect()
}

/// `name` in the directory of `file`.
fn sibling(file: &str, name: &str) -> String {
    Path::new(file)
        .with_file_name(name)
        .to_string_lossy()
        .into_owned()
}

fn ts_in(source: &str, file: &str) -> Flags {
    let got = extract(
        source,
        Language::TypeScript,
        Path::new(file),
        &known(&["src/a.ts", "src/b.ts"]),
    )
    .expect("extract succeeds");
    flags_of(&got)
}

/// The flags of `source` written in `file`, where `./b` is a file next to it.
fn ts_beside(source: &str, file: &str) -> Flags {
    let got = extract(
        source,
        Language::TypeScript,
        Path::new(file),
        &known(&[&sibling(file, "b.ts")]),
    )
    .expect("extract succeeds");
    flags_of(&got)
}

fn ts(source: &str) -> Flags {
    ts_in(source, "src/main.ts")
}

#[test]
fn import_type_is_type_only() {
    assert_eq!(ts("import type { A } from './a';"), [(1, TypeOnly, false)]);
    assert_eq!(ts("import type A from './a';"), [(1, TypeOnly, false)]);
    assert_eq!(
        ts("import type * as ns from './a';"),
        [(1, TypeOnly, false)]
    );
}

#[test]
fn export_type_from_is_type_only() {
    assert_eq!(ts("export type { A } from './a';"), [(1, TypeOnly, false)]);
    assert_eq!(ts("export type * from './a';"), [(1, TypeOnly, false)]);
    assert_eq!(
        ts("export type * as ns from './a';"),
        [(1, TypeOnly, false)]
    );
}

#[test]
fn an_import_whose_every_specifier_is_type_is_type_only() {
    assert_eq!(
        ts("import { type A, type B } from './a';"),
        [(1, TypeOnly, false)]
    );
    assert_eq!(
        ts("export { type A, type B } from './a';"),
        [(1, TypeOnly, false)]
    );
    assert_eq!(
        ts("import { type A as X } from './a';"),
        [(1, TypeOnly, false)]
    );
}

#[test]
fn one_value_specifier_makes_the_import_runtime() {
    assert_eq!(
        ts("import { type A, B } from './a';"),
        [(1, Runtime, false)]
    );
    assert_eq!(
        ts("import { A, type B } from './a';"),
        [(1, Runtime, false)]
    );
    assert_eq!(
        ts("export { type A, B } from './a';"),
        [(1, Runtime, false)]
    );
}

#[test]
fn a_default_or_namespace_import_beside_type_specifiers_is_runtime() {
    assert_eq!(
        ts("import D, { type A } from './a';"),
        [(1, Runtime, false)]
    );
    assert_eq!(
        ts("import * as ns from './a'; import { type A } from './b';"),
        [(1, Runtime, false), (1, TypeOnly, false)]
    );
}

#[test]
fn plain_forms_are_runtime() {
    for source in [
        "import { A } from './a';",
        "import A from './a';",
        "import * as ns from './a';",
        "import './a';",
        "import {} from './a';",
        "export { A } from './a';",
        "export * from './a';",
        "export * as ns from './a';",
        "const a = require('./a');",
        "const a = import('./a');",
    ] {
        assert_eq!(ts(source), [(1, Runtime, false)], "{source}");
    }
}

#[test]
fn a_declaration_file_makes_every_import_type_only() {
    for file in ["src/types.d.ts", "src/types.d.mts", "src/types.d.cts"] {
        assert_eq!(
            ts_in("import { A } from './a';\nexport * from './b';", file),
            [(1, TypeOnly, false), (2, TypeOnly, false)],
            "{file}"
        );
    }
    assert_eq!(
        ts_in("import { A } from './a';", "src/types.ts"),
        [(1, Runtime, false)]
    );
    assert_eq!(
        ts_in("import { A } from './a';", "src/d.ts"),
        [(1, Runtime, false)]
    );
}

#[test]
fn type_only_and_runtime_imports_of_one_file_on_one_line_stay_two_entries() {
    let got = ts("import type { A } from './a'; import { b } from './a';");
    assert_eq!(got, [(1, TypeOnly, false), (1, Runtime, false)]);
}

#[test]
fn imports_of_the_same_kind_on_one_line_still_merge() {
    let got = extract(
        "import type { A } from './a'; import type { B } from './a';",
        Language::TypeScript,
        Path::new("src/main.ts"),
        &known(&["src/a.ts"]),
    )
    .expect("extract succeeds");
    assert_eq!(got.resolved.len(), 1);
    assert_eq!(got.resolved.first().map(|r| r.symbols.len()), Some(2));
}

#[test]
fn type_imports_are_type_only_in_tsx_and_plain_javascript_stays_runtime() {
    let got = extract(
        "import type { A } from './a';",
        Language::TypeScript,
        Path::new("src/main.tsx"),
        &known(&["src/a.ts"]),
    )
    .expect("extract succeeds");
    assert_eq!(flags_of(&got), [(1, TypeOnly, false)]);
    let got = extract(
        "import { A } from './a';",
        Language::JavaScript,
        Path::new("src/main.js"),
        &known(&["src/a.js"]),
    )
    .expect("extract succeeds");
    assert_eq!(flags_of(&got), [(1, Runtime, false)]);
}

#[test]
fn typescript_test_files_are_test_only() {
    for (file, expected) in [
        ("src/a.test.ts", true),
        ("src/a.spec.ts", true),
        ("src/a.test.tsx", true),
        ("src/a.b.spec.ts", true),
        ("src/__tests__/a.ts", true),
        ("src/test/a.ts", true),
        ("tests/a.ts", true),
        ("src/deep/tests/more/a.ts", true),
        ("src/a.ts", false),
        ("src/test.ts", false),
        ("src/spec.ts", false),
        ("src/testing/a.ts", false),
        ("src/latest/a.ts", false),
        ("src/a.contest.ts", false),
    ] {
        let got = ts_beside("import './b';", file);
        assert_eq!(got.first().map(|f| f.2), Some(expected), "{file}");
    }
}

#[test]
fn javascript_test_files_are_test_only() {
    for (file, expected) in [
        ("src/a.test.js", true),
        ("src/a.spec.mjs", true),
        ("src/__tests__/a.js", true),
        ("src/a.js", false),
    ] {
        let got = extract(
            "require('./b');",
            Language::JavaScript,
            Path::new(file),
            &known(&[&sibling(file, "b.js")]),
        )
        .expect("extract succeeds");
        assert_eq!(
            got.resolved.first().map(|r| r.test_only),
            Some(expected),
            "{file}"
        );
    }
}

#[test]
fn a_test_file_can_hold_type_only_imports() {
    assert_eq!(
        ts_in("import type { A } from './a';", "src/a.test.ts"),
        [(1, TypeOnly, true)]
    );
}

fn py_in(source: &str, file: &str) -> Flags {
    let got = extract(
        source,
        Language::Python,
        Path::new(file),
        &known(&["a.py", "b.py", "c.py", "d.py"]),
    )
    .expect("extract succeeds");
    flags_of(&got)
}

fn py(source: &str) -> Flags {
    py_in(source, "main.py")
}

#[test]
fn python_imports_in_a_type_checking_block_are_type_only() {
    assert_eq!(
        py("if TYPE_CHECKING:\n    import a\n    from b import x\n"),
        [(2, TypeOnly, false), (3, TypeOnly, false)]
    );
    assert_eq!(
        py("if typing.TYPE_CHECKING:\n    import a\n"),
        [(2, TypeOnly, false)]
    );
}

#[test]
fn python_type_checking_applies_at_any_depth_below_the_block() {
    let source = "if TYPE_CHECKING:\n    try:\n        import a\n    except ImportError:\n        import b\n    def f():\n        import c\n";
    assert_eq!(
        py(source),
        [
            (3, TypeOnly, false),
            (5, TypeOnly, false),
            (7, TypeOnly, false)
        ]
    );
}

#[test]
fn python_imports_outside_the_type_checking_body_are_runtime() {
    let source =
        "import a\nif TYPE_CHECKING:\n    import b\nelse:\n    import c\nif other:\n    import d\n";
    assert_eq!(
        py(source),
        [
            (1, Runtime, false),
            (3, TypeOnly, false),
            (5, Runtime, false),
            (7, Runtime, false)
        ]
    );
    assert_eq!(
        py("if TYPE_CHECKING:\n    pass\nelif x:\n    import a\n"),
        [(4, Runtime, false)]
    );
    assert_eq!(
        py("if not TYPE_CHECKING:\n    import a\n"),
        [(2, Runtime, false)]
    );
}

#[test]
fn python_test_files_are_test_only() {
    for (file, expected) in [
        ("test_main.py", true),
        ("main_test.py", true),
        ("conftest.py", true),
        ("pkg/conftest.py", true),
        ("tests/main.py", true),
        ("pkg/test/main.py", true),
        ("pkg/tests/deep/main.py", true),
        ("main.py", false),
        ("tester.py", false),
        ("pkg/testing/main.py", false),
        ("contest.py", false),
        ("test.py", false),
    ] {
        let got = py_in("import a", file);
        assert_eq!(got.first().map(|f| f.2), Some(expected), "{file}");
    }
}

const REPO: &[&str] = &[
    "src/lib.rs",
    "src/model/mod.rs",
    "src/diagram/mod.rs",
    "src/lint/mod.rs",
    "src/lint/rules.rs",
];

fn rs_with(source: &str, file: &str, repo: &[&str]) -> FileImports {
    let known = known(repo);
    let library: BTreeMap<String, PathBuf> =
        BTreeMap::from([("driftwall".to_owned(), PathBuf::from("src"))]);
    let aliases = BTreeMap::new();
    let env = Environment {
        known: &known,
        assets: &BTreeSet::new(),
        crates: &library,
        aliases: &aliases,
        packages: &BTreeMap::new(),
        workspaces: &Workspaces::default(),
        projects: &BTreeMap::new(),
    };
    extract_in(source, Language::Rust, Path::new(file), &env).expect("extract succeeds")
}

fn rs(source: &str) -> Flags {
    flags_of(&rs_with(source, "src/lint/rules.rs", REPO))
}

#[test]
fn rust_imports_are_runtime_and_not_test_only_by_default() {
    assert_eq!(rs("use crate::model::A;"), [(1, Runtime, false)]);
}

#[test]
fn rust_imports_in_a_cfg_test_module_are_test_only() {
    let source = "use crate::model::A;\n#[cfg(test)]\nmod tests {\n    use crate::diagram::B;\n}\n";
    assert_eq!(rs(source), [(1, Runtime, false), (4, Runtime, true)]);
}

#[test]
fn rust_cfg_test_applies_at_any_nesting_depth() {
    let source = "#[cfg(test)]\nmod outer {\n    mod inner {\n        fn f() {\n            use crate::model::A;\n            crate::diagram::go();\n        }\n    }\n}\nfn after() { crate::model::B::new(); }\n";
    assert_eq!(
        rs(source),
        [(5, Runtime, true), (6, Runtime, true), (10, Runtime, false)]
    );
}

#[test]
fn rust_cfg_test_on_a_use_function_or_other_item_marks_what_is_inside_it() {
    assert_eq!(
        rs("#[cfg(test)]\nuse crate::model::A;\nuse crate::diagram::B;"),
        [(2, Runtime, true), (3, Runtime, false)]
    );
    assert_eq!(
        rs(
            "#[cfg(test)]\nfn helper() { crate::model::go(); }\nfn real() { crate::diagram::go(); }"
        ),
        [(2, Runtime, true), (3, Runtime, false)]
    );
    assert_eq!(
        rs("#[cfg(test)]\nimpl A {\n    fn f() { use crate::model::X; }\n}"),
        [(3, Runtime, true)]
    );
}

#[test]
fn rust_cfg_test_is_found_among_other_attributes_and_comments() {
    let source = "#[cfg(test)]\n#[allow(unused)]\n// note\nmod tests {\n    use crate::model::A;\n}\n#[derive(Debug)]\n#[cfg( test )]\nmod more {\n    use crate::diagram::B;\n}\n";
    assert_eq!(rs(source), [(5, Runtime, true), (10, Runtime, true)]);
}

#[test]
fn rust_inner_cfg_test_marks_the_module_it_opens() {
    let source =
        "mod tests {\n    #![cfg(test)]\n    use crate::model::A;\n}\nuse crate::diagram::B;\n";
    assert_eq!(rs(source), [(3, Runtime, true), (5, Runtime, false)]);
    assert_eq!(
        rs("#![cfg(test)]\nuse crate::model::A;"),
        [(2, Runtime, true)]
    );
}

#[test]
fn only_the_exact_cfg_test_predicate_counts() {
    for attribute in [
        "#[cfg(not(test))]",
        "#[cfg(any(test, debug_assertions))]",
        "#[cfg(feature = \"test\")]",
        "#[test]",
        "#[cfg(testing)]",
    ] {
        let source = format!("{attribute}\nmod m {{\n    use crate::model::A;\n}}\n");
        assert_eq!(rs(&source), [(3, Runtime, false)], "{attribute}");
    }
}

#[test]
fn a_cfg_test_attribute_on_a_sibling_does_not_leak_to_the_next_item() {
    let source = "#[cfg(test)]\nmod tests {}\nmod real {\n    use crate::model::A;\n}\n";
    assert_eq!(rs(source), [(4, Runtime, false)]);
}

#[test]
fn rust_files_named_as_tests_or_under_a_tests_directory_are_test_only() {
    for (file, expected) in [
        ("tests/it.rs", true),
        ("crates/a/tests/it.rs", true),
        ("tests/common/mod.rs", true),
        ("src/lint/rules_test.rs", true),
        ("src/lint/rules_tests.rs", true),
        ("src/lint/rules.rs", false),
        ("src/lint/tests.rs", false),
        ("src/lint/test.rs", false),
        ("src/latest/rules.rs", false),
        ("src/lint/contests.rs", false),
    ] {
        let got = rs_with(
            "use driftwall::model::A;",
            file,
            &["src/lib.rs", "src/model/mod.rs"],
        );
        assert_eq!(
            got.resolved.first().map(|r| r.test_only),
            Some(expected),
            "{file}"
        );
    }
}
