//! The blind spot census of `scan`: unread code files and files only `.driftwallignore` hides.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::scan;

fn tree(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    dir
}

fn set(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

#[test]
fn code_files_without_a_language_are_unread_by_extension() {
    let dir = tree(&[
        (".arch", "node: root\n"),
        ("App.swift", "import UIKit\n"),
        ("ios/View.swift", ""),
        ("build.sh", "echo\n"),
        ("lib/x.c", ""),
        ("lib/x.h", ""),
        ("page.html", ""),
        ("data.json", "{}"),
        ("README.md", ""),
    ]);
    let owned = scan(dir.path()).unwrap();
    let keys: Vec<(&str, BTreeSet<PathBuf>)> = owned
        .unread
        .iter()
        .map(|(key, files)| (key.as_str(), files.clone()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("c", set(&["lib/x.c"])),
            ("h", set(&["lib/x.h"])),
            ("html", set(&["page.html"])),
            ("sh", set(&["build.sh"])),
            ("swift", set(&["App.swift", "ios/View.swift"])),
        ]
    );
}

#[test]
fn a_supported_extension_is_removed_from_the_table_so_ts_is_source_not_unread() {
    let dir = tree(&[(".arch", "node: root\n"), ("a.ts", ""), ("b.swift", "")]);
    let owned = scan(dir.path()).unwrap();
    assert_eq!(owned.unread.keys().collect::<Vec<_>>(), ["swift"]);
    assert_eq!(
        owned.owners.keys().collect::<Vec<_>>(),
        [&PathBuf::from("a.ts")]
    );
}

#[test]
fn extensionless_scripts_with_a_shebang_are_keyed_shebang() {
    let dir = tree(&[
        (".arch", "node: root\n"),
        ("bin/deploy", "#!/usr/bin/env bash\n"),
        ("Makefile", "all:\n"),
        ("LICENSE", "MIT\n"),
    ]);
    let owned = scan(dir.path()).unwrap();
    assert_eq!(owned.unread.len(), 1);
    assert_eq!(owned.unread["shebang"], set(&["bin/deploy"]));
}

#[test]
fn unread_files_are_never_unmapped_and_stay_assets() {
    let dir = tree(&[("loose.swift", ""), ("src/x.ts", "")]);
    let owned = scan(dir.path()).unwrap();
    assert_eq!(owned.unmapped, set(&["src/x.ts"]));
    assert_eq!(owned.unread["swift"], set(&["loose.swift"]));
    assert!(owned.asset_unmapped.contains(&PathBuf::from("loose.swift")));
}

#[test]
fn gitignored_code_files_are_not_unread() {
    let dir = tree(&[
        (".gitignore", "Pods/\n"),
        ("Pods/x.swift", ""),
        ("a.swift", ""),
    ]);
    let owned = scan(dir.path()).unwrap();
    assert_eq!(owned.unread["swift"], set(&["a.swift"]));
}

#[test]
fn source_files_only_driftwallignore_hides_are_ignored_but_gitignored_ones_are_not() {
    let dir = tree(&[
        (".gitignore", "git-hidden.ts\n"),
        (
            ".driftwallignore",
            "legacy/\nold.py\ngit-hidden.ts\nnotes.swift\n",
        ),
        (".arch", "node: root\n"),
        ("app.ts", ""),
        ("legacy/a.js", ""),
        ("legacy/b/c.rs", ""),
        ("old.py", ""),
        ("git-hidden.ts", ""),
        ("notes.swift", ""),
    ]);
    let owned = scan(dir.path()).unwrap();
    assert_eq!(
        owned.ignored,
        set(&["legacy/a.js", "legacy/b/c.rs", "old.py"])
    );
    assert_eq!(
        owned.owners.keys().collect::<Vec<_>>(),
        [&PathBuf::from("app.ts")]
    );
    assert!(owned.unmapped.is_empty());
    assert!(
        owned.unread.is_empty(),
        "a hidden unreadable file is not counted"
    );
}

#[test]
fn a_nested_driftwallignore_hides_its_own_subtree_only() {
    let dir = tree(&[
        (".arch", "node: root\n"),
        ("sub/.driftwallignore", "skip.ts\n"),
        ("sub/skip.ts", ""),
        ("sub/keep.ts", ""),
        ("skip.ts", ""),
    ]);
    let owned = scan(dir.path()).unwrap();
    assert_eq!(owned.ignored, set(&["sub/skip.ts"]));
}

#[test]
fn nothing_is_ignored_without_a_driftwallignore() {
    let dir = tree(&[(".arch", "node: root\n"), ("a.ts", "")]);
    assert!(scan(dir.path()).unwrap().ignored.is_empty());
}

#[test]
fn excluded_files_are_not_counted_as_ignored() {
    let dir = tree(&[
        (".driftwallignore", "other.js\n"),
        (".arch", "node: root\n"),
        ("vendor/lib.js", ""),
        ("other.js", ""),
    ]);
    let owned = scan(dir.path()).unwrap();
    assert_eq!(owned.ignored, set(&["other.js"]));
    assert_eq!(owned.excluded.values().map(BTreeSet::len).sum::<usize>(), 1);
}
