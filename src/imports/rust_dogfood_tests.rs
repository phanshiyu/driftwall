//! driftwall extracts the edges of its own source tree.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::*;

/// The `.rs` files below `dir`, relative to `root`, found with `std::fs` alone.
fn rust_files(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Language>) {
    for entry in fs::read_dir(root.join(dir)).expect("read_dir").flatten() {
        let relative = dir.join(entry.file_name());
        if entry.path().is_dir() {
            rust_files(root, &relative, out);
        } else if relative.extension().is_some_and(|ext| ext == "rs") {
            out.insert(relative, Language::Rust);
        }
    }
}

fn dogfood() -> BTreeMap<PathBuf, FileImports> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = BTreeMap::new();
    rust_files(root, Path::new("src"), &mut files);
    collect(root, &files).expect("collect succeeds")
}

fn dependencies(all: &BTreeMap<PathBuf, FileImports>, file: &str) -> BTreeSet<String> {
    all.get(Path::new(file))
        .expect("file was collected")
        .resolved
        .iter()
        .map(|import| import.target.display().to_string())
        .collect()
}

/// The first directory below `src/` of each path, which is the architecture node.
fn node_of(path: &Path) -> Option<String> {
    let mut parts = path.components().skip(1);
    let node = parts.next()?.as_os_str().to_str()?;
    parts.next()?;
    Some(node.to_owned())
}

#[test]
fn lint_depends_on_the_other_engine_nodes() {
    let all = dogfood();
    let deps = dependencies(&all, "src/lint/mod.rs");
    for expected in [
        "src/diagram/mod.rs",
        "src/ownership/mod.rs",
        "src/imports/mod.rs",
        "src/model/mod.rs",
    ] {
        assert!(deps.contains(expected), "missing {expected} in {deps:?}");
    }
}

#[test]
fn a_binary_reaches_lint_through_the_library_name_read_from_cargo_toml() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = BTreeMap::new();
    rust_files(root, Path::new("src"), &mut files);
    let known: BTreeSet<PathBuf> = files.keys().cloned().collect();
    let crates = library_crates(root, &known);
    assert_eq!(crates.get("driftwall"), Some(&PathBuf::from("src")));
    let env = Environment {
        known: &known,
        crates: &crates,
        aliases: &BTreeMap::new(),
    };
    // why: src/cli/main.rs is a stub today, so the source it will have is supplied here.
    let source = "use driftwall::lint;\nfn main() { driftwall::lint::run(); }\n";
    let got = extract_in(source, Language::Rust, Path::new("src/cli/main.rs"), &env)
        .expect("extract succeeds");
    let targets: Vec<&Path> = got.resolved.iter().map(|r| r.target.as_path()).collect();
    assert_eq!(
        targets,
        [Path::new("src/lint/mod.rs"), Path::new("src/lint/mod.rs")]
    );
}

#[test]
fn the_binary_modules_reach_the_library_through_its_name() {
    let all = dogfood();
    let deps = dependencies(&all, "src/cli/commands.rs");
    assert!(deps.contains("src/lint/mod.rs"), "{deps:?}");
    assert!(deps.contains("src/report/mod.rs"), "{deps:?}");
    assert!(deps.contains("src/cli/args.rs"), "{deps:?}");
}

#[test]
fn model_depends_on_no_other_node() {
    let all = dogfood();
    for (file, imports) in &all {
        if node_of(file).as_deref() != Some("model") {
            continue;
        }
        for import in &imports.resolved {
            assert_eq!(
                node_of(&import.target).as_deref(),
                Some("model"),
                "{} -> {}",
                file.display(),
                import.target.display()
            );
        }
    }
    assert!(dependencies(&all, "src/model/mod.rs").is_empty());
}

#[test]
fn rust_files_never_leave_unresolved_imports() {
    for (file, imports) in &dogfood() {
        assert_eq!(imports.unresolved, Vec::new(), "{}", file.display());
    }
}
