//! Assets: every walked file that is not source, owned by the same tag rules.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;
use crate::code::{Exclusion, Ownership};

fn tree(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    dir
}

fn asset_owners(ownership: &Ownership) -> Vec<(String, String)> {
    ownership
        .asset_owners
        .iter()
        .map(|(path, node)| (path.display().to_string(), node.to_string()))
        .collect()
}

fn asset_unmapped(ownership: &Ownership) -> Vec<String> {
    ownership
        .asset_unmapped
        .iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|&(path, node)| (path.to_owned(), node.to_owned()))
        .collect()
}

#[test]
fn an_asset_is_owned_by_the_nearest_arch_like_a_source_file() {
    let dir = tree(&[
        ("a/.arch", "node: alpha\n"),
        ("a/data.json", ""),
        ("a/style.css", ""),
        ("a/x.ts", ""),
        ("a/inner/.arch", "node: inner\n"),
        ("a/inner/logo.svg", ""),
        ("a/inner/deep/more.json", ""),
        ("loose.json", ""),
    ]);
    let o = scan(dir.path()).unwrap();
    assert_eq!(
        asset_owners(&o),
        pairs(&[
            ("a/data.json", "alpha"),
            ("a/inner/deep/more.json", "inner"),
            ("a/inner/logo.svg", "inner"),
            ("a/style.css", "alpha"),
        ])
    );
    assert_eq!(asset_unmapped(&o), ["loose.json"]);
}

#[test]
fn assets_change_nothing_about_sources_tags_or_exclusions() {
    let plain = tree(&[
        (".arch", "node: root\n"),
        ("a.ts", ""),
        ("vendor/v.js", ""),
        ("lone/b.ts", ""),
    ]);
    let with_assets = tree(&[
        (".arch", "node: root\n"),
        ("a.ts", ""),
        ("vendor/v.js", ""),
        ("lone/b.ts", ""),
        ("data.json", ""),
        ("style.css", ""),
        ("vendor/v.css", ""),
        ("img/x.png", ""),
    ]);
    let before = scan(plain.path()).unwrap();
    let after = scan(with_assets.path()).unwrap();
    assert_eq!(before.owners, after.owners);
    assert_eq!(before.unmapped, after.unmapped);
    assert_eq!(before.tags, after.tags);
    assert_eq!(before.tag_dirs, after.tag_dirs);
    assert_eq!(before.excluded, after.excluded);
    assert_eq!(
        after.excluded[&Exclusion::Vendored]
            .iter()
            .collect::<Vec<_>>(),
        [Path::new("vendor/v.js")]
    );
    assert_eq!(
        asset_owners(&after).len(),
        4,
        "a vendored asset is still an asset"
    );
}

#[test]
fn an_asset_is_never_unmapped_and_never_creates_a_tag() {
    let dir = tree(&[("a/.arch", "node: *\n"), ("a/x.ts", ""), ("a/logo.svg", "")]);
    let o = scan(dir.path()).unwrap();
    assert!(o.unmapped.is_empty());
    let tags: Vec<&str> = o.tags.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(tags, ["x"]);
    assert_eq!(asset_owners(&o), pairs(&[("a/logo.svg", "logo")]));
}

#[test]
fn modules_name_an_asset_by_the_module_rule() {
    let dir = tree(&[
        ("src/.arch", "node: *\n"),
        ("src/a.ts", ""),
        ("src/a.css", ""),
        ("src/a.module.css", ""),
        ("src/index.json", ""),
        ("src/assets/logo.png", ""),
    ]);
    let o = scan(dir.path()).unwrap();
    assert_eq!(
        asset_owners(&o),
        pairs(&[
            ("src/a.css", "a"),
            ("src/a.module.css", "a"),
            ("src/assets/logo.png", "assets"),
            ("src/index.json", "src"),
        ])
    );
}

#[test]
fn an_asset_whose_module_name_is_not_a_node_id_is_unmapped_not_an_error() {
    let dir = tree(&[
        ("src/.arch", "node: *\n"),
        ("src/a.ts", ""),
        ("src/.eslintrc.json", ""),
        ("src/__.json", ""),
    ]);
    let o = scan(dir.path()).unwrap();
    assert_eq!(asset_owners(&o), pairs(&[]));
    assert_eq!(asset_unmapped(&o), ["src/.eslintrc.json", "src/__.json"]);
}

#[test]
fn arch_files_and_ignored_files_are_not_assets() {
    let dir = tree(&[
        (".gitignore", "generated/\n*.log\n"),
        (".driftwallignore", "skipped.json\n"),
        (".arch", "node: root\n"),
        ("keep.json", ""),
        ("skipped.json", ""),
        ("debug.log", ""),
        ("generated/out.json", ""),
        ("src/x.ts", ""),
    ]);
    let o = scan(dir.path()).unwrap();
    let all: Vec<PathBuf> = o.asset_owners.keys().cloned().collect();
    assert_eq!(
        all,
        [
            PathBuf::from(".driftwallignore"),
            PathBuf::from(".gitignore"),
            PathBuf::from("keep.json")
        ]
    );
}
