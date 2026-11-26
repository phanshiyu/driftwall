//! Rust paths that name another crate or module in a roundabout way: a leading `::`, a renamed
//! Cargo dependency, a module re-export and a glob of a re-exporter (see "Rust resolution" and
//! "Re-exports" in `docs/DESIGN.md`), and a nested repository, which `where` does not name an owner
//! for.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

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

    pub fn run(root: &Path, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_driftwall"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command.args(args).current_dir(root).output().unwrap()
    }
}

use support::{repo, run};

const WORKSPACE: &str = "[workspace]\nmembers = [\"crates/*\"]\n";
const A_MANIFEST: &str = "[package]\nname = \"a-lib\"\nversion = \"0.1.0\"\n";
const A_LIB: &str = "pub fn a() {}\npub struct A;\n";
const CONTRACT: &str = "%% map a: crates/a-lib/\n%% map b: crates/b/\nflowchart LR\n  a\n  b\n";

fn b_manifest(dependency: &str) -> String {
    format!("[package]\nname = \"b\"\nversion = \"0.1.0\"\n[dependencies]\n{dependency}\n")
}

fn lint_text(dir: &tempfile::TempDir) -> (bool, String) {
    let output = run(dir.path(), &["lint"]);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn a_leading_double_colon_path_is_an_edge_like_the_plain_path() {
    for source in [
        "use a_lib::A;\n",
        "use ::a_lib::A;\n",
        "pub fn f() { ::a_lib::a(); }\n",
        "pub fn f() { println!(\"{}\", ::a_lib::a()); }\n",
    ] {
        let manifest = b_manifest("a-lib = { path = \"../a-lib\" }");
        let dir = repo(&[
            ("Cargo.toml", WORKSPACE),
            ("crates/a-lib/Cargo.toml", A_MANIFEST),
            ("crates/a-lib/src/lib.rs", A_LIB),
            ("crates/b/Cargo.toml", &manifest),
            ("crates/b/src/lib.rs", source),
            ("architecture.mmd", CONTRACT),
        ]);
        let (ok, text) = lint_text(&dir);
        assert!(!ok && text.contains("undeclared-edge"), "{source}: {text}");
    }
}

#[test]
fn a_renamed_dependency_is_an_edge_to_the_member_crate() {
    let manifest = b_manifest("alpha = { path = \"../a-lib\", package = \"a-lib\" }");
    let dir = repo(&[
        ("Cargo.toml", WORKSPACE),
        ("crates/a-lib/Cargo.toml", A_MANIFEST),
        ("crates/a-lib/src/lib.rs", A_LIB),
        ("crates/b/Cargo.toml", &manifest),
        ("crates/b/src/lib.rs", "use alpha::A;\n"),
        ("architecture.mmd", CONTRACT),
    ]);
    let (ok, text) = lint_text(&dir);
    assert!(!ok && text.contains("undeclared-edge"), "{text}");
    let declared = format!("{CONTRACT}  b --> a\n");
    std::fs::write(dir.path().join("architecture.mmd"), declared).unwrap();
    let (ok, text) = lint_text(&dir);
    assert!(ok, "{text}");
}

#[test]
fn a_rename_applies_to_its_own_crate_only() {
    let renamed = b_manifest("alpha = { path = \"../a-lib\", package = \"a-lib\" }");
    let plain = b_manifest("");
    let dir = repo(&[
        ("Cargo.toml", WORKSPACE),
        ("crates/a-lib/Cargo.toml", A_MANIFEST),
        ("crates/a-lib/src/lib.rs", A_LIB),
        ("crates/b/Cargo.toml", &renamed),
        ("crates/b/src/lib.rs", ""),
        ("crates/c/Cargo.toml", &plain),
        ("crates/c/src/lib.rs", "use alpha::A;\n"),
        (
            "architecture.mmd",
            "%% map a: crates/a-lib/\n%% map b: crates/b/\n%% map c: crates/c/\nflowchart LR\n  a\n  b\n  c\n",
        ),
    ]);
    let (ok, text) = lint_text(&dir);
    assert!(ok, "{text}");
}

const MODULES: &str = "%% map a: src/a/\n%% map b: src/b/\n%% map c: src/c/\n%% map m: src/main.rs\nflowchart LR\n  a\n  b\n  c\n  m\n  b --> a\n";

fn modules(c: &str, b: &str) -> tempfile::TempDir {
    repo(&[
        (
            "Cargo.toml",
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\n",
        ),
        (
            "src/main.rs",
            "pub mod a;\npub mod b;\npub mod c;\nfn main() {}\n",
        ),
        ("src/a/mod.rs", "pub fn a() {}\npub struct A;\n"),
        ("src/b/mod.rs", b),
        ("src/c/mod.rs", c),
        ("architecture.mmd", MODULES),
    ])
}

#[test]
fn self_through_a_module_alias_is_a_reexport() {
    let dir = modules(
        "use crate::a as z;\npub use self::z::A;\npub type X = self::z::A;\n",
        "use crate::c::A;\nuse crate::c::X;\n",
    );
    let (ok, text) = lint_text(&dir);
    // why: `c` imports `a` itself, which the contract does not declare; `b` reaches `a`, not `c`.
    assert!(!ok && text.contains("`c` imports `a`"), "{text}");
    assert!(!text.contains("`b` imports `c`"), "{text}");
}

#[test]
fn a_module_reexport_makes_the_edge_go_to_the_module() {
    for (c, b) in [
        ("pub use crate::a;\n", "pub fn f() { crate::c::a::a(); }\n"),
        ("pub use crate::a as inner;\n", "use crate::c::inner::A;\n"),
    ] {
        let dir = modules(c, b);
        let (_, text) = lint_text(&dir);
        assert!(!text.contains("`b` imports `c`"), "{text}");
        assert!(!text.contains("`b` imports `a`"), "declared edge: {text}");
        assert!(!text.contains("stale-edge"), "{text}");
    }
}

#[test]
fn a_glob_of_a_reexporter_makes_the_edge_go_to_what_it_reexports() {
    let dir = modules(
        "pub use crate::a::A;\n",
        "use crate::c::*;\npub fn f(_: A) {}\n",
    );
    let (_, text) = lint_text(&dir);
    assert!(!text.contains("`b` imports `c`"), "{text}");
    assert!(!text.contains("stale-edge"), "{text}");
}

#[test]
fn an_inline_module_of_reexports_makes_the_edge_go_to_what_it_reexports() {
    let dir = modules(
        "pub mod r {\n    pub use crate::a::A;\n}\n",
        "use crate::c::r::A;\n",
    );
    let (_, text) = lint_text(&dir);
    assert!(!text.contains("`b` imports `c`"), "{text}");
    assert!(!text.contains("stale-edge"), "{text}");
}

#[test]
fn a_module_reexport_stays_without_the_edge_when_c_is_not_a_reexporter() {
    // why: control, a private `use` of the module is no re-export, so `b` really imports `c`.
    let dir = modules("use crate::a;\npub fn f() {}\n", "use crate::c::a::A;\n");
    let (ok, text) = lint_text(&dir);
    assert!(!ok && text.contains("`b` imports `c`"), "{text}");
}

#[test]
fn a_test_attribute_outside_cfg_test_is_test_code() {
    let dir = repo(&[
        (
            "Cargo.toml",
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\n",
        ),
        ("src/main.rs", "pub mod a;\npub mod b;\nfn main() {}\n"),
        ("src/a.rs", "pub fn a() {}\n"),
        (
            "src/b.rs",
            "#[test]\nfn t() { crate::a::a(); }\n#[tokio::test]\nasync fn u() { crate::a::a(); }\n",
        ),
        (
            "architecture.mmd",
            "%% map a: src/a.rs\n%% map b: src/b.rs\n%% map m: src/main.rs\nflowchart LR\n  a\n  b\n  m\n  b -->|test| a\n",
        ),
    ]);
    let (ok, text) = lint_text(&dir);
    assert!(ok, "{text}");
}

#[test]
fn where_says_a_file_in_a_nested_repository_is_not_read() {
    let dir = repo(&[
        ("architecture.mmd", "%% map a: src/a/\nflowchart LR\n  a\n"),
        ("src/a/x.ts", ""),
        ("src/a/vendored/.git", "gitdir: ../../../elsewhere\n"),
        ("src/a/vendored/y.ts", ""),
        ("src/a/hollow/.git", ""),
        ("src/a/hollow/z.ts", ""),
    ]);
    let say = |file: &str| {
        let output = run(dir.path(), &["where", file]);
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    assert!(
        say("src/a/vendored/y.ts").contains("not read (inside a nested repository)"),
        "{}",
        say("src/a/vendored/y.ts")
    );
    // why: an empty `.git` entry marks no repository, so the file is an ordinary source file of `a`.
    assert!(
        say("src/a/hollow/z.ts").contains("a (via"),
        "{}",
        say("src/a/hollow/z.ts")
    );
}
