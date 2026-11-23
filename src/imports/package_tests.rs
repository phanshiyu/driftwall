//! Package uses: imports of code outside the repository, per language, see "Package uses" in
//! `docs/DESIGN.md`.

use super::fixtures::repo;
use super::*;
use crate::model::SkipReason;

/// (line, name, kind, `test_only`) of every package use of `file`.
fn uses(imports: &FileImports) -> Vec<(u32, &str, &'static str, bool)> {
    imports
        .packages
        .iter()
        .map(|p| (p.at.line, p.name.as_str(), p.kind.as_str(), p.test_only))
        .collect()
}

const MANIFEST: (&str, &str) = (
    "package.json",
    r#"{"dependencies": {"lit": "1", "@lit/reactive-element": "1", "three": "1"}}"#,
);

#[test]
fn node_builtins_are_stdlib_named_node_module_whatever_the_spelling() {
    let r = repo(&[(
        "a.ts",
        "import a from 'fs';\nimport b from 'node:path';\nimport c from 'fs/promises';\nimport d from 'node:test';\nimport e from 'node:fs/promises';\nimport f from 'child_process';",
    )]);
    assert_eq!(
        uses(&r.imports("a.ts")),
        [
            (1, "node:fs", "stdlib", false),
            (2, "node:path", "stdlib", false),
            (3, "node:fs", "stdlib", false),
            (4, "node:test", "stdlib", false),
            (5, "node:fs", "stdlib", false),
            (6, "node:child_process", "stdlib", false),
        ]
    );
}

#[test]
fn a_bare_test_is_a_third_party_package_and_node_test_is_not() {
    let r = repo(&[("a.ts", "import a from 'test';")]);
    assert_eq!(
        uses(&r.imports("a.ts")),
        [(1, "test", "third-party", false)]
    );
}

#[test]
fn declared_packages_are_third_party_named_by_scope_and_first_segment() {
    let r = repo(&[
        MANIFEST,
        (
            "a.ts",
            "import { x } from 'lit';\nimport y from '@lit/reactive-element/decorators.js';\nimport z from 'three/examples/jsm/x.js';",
        ),
    ]);
    let got = r.imports("a.ts");
    assert_eq!(
        uses(&got),
        [
            (1, "lit", "third-party", false),
            (2, "@lit/reactive-element", "third-party", false),
            (3, "three", "third-party", false),
        ]
    );
    assert_eq!(got.skipped, []);
}

#[test]
fn an_undeclared_package_is_a_third_party_use_and_stays_a_skipped_reference() {
    let r = repo(&[("a.ts", "import a from 'left-pad';")]);
    let got = r.imports("a.ts");
    assert_eq!(uses(&got), [(1, "left-pad", "third-party", false)]);
    assert_eq!(got.skipped.len(), 1);
    assert_eq!(got.skipped[0].reason, SkipReason::UndeclaredPackage);
    assert_eq!(got.skipped[0].specifier, "left-pad");
}

#[test]
fn what_is_not_a_package_is_no_use() {
    let r = repo(&[
        (
            "package.json",
            r##"{"workspaces": ["packages/*"], "imports": {"#x": "./x.ts"}}"##,
        ),
        (
            "packages/b/package.json",
            r#"{"name": "@w/b", "main": "i.ts"}"#,
        ),
        ("packages/b/i.ts", ""),
        ("x.ts", ""),
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@app/*": ["src/*"]}}}"#,
        ),
        ("src/y.ts", ""),
        (
            "a.ts",
            "import './x';\nimport '@w/b';\nimport '@app/y';\nimport '@app/missing';\nimport 'virtual:pwa';\nimport 'https://cdn.example/x.js';\nimport '#x';\nimport '#nope';\nimport './x.ts';",
        ),
    ]);
    assert_eq!(uses(&r.imports("a.ts")), []);
}

#[test]
fn a_type_import_and_a_test_file_are_uses_with_their_flags() {
    let r = repo(&[
        MANIFEST,
        ("a.ts", "import type { T } from 'lit';"),
        ("a.test.ts", "import { x } from 'three';\nimport 'node:fs';"),
    ]);
    assert_eq!(uses(&r.imports("a.ts")), [(1, "lit", "third-party", false)]);
    assert_eq!(
        uses(&r.imports("a.test.ts")),
        [
            (1, "three", "third-party", true),
            (2, "node:fs", "stdlib", true)
        ]
    );
}

#[test]
fn a_package_imported_twice_on_a_line_is_one_use_and_on_two_lines_two() {
    let r = repo(&[
        MANIFEST,
        (
            "a.ts",
            "import a from 'lit'; import b from 'lit/x.js';\nimport c from 'lit';",
        ),
    ]);
    assert_eq!(
        uses(&r.imports("a.ts")),
        [
            (1, "lit", "third-party", false),
            (2, "lit", "third-party", false)
        ]
    );
}

#[test]
fn require_and_dynamic_import_with_a_literal_are_uses() {
    let r = repo(&[(
        "a.js",
        "const a = require('left-pad');\nimport('node:os');\n",
    )]);
    assert_eq!(
        uses(&r.imports("a.js")),
        [
            (1, "left-pad", "third-party", false),
            (2, "node:os", "stdlib", false)
        ]
    );
}

#[test]
fn svelte_scripts_record_their_packages_like_typescript() {
    let r = repo(&[
        MANIFEST,
        (
            "A.svelte",
            "<script lang=\"ts\">\n  import { html } from 'lit';\n</script>\n",
        ),
    ]);
    assert_eq!(
        uses(&r.imports("A.svelte")),
        [(2, "lit", "third-party", false)]
    );
}

#[test]
fn python_modules_no_root_resolves_are_stdlib_or_third_party_by_the_pinned_list() {
    let r = repo(&[(
        "a.py",
        "import os.path\nimport yaml.constructor\nfrom collections import OrderedDict\nfrom __future__ import annotations\nimport distutils\nimport importlib\nimportlib.import_module('numpy.linalg')\n",
    )]);
    assert_eq!(
        uses(&r.imports("a.py")),
        [
            (1, "os", "stdlib", false),
            (2, "yaml", "third-party", false),
            (3, "collections", "stdlib", false),
            (5, "distutils", "third-party", false),
            (6, "importlib", "stdlib", false),
            (7, "numpy", "third-party", false),
        ]
    );
}

#[test]
fn python_first_party_modules_are_never_packages() {
    let r = repo(&[
        ("pkg/__init__.py", ""),
        ("pkg/a.py", ""),
        ("other/tool.py", ""),
        (
            "main.py",
            "import pkg.a\nimport pkg.typo\nfrom . import x\nimport tool\n",
        ),
    ]);
    let got = r.imports("main.py");
    assert_eq!(uses(&got), []);
    assert_eq!(got.skipped.len(), 1);
    assert_eq!(got.skipped[0].reason, SkipReason::UnreachableModule);
}

#[test]
fn a_python_namespace_directory_miss_is_a_third_party_use_and_a_skipped_reference() {
    let r = repo(&[
        ("google/protos/x.py", ""),
        ("main.py", "from google.cloud import storage\n"),
    ]);
    let got = r.imports("main.py");
    assert_eq!(uses(&got), [(1, "google", "third-party", false)]);
    assert_eq!(got.skipped.len(), 1);
    assert_eq!(got.skipped[0].reason, SkipReason::UndeclaredPackage);
}

#[test]
fn python_test_files_record_test_only_uses() {
    let r = repo(&[
        ("test_a.py", "import pytest\n"),
        ("a.py", "import pytest\n"),
    ]);
    assert_eq!(
        uses(&r.imports("test_a.py")),
        [(1, "pytest", "third-party", true)]
    );
    assert_eq!(
        uses(&r.imports("a.py")),
        [(1, "pytest", "third-party", false)]
    );
}

#[test]
fn rust_uses_are_std_crates_and_the_dependencies_the_manifest_lists() {
    let r = repo(&[
        (
            "Cargo.toml",
            "[package]\nname = \"app\"\n[dependencies]\nserde-json = \"1\"\nrayon = \"1\"\n[dev-dependencies]\ntempfile = \"3\"\n",
        ),
        (
            "src/main.rs",
            "use std::fs;\nuse serde_json::Value;\nuse rayon::prelude::*;\nuse crate::x;\nfn f() { let _ = core::mem::size_of::<u8>(); let _ = Local::new(); let _ = alloc::vec::Vec::<u8>::new(); }\n#[cfg(test)]\nmod tests {\n    use tempfile::tempdir;\n}\n",
        ),
    ]);
    assert_eq!(
        uses(&r.imports("src/main.rs")),
        [
            (1, "std", "stdlib", false),
            (2, "serde_json", "third-party", false),
            (3, "rayon", "third-party", false),
            (5, "alloc", "stdlib", false),
            (5, "core", "stdlib", false),
            (8, "tempfile", "third-party", true),
        ]
    );
}

#[test]
fn a_rust_name_no_manifest_lists_and_a_repository_crate_are_no_package() {
    let r = repo(&[
        ("Cargo.toml", "[package]\nname = \"app\"\n"),
        (
            "src/main.rs",
            "use serde::Serialize;\nuse mylib::x;\nfn f() { let _ = toml::from_str(\"\"); }\n",
        ),
        ("mylib/Cargo.toml", "[package]\nname = \"mylib\"\n"),
        ("mylib/lib.rs", ""),
    ]);
    assert_eq!(uses(&r.imports("src/main.rs")), []);
}

#[test]
fn the_nearest_manifest_above_a_rust_file_governs_it() {
    let r = repo(&[
        ("Cargo.toml", "[dependencies]\nrayon = \"1\"\n"),
        ("crates/a/Cargo.toml", "[dependencies]\nserde = \"1\"\n"),
        ("crates/a/src/lib.rs", "use serde::S;\nuse rayon::T;\n"),
    ]);
    assert_eq!(
        uses(&r.imports("crates/a/src/lib.rs")),
        [(1, "serde", "third-party", false)]
    );
}

#[test]
fn swift_modules_no_target_declares_are_system_frameworks_or_third_party() {
    let r = repo(&[
        (
            "Package.swift",
            "import PackageDescription\nlet package = Package(name: \"P\", targets: [.target(name: \"Core\")])\n",
        ),
        (
            "Sources/Core/Core.swift",
            "import Foundation\nimport Alamofire\n@testable import XCTest\nimport struct SwiftUI.Color\n",
        ),
        ("Sources/App/main.swift", "import Core\n"),
    ]);
    assert_eq!(
        uses(&r.imports("Sources/Core/Core.swift")),
        [
            (1, "Foundation", "stdlib", false),
            (2, "Alamofire", "third-party", false),
            (3, "XCTest", "stdlib", false),
            (4, "SwiftUI", "stdlib", false),
        ]
    );
    assert_eq!(uses(&r.imports("Sources/App/main.swift")), []);
    assert_eq!(
        uses(&r.imports("Package.swift")),
        [(1, "PackageDescription", "stdlib", false)]
    );
}

#[test]
fn shell_has_no_package_uses() {
    let r = repo(&[(
        "a.sh",
        "source /etc/profile\n. ~/x.sh\nsource \"$HOME/y.sh\"\n",
    )]);
    assert_eq!(uses(&r.imports("a.sh")), []);
}

#[test]
fn package_uses_are_serialized_flat() {
    let r = repo(&[("a.ts", "import a from 'node:fs';")]);
    let json = serde_json::to_string(&r.imports("a.ts").packages).unwrap();
    assert_eq!(
        json,
        r#"[{"file":"a.ts","line":1,"name":"node:fs","kind":"stdlib","test_only":false}]"#
    );
}
