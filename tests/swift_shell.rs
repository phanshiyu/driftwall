//! End to end: a repository modelled on a macOS app with Swift targets, deploy scripts that source
//! libraries of another directory, and extensionless scripts. The dependencies that only these
//! languages express must show up as edges, and `init` must baseline them.

use driftwall::lint::{self, LintOptions};
use driftwall::model::ViolationKind;

const PACKAGE: &str = "// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: \"PM\",
    products: [.library(name: \"AppCore\", targets: [\"AppCore\"])],
    targets: [
        .target(name: \"AppCore\"),
        .executableTarget(name: \"DeskApp\", dependencies: [\"AppCore\"]),
        .executableTarget(name: \"app-cli\", dependencies: [\"AppCore\"]),
        .executableTarget(name: \"app-perf\", dependencies: [\"AppCore\"]),
        .testTarget(name: \"AppCoreTests\", dependencies: [\"AppCore\"]),
    ]
)
";

const DEPLOY: &str = "#!/bin/bash
ROOT=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"
. \"$ROOT/device/lib/net.sh\"
. \"$ROOT/device/lib/disk.sh\"
echo deploying
";

/// The files of the repository, without any `.arch` or contract.
fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Package.swift", PACKAGE),
        (
            "Sources/AppCore/AppCore.swift",
            "import Foundation\npublic enum AppCore {}\n",
        ),
        ("Sources/AppCore/Frame.swift", "public struct Frame {}\n"),
        ("Sources/AppCore/Net/Link.swift", "public struct Link {}\n"),
        (
            "Sources/DeskApp/main.swift",
            "import SwiftUI\nimport AppCore\nimport struct AppCore.Frame\n",
        ),
        ("Sources/DeskApp/Views/Panel.swift", "import AppCore\n"),
        ("Sources/app-cli/main.swift", "import AppCore\n"),
        ("Sources/app-perf/main.swift", "@testable import AppCore\n"),
        (
            "Tests/AppCoreTests/FrameTests.swift",
            "import XCTest\n@testable import AppCore\n",
        ),
        ("scripts/deploy.sh", DEPLOY),
        ("scripts/release.sh", DEPLOY),
        ("device/lib/net.sh", "#!/bin/sh\nnet_up() { :; }\n"),
        ("device/lib/disk.sh", "#!/bin/sh\ndisk_up() { :; }\n"),
        (
            "bin/provision",
            "#!/usr/bin/env bash\n. \"${BASH_SOURCE%/*}/../device/lib/net.sh\"\n",
        ),
        (
            "tools/gen",
            "#!/usr/bin/env python3\nimport libpy.helper\nprint(libpy.helper.x)\n",
        ),
        ("libpy/helper.py", "x = 1\n"),
        ("docs/NOTES", "plain text, no shebang\n"),
        ("docs/runner", "#!/usr/bin/env node\nconsole.log(1);\n"),
    ]
}

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use driftwall::lint::{self, LintOptions};
    use tempfile::TempDir;

    pub fn repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    pub fn run_on(subcommand: &str, root: &Path, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .arg(subcommand)
            .arg(root)
            .args(extra)
            .output()
            .unwrap()
    }

    pub fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    pub fn read(root: &Path, path: &str) -> String {
        fs::read_to_string(root.join(path)).unwrap()
    }

    /// Every observed edge as `from -> to`.
    pub fn edges(root: &Path) -> Vec<String> {
        let observation = lint::observe(root, &LintOptions::default()).unwrap();
        observation
            .observed
            .keys()
            .map(|edge| format!("{} -> {}", edge.from, edge.to))
            .collect()
    }
}

use support::{edges, read, repo, run_on, text};

#[test]
fn init_makes_one_node_per_swift_target_and_lints_clean() {
    let dir = repo(&files());
    let init = run_on("init", dir.path(), &[]);
    assert_eq!(init.status.code(), Some(0), "{}", text(&init.stderr));

    let contract = driftwall::diagram::parse(&read(dir.path(), "architecture.mmd")).unwrap();
    let nodes: Vec<String> = contract.nodes.keys().map(ToString::to_string).collect();
    for expected in [
        "AppCore",
        "DeskApp",
        "app-cli",
        "app-perf",
        "AppCoreTests",
    ] {
        assert!(
            nodes.contains(&expected.to_owned()),
            "{expected} in {nodes:?}"
        );
    }
    let edge_pairs: Vec<String> = contract
        .edges
        .keys()
        .map(|edge| format!("{} {}", edge.from, edge.to))
        .collect();
    for expected in [
        "DeskApp AppCore",
        "app-cli AppCore",
        "app-perf AppCore",
        "AppCoreTests AppCore",
    ] {
        assert!(
            edge_pairs.contains(&expected.to_owned()),
            "{expected} in {edge_pairs:?}"
        );
    }
    assert_eq!(
        read(dir.path(), "Sources/AppCore/.arch"),
        "node: AppCore\n",
        "a Swift target is a plain node, never node: *"
    );
    assert_eq!(read(dir.path(), "Sources/app-cli/.arch"), "node: app-cli\n");
    assert!(
        !dir.path().join("Sources/AppCore/Net/.arch").exists(),
        "no tag below a target"
    );

    let lint = run_on("lint", dir.path(), &["--strict"]);
    assert_eq!(
        (lint.status.code(), text(&lint.stdout)),
        (Some(0), "driftwall: clean\n".to_owned()),
        "{}",
        read(dir.path(), "architecture.mmd")
    );
}

#[test]
fn a_swift_import_of_another_target_is_an_edge_of_the_target_nodes() {
    let dir = repo(&files());
    let init = run_on("init", dir.path(), &[]);
    assert_eq!(init.status.code(), Some(0), "{}", text(&init.stderr));
    let all = edges(dir.path());
    for expected in [
        "AppCoreTests -> AppCore",
        "DeskApp -> AppCore",
        "app-cli -> AppCore",
        "app-perf -> AppCore",
    ] {
        assert!(all.contains(&expected.to_owned()), "{expected} in {all:?}");
    }
}

#[test]
fn sourcing_a_library_of_another_directory_is_a_scripts_to_device_edge() {
    let dir = repo(&files());
    let init = run_on("init", dir.path(), &[]);
    assert_eq!(init.status.code(), Some(0), "{}", text(&init.stderr));
    let contract = driftwall::diagram::parse(&read(dir.path(), "architecture.mmd")).unwrap();
    let declared: Vec<String> = contract
        .edges
        .keys()
        .map(|edge| format!("{} -> {}", edge.from, edge.to))
        .collect();
    assert!(
        declared.contains(&"scripts -> device".to_owned()),
        "{declared:?}"
    );
    assert!(edges(dir.path()).contains(&"scripts -> device".to_owned()));
}

#[test]
fn without_the_edge_in_the_contract_each_sourced_file_is_an_undeclared_edge_at_its_line() {
    let dir = repo(&files());
    let tags = [
        ("scripts/.arch", "node: scripts\n"),
        ("device/.arch", "node: device\n"),
    ];
    for (path, contents) in tags {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    std::fs::write(
        dir.path().join("architecture.mmd"),
        "flowchart LR\n  scripts[\"scripts\"]\n  device[\"device\"]\n",
    )
    .unwrap();
    let report = lint::run(dir.path(), &LintOptions::default()).unwrap();
    let found: Vec<(ViolationKind, String, u32)> = report
        .violations
        .iter()
        .filter(|v| v.kind == ViolationKind::UndeclaredEdge)
        .map(|v| {
            let at = v.at.as_ref().unwrap();
            (v.kind, at.file.display().to_string(), at.line)
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                ViolationKind::UndeclaredEdge,
                "scripts/deploy.sh".to_owned(),
                3
            ),
            (
                ViolationKind::UndeclaredEdge,
                "scripts/deploy.sh".to_owned(),
                4
            ),
            (
                ViolationKind::UndeclaredEdge,
                "scripts/release.sh".to_owned(),
                3
            ),
            (
                ViolationKind::UndeclaredEdge,
                "scripts/release.sh".to_owned(),
                4
            ),
        ]
    );
}

#[test]
fn extensionless_scripts_are_source_and_other_extensionless_files_are_not() {
    let dir = repo(&files());
    let ownership = driftwall::ownership::scan(dir.path()).unwrap();
    let unmapped: Vec<String> = ownership
        .unmapped
        .iter()
        .filter(|path| {
            !matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("swift" | "sh")
            )
        })
        .map(|path| path.display().to_string())
        .collect();
    assert_eq!(unmapped, ["bin/provision", "libpy/helper.py", "tools/gen"]);
    assert!(
        ownership
            .asset_unmapped
            .iter()
            .any(|path| path.ends_with("docs/NOTES"))
    );
    assert!(
        ownership
            .asset_unmapped
            .iter()
            .any(|path| path.ends_with("docs/runner"))
    );
}

#[test]
fn an_extensionless_python_script_imports_like_a_py_file_and_a_bash_one_sources() {
    let dir = repo(&files());
    let init = run_on("init", dir.path(), &[]);
    assert_eq!(init.status.code(), Some(0), "{}", text(&init.stderr));
    let observation = lint::observe(dir.path(), &LintOptions::default()).unwrap();
    let sites: Vec<String> = observation
        .observed
        .iter()
        .flat_map(|(edge, sites)| {
            sites.iter().map(move |site| {
                format!(
                    "{}:{} {}->{}",
                    site.file.display(),
                    site.line,
                    edge.from,
                    edge.to
                )
            })
        })
        .filter(|site| site.starts_with("bin/") || site.starts_with("tools/"))
        .collect();
    assert_eq!(
        sites,
        ["bin/provision:2 bin->device", "tools/gen:2 tools->libpy"]
    );
}
