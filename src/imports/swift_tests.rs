//! Swift: imports of the modules a `Package.swift` declares, resolved to their entry files.

use super::fixtures::{Repo, repo};
use super::*;
use crate::model::{ImportKind, Symbol};

/// A package with a library `AppCore`, two executables that depend on it and a test target.
const MANIFEST: &str = r#"// swift-tools-version:5.9
import PackageDescription
let package = Package(
    name: "PM",
    targets: [
        .target(name: "AppCore"),
        .executableTarget(name: "DeskApp", dependencies: ["AppCore"]),
        .executableTarget(name: "app-cli", dependencies: ["AppCore"], path: "Tools/cli"),
        .testTarget(name: "AppCoreTests", dependencies: ["AppCore"]),
    ]
)
"#;

/// (line, target, symbols, kind, `test_only`) of every resolved import of `file`.
fn found(r: &Repo, file: &str) -> Vec<(u32, String, Vec<String>, ImportKind, bool)> {
    r.imports(file)
        .resolved
        .iter()
        .map(|i| {
            (
                i.at.line,
                i.target.display().to_string(),
                i.symbols.iter().map(ToString::to_string).collect(),
                i.kind,
                i.test_only,
            )
        })
        .collect()
}

fn targets(r: &Repo, file: &str) -> Vec<(u32, String)> {
    r.edges(file)
}

#[test]
fn a_plain_import_of_a_local_target_resolves_to_its_entry_file() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", "public struct Frame {}\n"),
        ("Sources/AppCore/Other.swift", ""),
        (
            "Sources/DeskApp/main.swift",
            "import Foundation\nimport AppCore\n",
        ),
    ]);
    assert_eq!(
        found(&r, "Sources/DeskApp/main.swift"),
        [(
            2,
            "Sources/AppCore/AppCore.swift".to_owned(),
            vec![],
            ImportKind::Runtime,
            false
        )]
    );
}

#[test]
fn testable_and_exported_imports_are_runtime_imports_at_the_line_of_the_declaration() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", ""),
        (
            "Sources/DeskApp/App.swift",
            "// header\n\n@testable import AppCore\n@_exported\nimport AppCore\n",
        ),
    ]);
    assert_eq!(
        found(&r, "Sources/DeskApp/App.swift"),
        [
            (
                3,
                "Sources/AppCore/AppCore.swift".to_owned(),
                vec![],
                ImportKind::Runtime,
                false
            ),
            (
                4,
                "Sources/AppCore/AppCore.swift".to_owned(),
                vec![],
                ImportKind::Runtime,
                false
            ),
        ]
    );
}

#[test]
fn a_kind_qualified_import_uses_the_last_segment() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", ""),
        (
            "Sources/DeskApp/App.swift",
            "import struct AppCore.Frame\nimport func AppCore.send\nimport typealias AppCore.Alias\nimport AppCore.Sub\nimport class AppCore\n",
        ),
    ]);
    let got: Vec<(u32, Vec<String>)> = found(&r, "Sources/DeskApp/App.swift")
        .into_iter()
        .map(|(line, _, symbols, _, _)| (line, symbols))
        .collect();
    assert_eq!(
        got,
        [
            (1, vec!["Frame".to_owned()]),
            (2, vec!["send".to_owned()]),
            (3, vec!["Alias".to_owned()]),
            (4, vec![]),
            (5, vec![]),
        ]
    );
    let first = &r.imports("Sources/DeskApp/App.swift").resolved[0];
    assert_eq!(
        first.symbols.iter().collect::<Vec<_>>(),
        [&Symbol::Name("Frame".to_owned())]
    );
}

#[test]
fn modules_that_no_target_declares_are_ignored() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", ""),
        (
            "Sources/DeskApp/App.swift",
            "import Foundation\nimport SwiftUI\nimport PackageDescription\nimport ArgumentParser\n",
        ),
    ]);
    let got = r.imports("Sources/DeskApp/App.swift");
    assert_eq!(got, FileImports::default());
}

#[test]
fn imports_in_comments_and_strings_are_not_imports() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", ""),
        (
            "Sources/DeskApp/App.swift",
            "// import AppCore\n/* import AppCore */\nlet s = \"\"\"\nimport AppCore\n\"\"\"\n#if canImport(AppKit)\nimport AppCore\n#endif\n",
        ),
    ]);
    assert_eq!(targets(&r, "Sources/DeskApp/App.swift").len(), 1);
    assert_eq!(targets(&r, "Sources/DeskApp/App.swift")[0].0, 7);
}

#[test]
fn the_entry_file_is_named_after_the_target_when_it_exists() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/A.swift", ""),
        ("Sources/AppCore/AppCore.swift", ""),
        ("Sources/AppCore/Deep/AppCore.swift", ""),
        ("Sources/DeskApp/main.swift", "import AppCore\n"),
    ]);
    assert_eq!(
        targets(&r, "Sources/DeskApp/main.swift"),
        [(1, "Sources/AppCore/AppCore.swift".to_owned())]
    );
}

#[test]
fn without_a_file_of_the_target_name_the_first_swift_file_in_path_order_is_the_entry() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/b.swift", ""),
        ("Sources/AppCore/Zed/a.swift", ""),
        ("Sources/AppCore/Sub/z.swift", ""),
        ("Sources/AppCore/notes.txt", ""),
        ("Sources/AppCoreExtras/A.swift", ""),
        ("Sources/DeskApp/main.swift", "import AppCore\n"),
    ]);
    assert_eq!(
        targets(&r, "Sources/DeskApp/main.swift"),
        [(1, "Sources/AppCore/Sub/z.swift".to_owned())]
    );
}

#[test]
fn a_target_with_no_swift_file_is_ignored() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/data.json", "{}"),
        ("Sources/DeskApp/main.swift", "import AppCore\n"),
    ]);
    assert_eq!(
        r.imports("Sources/DeskApp/main.swift"),
        FileImports::default()
    );
}

#[test]
fn an_explicit_path_and_a_module_name_with_a_dash_are_followed() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", ""),
        ("Tools/cli/main.swift", "import AppCore\n"),
        ("Sources/DeskApp/main.swift", "import app_cli\n"),
    ]);
    assert_eq!(
        targets(&r, "Sources/DeskApp/main.swift"),
        [(1, "Tools/cli/main.swift".to_owned())]
    );
    assert_eq!(
        targets(&r, "Tools/cli/main.swift"),
        [(1, "Sources/AppCore/AppCore.swift".to_owned())]
    );
}

#[test]
fn an_import_in_a_test_target_carries_the_test_flag() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", ""),
        (
            "Tests/AppCoreTests/AppCoreTests.swift",
            "@testable import AppCore\n",
        ),
    ]);
    assert_eq!(
        found(&r, "Tests/AppCoreTests/AppCoreTests.swift"),
        [(
            1,
            "Sources/AppCore/AppCore.swift".to_owned(),
            vec![],
            ImportKind::Runtime,
            true
        )]
    );
}

#[test]
fn manifests_below_the_root_resolve_their_paths_from_their_own_directory() {
    let r = repo(&[
        ("app/Package.swift", MANIFEST),
        ("app/Sources/AppCore/AppCore.swift", ""),
        ("app/Sources/DeskApp/main.swift", "import AppCore\n"),
        ("other/Sources/DeskApp/main.swift", "import AppCore\n"),
    ]);
    assert_eq!(
        targets(&r, "app/Sources/DeskApp/main.swift"),
        [(1, "app/Sources/AppCore/AppCore.swift".to_owned())]
    );
    assert_eq!(
        targets(&r, "other/Sources/DeskApp/main.swift"),
        [(1, "app/Sources/AppCore/AppCore.swift".to_owned())],
        "a module declared only by another package is still a repository module"
    );
}

#[test]
fn a_module_declared_by_several_packages_resolves_to_the_nearest() {
    let manifest = r#"let p = Package(name: "x", targets: [.target(name: "Shared")])"#;
    let r = repo(&[
        ("a/Package.swift", manifest),
        ("a/Sources/Shared/Shared.swift", ""),
        ("a/Sources/App/main.swift", "import Shared\n"),
        ("a/nested/Package.swift", manifest),
        ("a/nested/Sources/Shared/Shared.swift", ""),
        ("a/nested/Sources/App/main.swift", "import Shared\n"),
        ("b/Package.swift", manifest),
        ("b/Sources/Shared/Shared.swift", ""),
        ("c/main.swift", "import Shared\n"),
        ("a/loose.swift", "import Shared\n"),
    ]);
    let edge = |file: &str| targets(&r, file)[0].1.clone();
    assert_eq!(
        edge("a/Sources/App/main.swift"),
        "a/Sources/Shared/Shared.swift"
    );
    assert_eq!(
        edge("a/nested/Sources/App/main.swift"),
        "a/nested/Sources/Shared/Shared.swift"
    );
    assert_eq!(edge("a/loose.swift"), "a/Sources/Shared/Shared.swift");
    assert_eq!(
        edge("c/main.swift"),
        "a/Sources/Shared/Shared.swift",
        "no package contains the importer: the first in path order wins the tie"
    );
}

#[test]
fn a_package_that_contains_the_importer_beats_a_nearer_sibling() {
    let manifest = r#"let p = Package(name: "x", targets: [.target(name: "Shared")])"#;
    let r = repo(&[
        ("app/Package.swift", manifest),
        ("app/Sources/Shared/Shared.swift", ""),
        ("app/Sources/App/main.swift", "import Shared\n"),
        ("app/sub/Package.swift", manifest),
        ("app/sub/Sources/Shared/Shared.swift", ""),
    ]);
    assert_eq!(
        targets(&r, "app/Sources/App/main.swift"),
        [(1, "app/Sources/Shared/Shared.swift".to_owned())]
    );
}

#[test]
fn the_manifest_itself_is_a_swift_file_that_imports_nothing_local() {
    let r = repo(&[
        ("Package.swift", MANIFEST),
        ("Sources/AppCore/AppCore.swift", ""),
    ]);
    assert_eq!(r.imports("Package.swift"), FileImports::default());
}
