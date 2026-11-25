//! A Vite alias whose target is a build output: skipped, naming the node whose directory holds it.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::code::{SkipReason, Via};
use crate::contract::{Edge, NodeId};
use crate::results::ViolationKind;

const CONTRACT: &str = "flowchart LR\n  player[\"player\"]\n  wasm[\"wasm\"]\n  player --> wasm\n";
const CONFIG: &str = "import path from 'node:path';\nexport default { resolve: { alias: { 'mrc-wasm': path.resolve(__dirname, '../engine/wasm/pkg') } } };\n";

fn repo(extra: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    let base = [
        ("architecture.mmd", CONTRACT),
        ("src/.arch", "node: player\n"),
        ("src/vite.config.ts", CONFIG),
        (
            "src/player/CanvasPlayer.tsx",
            "import x from 'left-pad';\nimport init from 'mrc-wasm';\n",
        ),
    ];
    for (path, contents) in base.iter().chain(extra) {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    dir
}

fn aliased(dir: &TempDir) -> Vec<(u32, String, Option<String>, Option<String>)> {
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    assert_eq!(observation.report.violations, Vec::new());
    observation
        .blind_spots
        .skipped
        .get(&SkipReason::AliasTargetMissing)
        .into_iter()
        .flatten()
        .map(|reference| {
            (
                reference.at.line,
                reference.specifier.clone(),
                reference.node.as_ref().map(ToString::to_string),
                reference.message.clone(),
            )
        })
        .collect()
}

#[test]
fn a_target_inside_a_directory_owned_by_a_node_names_that_node() {
    let dir = repo(&[
        ("engine/wasm/.arch", "node: wasm\n"),
        ("engine/wasm/lib.rs", ""),
    ]);
    assert_eq!(
        aliased(&dir),
        [(
            2,
            "mrc-wasm".to_owned(),
            Some("player".to_owned()),
            Some(
                "alias `mrc-wasm` points to `engine/wasm/pkg`, which has no file in the repository; the target lies in the directory of node `wasm`"
                    .to_owned()
            )
        )]
    );
}

#[test]
fn a_target_in_no_owned_directory_is_skipped_without_a_node() {
    let dir = repo(&[]);
    with_contract(&dir, "flowchart LR\n  player\n  wasm\n");
    let rows = aliased(&dir);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0].3.as_deref(),
        Some("alias `mrc-wasm` points to `engine/wasm/pkg`, which has no file in the repository")
    );
}

#[test]
fn the_package_import_beside_it_is_still_an_undeclared_package() {
    let dir = repo(&[]);
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    let packages: Vec<_> = observation
        .blind_spots
        .skipped
        .get(&SkipReason::UndeclaredPackage)
        .into_iter()
        .flatten()
        .map(|reference| reference.specifier.clone())
        .collect();
    assert_eq!(packages, ["left-pad"]);
}

fn with_contract(dir: &TempDir, contract: &str) {
    fs::write(dir.path().join("architecture.mmd"), contract).unwrap();
}

fn owned_wasm() -> [(&'static str, &'static str); 2] {
    [
        ("engine/wasm/.arch", "node: wasm\n"),
        ("engine/wasm/lib.rs", ""),
    ]
}

#[test]
fn the_alias_realises_a_solid_edge_and_stays_skipped() {
    let dir = repo(&owned_wasm());
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    assert_eq!(observation.report.violations, Vec::new());
    let edge = Edge {
        from: NodeId::new("player").unwrap(),
        to: NodeId::new("wasm").unwrap(),
    };
    let sites = &observation.observed[&edge];
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].at.line, 2);
    assert_eq!(sites[0].via, Some(Via::AliasToBuildOutput));
    assert_eq!(
        serde_json::to_value(&sites[0]).unwrap()["via"],
        "alias-to-build-output"
    );
    assert_eq!(aliased(&dir).len(), 1);
}

#[test]
fn an_undeclared_dependency_through_the_alias_is_a_violation() {
    let dir = repo(&owned_wasm());
    with_contract(&dir, "flowchart LR\n  player\n  wasm\n");
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    let found: Vec<_> = observation
        .report
        .violations
        .iter()
        .map(|v| (v.kind, v.at.as_ref().map(|at| at.line)))
        .collect();
    assert_eq!(found, [(ViolationKind::UndeclaredEdge, Some(2))]);
}

#[test]
fn a_planned_edge_is_realised_by_the_alias() {
    let dir = repo(&owned_wasm());
    with_contract(&dir, "flowchart LR\n  player\n  wasm\n  player -.-> wasm\n");
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    let kinds: Vec<_> = observation
        .report
        .violations
        .iter()
        .map(|v| v.kind)
        .collect();
    assert_eq!(kinds, [ViolationKind::PlannedRealized]);
}

#[test]
fn a_build_output_in_no_owned_directory_makes_no_edge() {
    let dir = repo(&[]);
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    assert!(observation.observed.is_empty());
}

#[test]
fn a_map_subtree_owns_the_build_output_too() {
    let dir = repo(&owned_wasm());
    fs::remove_file(dir.path().join("src/.arch")).unwrap();
    fs::remove_file(dir.path().join("engine/wasm/.arch")).unwrap();
    with_contract(
        &dir,
        "flowchart LR\n  player\n  wasm\n  player --> wasm\n  %% map player: src/\n  %% map wasm: engine/wasm/\n",
    );
    let observation = observe(dir.path(), &LintOptions::default()).unwrap();
    assert_eq!(observation.report.violations, Vec::new());
    assert_eq!(observation.observed.len(), 1);
}
