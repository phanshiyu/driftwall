//! Unit tests of the pure plan, and of [`run`] against temp directories.

use std::collections::BTreeSet;
use std::fs;

use tempfile::TempDir;

use super::*;
use crate::imports::FileImports;
use crate::model::{Edge, EdgeKind, Location, NodeId, ResolvedImport};
use crate::ownership::Tag;

fn files(paths: &[&str]) -> BTreeMap<PathBuf, Language> {
    paths
        .iter()
        .map(|path| (PathBuf::from(path), Language::TypeScript))
        .collect()
}

fn import(from: &str, to: &str) -> (PathBuf, FileImports) {
    (
        PathBuf::from(from),
        FileImports {
            resolved: vec![ResolvedImport::new(
                Location {
                    file: PathBuf::from(from),
                    line: 1,
                },
                PathBuf::from(to),
            )],
            unresolved: Vec::new(),
        },
    )
}

fn plan_of(paths: &[&str], depth: Option<usize>) -> InitPlan {
    plan(&files(paths), &BTreeMap::new(), &InitOptions { depth })
}

/// The tagged directories (as `/` strings) with their ids.
fn tags(plan: &InitPlan) -> Vec<(String, String)> {
    plan.tags
        .iter()
        .map(|(dir, tag)| (dir.display().to_string(), tag.to_string()))
        .collect()
}

fn node_ids(plan: &InitPlan) -> Vec<String> {
    plan.contract
        .nodes
        .keys()
        .map(ToString::to_string)
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(dir, id)| ((*dir).to_owned(), (*id).to_owned()))
        .collect()
}

#[test]
fn single_chain_descends_to_the_first_fork() {
    let p = plan_of(&["src/auth/a.ts", "src/billing/b.ts"], None);
    assert_eq!(
        tags(&p),
        pairs(&[("src/auth", "auth"), ("src/billing", "billing")])
    );
    assert_eq!(
        p.contract.nodes[&NodeId::new("auth").unwrap()]
            .label
            .as_deref(),
        Some("src/auth")
    );
}

#[test]
fn a_longer_corridor_is_descended_too() {
    let p = plan_of(
        &["packages/app/src/a/x.ts", "packages/app/src/b/y.ts"],
        None,
    );
    assert_eq!(
        tags(&p),
        pairs(&[("packages/app/src/a", "a"), ("packages/app/src/b", "b")])
    );
}

#[test]
fn multi_branch_root_cuts_at_depth_one() {
    let p = plan_of(&["api/a.ts", "web/deep/b.ts", "web/c.ts"], None);
    assert_eq!(tags(&p), pairs(&[("api", "api"), ("web", "web")]));
}

#[test]
fn root_files_form_the_root_node_next_to_directories() {
    let p = plan_of(&["main.ts", "lib/a.ts", "lib/sub/b.ts"], None);
    assert_eq!(tags(&p), pairs(&[("", "root"), ("lib", "lib")]));
}

#[test]
fn one_module_in_a_directory_is_one_node() {
    let p = plan_of(&["src/a.ts", "src/a.test.ts"], None);
    assert_eq!(tags(&p), pairs(&[("src", "src")]));
    let flat = plan_of(&["a.ts"], None);
    assert_eq!(tags(&flat), pairs(&[("", "root")]));
}

#[test]
fn two_modules_in_the_source_root_become_module_nodes() {
    let p = plan_of(&["src/a.ts", "src/b.ts"], None);
    assert_eq!(tags(&p), pairs(&[("src", "*")]));
    assert_eq!(node_ids(&p), ["a", "b"]);
    let flat = plan_of(&["a.ts", "b.ts"], None);
    assert_eq!(tags(&flat), pairs(&[("", "*")]));
    assert_eq!(node_ids(&flat), ["a", "b"]);
}

#[test]
fn module_nodes_are_the_ids_ownership_derives() {
    let paths = [
        "src/main.rs",
        "src/gitlab.rs",
        "src/gitlab/client.rs",
        "src/routes/a.rs",
        "src/ui/mod.rs",
        "src/ui/b.test.rs",
    ];
    let p = plan_of(&paths, None);
    assert_eq!(tags(&p), pairs(&[("src", "*")]));
    assert_eq!(node_ids(&p), ["gitlab", "main", "routes", "ui"]);
    assert_eq!(
        p.contract.nodes[&NodeId::new("routes").unwrap()]
            .label
            .as_deref(),
        Some("src/routes")
    );
}

#[test]
fn module_nodes_get_edges_between_modules_and_directories() {
    let all = files(&["src/main.ts", "src/store.ts", "src/ui/b.ts", "src/ui/c.ts"]);
    let imports: BTreeMap<PathBuf, FileImports> = [
        import("src/main.ts", "src/ui/b.ts"),
        import("src/store.ts", "src/main.ts"),
        import("src/ui/b.ts", "src/ui/c.ts"),
        import("src/ui/c.ts", "src/store.ts"),
    ]
    .into_iter()
    .collect();
    let p = plan(&all, &imports, &InitOptions::default());
    let edges: Vec<String> = p
        .contract
        .edges
        .keys()
        .map(|edge| format!("{} {}", edge.from, edge.to))
        .collect();
    assert_eq!(edges, ["main ui", "store main", "ui store"]);
}

#[test]
fn one_module_next_to_directories_keeps_directory_nodes() {
    let p = plan_of(&["src/main.ts", "src/auth/a.ts", "src/billing/b.ts"], None);
    assert_eq!(
        tags(&p),
        pairs(&[
            ("src", "src"),
            ("src/auth", "auth"),
            ("src/billing", "billing")
        ])
    );
}

#[test]
fn an_explicit_depth_never_makes_module_nodes() {
    let paths = ["src/a.ts", "src/b.ts", "src/c/x.ts"];
    for depth in 0..=3 {
        let p = plan_of(&paths, Some(depth));
        assert!(
            p.tags.values().all(|tag| *tag != Tag::Modules),
            "depth {depth}"
        );
    }
    assert_eq!(
        tags(&plan_of(&paths, Some(2))),
        pairs(&[("src", "src"), ("src/c", "c")])
    );
}

#[test]
fn a_name_the_contract_cannot_hold_falls_back_to_directory_nodes() {
    for bad in [
        "src/_private.py",
        "src/.eslintrc.js",
        "src/end.rs",
        "src/9a.ts",
    ] {
        let p = plan_of(&["src/a.ts", "src/b.ts", bad], None);
        assert_eq!(tags(&p), pairs(&[("src", "src")]), "{bad}");
    }
    let bad_directory = plan_of(&["src/a.ts", "src/b.ts", "src/__tests__/t.ts"], None);
    assert_eq!(
        tags(&bad_directory),
        pairs(&[("src", "src"), ("src/__tests__", "n-__tests__")])
    );
}

#[test]
fn own_files_stop_the_descent_and_join_their_directory() {
    let p = plan_of(&["src/index.ts", "src/auth/a.ts", "src/billing/b.ts"], None);
    assert_eq!(
        tags(&p),
        pairs(&[
            ("src", "src"),
            ("src/auth", "auth"),
            ("src/billing", "billing")
        ])
    );
}

#[test]
fn explicit_depth_overrides_the_automatic_choice() {
    let paths = ["src/auth/a.ts", "src/auth/deep/c.ts", "src/billing/b.ts"];
    let shallow = plan_of(&paths, Some(1));
    assert_eq!(tags(&shallow), pairs(&[("src", "src")]));
    let deep = plan_of(&paths, Some(3));
    assert_eq!(
        tags(&deep),
        pairs(&[
            ("src/auth", "auth"),
            ("src/auth/deep", "auth-deep"),
            ("src/billing", "billing")
        ])
    );
    let none = plan_of(&paths, Some(0));
    assert_eq!(tags(&none), pairs(&[("", "root")]));
}

#[test]
fn ids_are_sanitised_and_never_reserved_or_colliding() {
    let p = plan_of(
        &[
            "x/end/a.ts",
            "x/9lives/b.ts",
            "x/my.pkg/c.ts",
            "x/my-pkg/d.ts",
            "x/%%/e.ts",
        ],
        Some(2),
    );
    let ids: Vec<String> = tags(&p).into_iter().map(|(_, id)| id).collect();
    assert_eq!(ids, ["n", "n-9lives", "end-2", "my-pkg", "my-pkg-2"]);
    for id in &ids {
        assert!(NodeId::new(id).is_ok(), "{id}");
    }
}

#[test]
fn a_directory_named_root_does_not_collide_with_the_root_node() {
    let p = plan_of(&["a.ts", "root/b.ts"], None);
    assert_eq!(tags(&p), pairs(&[("", "root"), ("root", "root-2")]));
}

#[test]
fn edges_are_solid_deduplicated_and_cross_owner_only() {
    let all = files(&["a/x.ts", "a/y.ts", "b/z.ts"]);
    let imports: BTreeMap<PathBuf, FileImports> = [
        import("a/x.ts", "b/z.ts"),
        import("a/y.ts", "b/z.ts"),
        import("a/x.ts", "a/y.ts"),
        import("b/z.ts", "elsewhere/missing.ts"),
        import("outside/o.ts", "b/z.ts"),
    ]
    .into_iter()
    .collect();
    let p = plan(&all, &imports, &InitOptions::default());
    let edges: Vec<(&Edge, &EdgeKind)> = p.contract.edges.iter().collect();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].0.from.as_str(), "a");
    assert_eq!(edges[0].0.to.as_str(), "b");
    assert_eq!(*edges[0].1, EdgeKind::Implemented);
}

#[test]
fn plan_is_deterministic() {
    let paths = ["z/a.ts", "m/b.ts", "a/c.ts"];
    assert_eq!(plan_of(&paths, None), plan_of(&paths, None));
}

#[test]
fn every_planned_node_is_tagged_exactly_once() {
    let p = plan_of(&["a/x.ts", "b/y.ts", "main.ts"], None);
    let tagged: BTreeSet<&NodeId> = p
        .tags
        .values()
        .filter_map(|tag| match tag {
            Tag::Node(id) => Some(id),
            Tag::Modules => None,
        })
        .collect();
    let declared: BTreeSet<&NodeId> = p.contract.nodes.keys().collect();
    assert_eq!(tagged, declared);
}

fn repo(entries: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (path, contents) in entries {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    dir
}

fn two_dirs() -> TempDir {
    repo(&[
        (
            "src/a/x.ts",
            "import { y } from '../b/y';\nexport const x = y;\n",
        ),
        ("src/b/y.ts", "export const y = 1;\n"),
    ])
}

#[test]
fn run_writes_contract_and_tags() {
    let dir = two_dirs();
    let summary = run(
        dir.path(),
        &InitOptions::default(),
        &WriteOptions::default(),
    )
    .unwrap();
    assert_eq!((summary.nodes, summary.edges, summary.tag_files), (2, 1, 2));
    let arch = fs::read_to_string(dir.path().join("src/a/.arch")).unwrap();
    assert_eq!(arch, "node: a\n");
    let contract = fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(contract.contains("a --> b"), "{contract}");
}

#[test]
fn no_sources_is_an_error() {
    let dir = repo(&[("README.md", "hi\n")]);
    let result = run(
        dir.path(),
        &InitOptions::default(),
        &WriteOptions::default(),
    );
    assert!(matches!(result, Err(InitError::NoSources)));
    assert!(!dir.path().join("architecture.mmd").exists());
}

#[test]
fn conflicts_list_every_path_sorted_and_write_nothing() {
    let dir = two_dirs();
    fs::write(dir.path().join("architecture.mmd"), "keep").unwrap();
    fs::write(dir.path().join("src/b/.arch"), "node: keep\n").unwrap();
    let result = run(
        dir.path(),
        &InitOptions::default(),
        &WriteOptions::default(),
    );
    let Err(InitError::Conflict { paths }) = result else {
        panic!("expected a conflict");
    };
    assert_eq!(
        paths,
        [
            PathBuf::from("architecture.mmd"),
            PathBuf::from("src/b/.arch")
        ]
    );
    assert!(!dir.path().join("src/a/.arch").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("architecture.mmd")).unwrap(),
        "keep"
    );
}

#[test]
fn force_overwrites() {
    let dir = two_dirs();
    fs::write(dir.path().join("architecture.mmd"), "keep").unwrap();
    let write = WriteOptions {
        force: true,
        ..WriteOptions::default()
    };
    run(dir.path(), &InitOptions::default(), &write).unwrap();
    let contract = fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(contract.starts_with("%% driftwall"), "{contract}");
}

#[test]
fn no_tags_writes_only_the_contract() {
    let dir = two_dirs();
    let write = WriteOptions {
        write_tags: false,
        ..WriteOptions::default()
    };
    let summary = run(dir.path(), &InitOptions::default(), &write).unwrap();
    assert_eq!(summary.tag_files, 0);
    assert!(!dir.path().join("src/a/.arch").exists());
}

#[test]
fn a_nested_arch_that_would_override_a_baseline_tag_is_refused() {
    let dir = two_dirs();
    fs::create_dir_all(dir.path().join("src/a/deep")).unwrap();
    fs::write(dir.path().join("src/a/deep/.arch"), "node: other\n").unwrap();
    fs::write(dir.path().join("src/a/deep/z.ts"), "export const z = 1;\n").unwrap();
    let write = WriteOptions {
        force: true,
        ..WriteOptions::default()
    };
    let result = run(dir.path(), &InitOptions::default(), &write);
    assert!(matches!(result, Err(InitError::Shadowed { .. })));
    assert!(!dir.path().join("architecture.mmd").exists());
}

#[test]
fn a_custom_contract_path_is_honoured() {
    let dir = two_dirs();
    let write = WriteOptions {
        contract: PathBuf::from("docs/arch.mmd"),
        ..WriteOptions::default()
    };
    let summary = run(dir.path(), &InitOptions::default(), &write).unwrap();
    assert_eq!(summary.contract, PathBuf::from("docs/arch.mmd"));
    assert!(dir.path().join("docs/arch.mmd").exists());
}
