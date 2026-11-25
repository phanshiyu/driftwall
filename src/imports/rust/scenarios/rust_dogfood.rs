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
        // why: `diagram/mod.rs` only re-exports `parse`, so lint depends on the file that defines it.
        "src/diagram/parse.rs",
        "src/ownership/mod.rs",
        // why: `imports/mod.rs` only re-exports `collect_with_facades`, so lint depends on the file that defines it.
        "src/imports/collect.rs",
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
        files: Files {
            known: &known,
            assets: &BTreeSet::new(),
            unowned: &BTreeSet::new(),
        },
        test_files: &BTreeSet::new(),
        web: fixtures::no_web(),
        python: &PythonLayout::default(),
        rust: crate::imports::rust::Env {
            crates: &crates,
            paths: &PathModules::default(),
            dependencies: &BTreeMap::new(),
        },
        swift: &SwiftModules::default(),
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
    // why: model/mod.rs declares and re-exports its submodules, baseline.rs, blind.rs, boundary.rs, located.rs, map.rs, moved.rs, open_order.rs, package_rule.rs, rules.rs and status.rs, and nothing else.
    assert_eq!(
        dependencies(&all, "src/model/mod.rs"),
        BTreeSet::from([
            "src/model/baseline.rs".to_owned(),
            "src/model/blind.rs".to_owned(),
            "src/model/boundary.rs".to_owned(),
            "src/model/located.rs".to_owned(),
            "src/model/map.rs".to_owned(),
            "src/model/moved.rs".to_owned(),
            "src/model/open_order.rs".to_owned(),
            "src/model/package_rule.rs".to_owned(),
            "src/model/rules.rs".to_owned(),
            "src/model/status.rs".to_owned()
        ])
    );
}

/// Every symbol `file` uses from `target`, across all of its import lines, as serialized.
fn symbols_used(
    all: &BTreeMap<PathBuf, FileImports>,
    file: &str,
    target: &str,
) -> BTreeSet<String> {
    all.get(Path::new(file))
        .expect("file was collected")
        .resolved
        .iter()
        .filter(|import| import.target == Path::new(target))
        .flat_map(|import| import.symbols.iter().map(ToString::to_string))
        .collect()
}

/// Asserts `file` uses every one of `names` from `target`. Other symbols may be used too, so the
/// test survives unrelated edits to the real source.
fn assert_uses(all: &BTreeMap<PathBuf, FileImports>, file: &str, target: &str, names: &[&str]) {
    let used = symbols_used(all, file, target);
    for name in names {
        assert!(
            used.contains(*name),
            "{file} -> {target}: {name} in {used:?}"
        );
    }
}

#[test]
fn lint_uses_the_diagram_parser_by_name() {
    // why: `diagram::parse(&source)` is reached through the `self` binding of `use crate::diagram`;
    // why: `diagram/mod.rs` only re-exports it, so the sites are at the files that define them.
    let all = dogfood();
    assert_uses(
        &all,
        "src/lint/mod.rs",
        "src/diagram/error.rs",
        &["DiagramError"],
    );
    assert_uses(&all, "src/lint/mod.rs", "src/diagram/parse.rs", &["parse"]);
}

#[test]
fn the_binary_uses_lint_observe_through_the_bound_module_name() {
    // why: `use driftwall::lint::{self, LintOptions};` and `lint::observe(&args.path, &options)`.
    assert_uses(
        &dogfood(),
        "src/cli/commands.rs",
        "src/lint/mod.rs",
        &["LintOptions", "observe"],
    );
}

#[test]
fn a_module_import_group_is_a_dependency_that_uses_no_symbol() {
    // why: `use crate::{diff, locate, views};` binds three sibling modules and names no item of them.
    let all = dogfood();
    for target in ["src/cli/diff.rs", "src/cli/locate.rs", "src/cli/views.rs"] {
        let imports: Vec<&crate::model::ResolvedImport> = all
            .get(Path::new("src/cli/commands.rs"))
            .expect("file was collected")
            .resolved
            .iter()
            .filter(|import| import.target == Path::new(target))
            .collect();
        assert!(!imports.is_empty(), "no import of {target}");
        assert!(
            imports
                .iter()
                .all(|import| !import.symbols.contains(&crate::model::Symbol::All)),
            "{target}: {imports:?}"
        );
    }
}

#[test]
fn the_binary_uses_the_report_renderers_by_name() {
    // why: `report/mod.rs` only re-exports the renderers, so each is used at its own file.
    let all = dogfood();
    assert_uses(
        &all,
        "src/cli/commands.rs",
        "src/report/text.rs",
        &["render_text"],
    );
    assert_uses(
        &all,
        "src/cli/commands.rs",
        "src/report/json.rs",
        &["render_json"],
    );
    assert_uses(
        &all,
        "src/cli/commands.rs",
        "src/report/sarif.rs",
        &["render_sarif"],
    );
}

#[test]
fn symbols_are_deterministic_across_runs() {
    assert_eq!(dogfood(), dogfood());
}

#[test]
fn rust_files_never_leave_unresolved_imports() {
    for (file, imports) in &dogfood() {
        assert_eq!(imports.unresolved, Vec::new(), "{}", file.display());
    }
}
