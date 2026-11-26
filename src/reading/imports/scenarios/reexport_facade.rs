//! Facade nodes stop the walk of a re-export, see "Facades" in "Re-exports" in `docs/DESIGN.md`.

use std::path::{Path, PathBuf};

use super::fixtures::repo;
use super::*;

const FILES: &[(&str, &str)] = &[
    ("lib/index.ts", "export { Comment } from './wire';\n"),
    (
        "lib/wire.ts",
        "export type { Comment } from '../types/types';\n",
    ),
    ("lib/own.ts", "import { Comment } from './wire';\n"),
    ("types/types.ts", "export interface Comment {}\n"),
    ("page.ts", "import { Comment } from './lib/index';\n"),
];

/// The target of the single import of `file` when `wire.ts` is owned by a facade node.
fn target_of(file: &str, facades: &[&str]) -> String {
    let repo = repo(FILES);
    let facades: Facades = facades
        .iter()
        .map(|path| (PathBuf::from(path), "lib".to_owned()))
        .collect();
    let mut all = collect_with_facades(
        repo.root(),
        &repo.files(),
        &repo.assets(),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &facades,
    )
    .unwrap();
    let found = all.remove(Path::new(file)).unwrap();
    assert_eq!(found.resolved.len(), 1, "{:?}", found.resolved);
    found.resolved[0].target.display().to_string()
}

#[test]
fn without_a_facade_the_walk_reaches_the_definition() {
    assert_eq!(target_of("page.ts", &[]), "types/types.ts");
}

#[test]
fn the_walk_stops_at_the_facade_file_for_an_importer_outside_its_node() {
    assert_eq!(
        target_of("page.ts", &["lib/wire.ts", "lib/index.ts", "lib/own.ts"]),
        "lib/index.ts"
    );
    // why: only `wire.ts` is in the facade node here, so the barrel in front of it is followed to it.
    assert_eq!(target_of("page.ts", &["lib/wire.ts"]), "lib/wire.ts");
}

#[test]
fn an_importer_inside_the_facade_node_follows_as_usual() {
    let node = ["lib/wire.ts", "lib/own.ts"];
    assert_eq!(target_of("lib/own.ts", &node), "types/types.ts");
}

#[test]
fn a_module_reexport_stops_at_a_facade_like_a_name_reexport() {
    let files = [
        ("src/lib.rs", "mod a;\nmod b;\nmod c;\n"),
        ("src/a.rs", "pub fn f() {}\n"),
        ("src/c.rs", "pub use crate::a;\n"),
        ("src/b.rs", "use crate::c::a::f;\n"),
    ];
    let repo = repo(&files);
    let followed = |facades: &Facades| {
        let mut all = collect_with_facades(
            repo.root(),
            &repo.files(),
            &repo.assets(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            facades,
        )
        .unwrap();
        let found = all.remove(Path::new("src/b.rs")).unwrap();
        found.resolved[0].target.display().to_string()
    };
    assert_eq!(followed(&Facades::new()), "src/a.rs");
    let facade: Facades = [(PathBuf::from("src/c.rs"), "c".to_owned())].into();
    assert_eq!(followed(&facade), "src/c.rs");
}
