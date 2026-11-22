//! Unit tests of how the plan treats Swift targets: one node per target, named after it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::*;
use crate::model::Language;

fn planned(paths: &[&str], units: &[Unit]) -> InitPlan {
    let files: BTreeMap<PathBuf, Language> = paths
        .iter()
        .map(|path| (PathBuf::from(path), Language::Swift))
        .collect();
    plan(
        &files,
        &BTreeSet::new(),
        &BTreeMap::new(),
        &InitOptions::default(),
        units,
        &Existing::default(),
    )
}

fn unit(dir: &str, kind: UnitKind, name: Option<&str>) -> Unit {
    Unit {
        dir: PathBuf::from(dir),
        kind,
        name: name.map(str::to_owned),
    }
}

fn swift(dir: &str, name: &str) -> Unit {
    unit(dir, UnitKind::SwiftTarget, Some(name))
}

fn tags(plan: &InitPlan) -> Vec<(String, String)> {
    plan.tags
        .iter()
        .map(|(dir, found)| (dir.display().to_string(), found.tag.to_string()))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(dir, tag)| ((*dir).to_owned(), (*tag).to_owned()))
        .collect()
}

fn node_ids(plan: &InitPlan) -> Vec<String> {
    plan.contract
        .nodes
        .keys()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_swift_target_is_one_node_named_after_the_target_and_never_split() {
    let targets = [
        swift("Sources/AppCore", "AppCore"),
        swift("Sources/app-cli", "app-cli"),
        swift("Tests/AppCoreTests", "AppCoreTests"),
    ];
    let p = planned(
        &[
            "Package.swift",
            "Sources/AppCore/A.swift",
            "Sources/AppCore/B.swift",
            "Sources/AppCore/Net/C.swift",
            "Sources/app-cli/main.swift",
            "Sources/app-cli/Other.swift",
            "Tests/AppCoreTests/T.swift",
            "Tests/AppCoreTests/U.swift",
        ],
        &targets,
    );
    assert_eq!(
        tags(&p),
        pairs(&[
            ("", "root"),
            ("Sources/AppCore", "AppCore"),
            ("Sources/app-cli", "app-cli"),
            ("Tests/AppCoreTests", "AppCoreTests"),
        ])
    );
    assert_eq!(node_ids(&p), ["AppCore", "AppCoreTests", "app-cli", "root"]);
}

#[test]
fn a_swift_target_name_is_sanitised_and_collisions_are_numbered() {
    let targets = [
        swift("Sources/My.Lib", "My.Lib"),
        swift("Sources/my-lib", "my-lib"),
        swift("Sources/9lives", "9lives"),
        swift("Sources/other", "my-lib"),
    ];
    let p = planned(
        &[
            "Sources/My.Lib/a.swift",
            "Sources/my-lib/a.swift",
            "Sources/9lives/a.swift",
            "Sources/other/a.swift",
        ],
        &targets,
    );
    assert_eq!(
        tags(&p),
        pairs(&[
            ("Sources/9lives", "n-9lives"),
            ("Sources/My.Lib", "My-Lib"),
            ("Sources/my-lib", "my-lib"),
            ("Sources/other", "my-lib-2"),
        ])
    );
}

#[test]
fn a_lone_swift_target_is_not_the_primary_unit() {
    let p = planned(
        &["Sources/Shared/a.swift", "Sources/Shared/b.swift"],
        &[swift("Sources/Shared", "Shared")],
    );
    assert_eq!(tags(&p), pairs(&[("Sources/Shared", "Shared")]));
}

#[test]
fn two_swift_targets_in_one_directory_share_a_node_named_after_the_first_by_name() {
    let targets = [
        swift("Sources/Shared", "Alpha"),
        swift("Sources/Shared", "Zed"),
    ];
    let p = planned(
        &["Sources/Shared/a.swift", "Sources/Shared/b.swift"],
        &targets,
    );
    assert_eq!(tags(&p), pairs(&[("Sources/Shared", "Alpha")]));
}

#[test]
fn a_swift_target_inside_a_decomposed_unit_takes_its_own_files() {
    let units = [
        unit("", UnitKind::Package, None),
        swift("Sources/App", "App"),
    ];
    let p = planned(
        &["src/a.swift", "src/b.swift", "Sources/App/main.swift"],
        &units,
    );
    assert_eq!(tags(&p), pairs(&[("Sources/App", "App"), ("src", "*")]));
    assert_eq!(node_ids(&p), ["App", "a", "b"]);
}
