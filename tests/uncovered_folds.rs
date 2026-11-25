//! A declarations-only entry file no rule covers is folded, not `unmapped-file` (see "Folded entry
//! files" in `docs/DESIGN.md`).

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

    /// Runs the binary with every `GIT_*` variable removed, so a hook that runs the tests cannot
    /// point git at the repository being committed.
    pub fn driftwall(root: &Path, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_driftwall"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command.args(args).current_dir(root).output().unwrap()
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

use support::{code, driftwall, repo, stdout};

const CLEAN: &str = "driftwall: clean\n";

/// A feature module whose `mod.rs` only declares its parts, mapped by subdirectory.
fn billing(mod_rs: &str, maps: &str) -> tempfile::TempDir {
    let contract = format!("flowchart LR\n  billing-service\n  billing-controller\n{maps}");
    repo(&[
        ("architecture.mmd", &contract),
        ("src/features/billing/mod.rs", mod_rs),
        ("src/features/billing/service/run.rs", "pub fn run() {}\n"),
        (
            "src/features/billing/controller/handle.rs",
            "pub fn handle() {}\n",
        ),
    ])
}

const BY_SUBDIRECTORY: &str = "%% map billing-service: src/features/billing/service/\n%% map billing-controller: src/features/billing/controller/\n";
const DECLARATIONS: &str =
    "//! Billing.\n#[cfg(test)]\nmod tests;\nmod controller;\npub(crate) mod service;\n";

#[test]
fn a_declarations_only_mod_rs_under_subdirectory_maps_is_folded_and_lint_is_clean() {
    let dir = billing(DECLARATIONS, BY_SUBDIRECTORY);
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, CLEAN.to_owned()),
        "{lint:?}"
    );
    let at = driftwall(dir.path(), &["where", "src/features/billing/mod.rs"]);
    assert_eq!(
        (code(&at), stdout(&at)),
        (
            0,
            "src/features/billing/mod.rs: folded, no node (no rule covers it)\n".to_owned()
        )
    );
    let json = driftwall(
        dir.path(),
        &["where", "src/features/billing/mod.rs", "--format", "json"],
    );
    let json = stdout(&json);
    assert!(json.contains("\"kind\": \"folded\""), "{json}");
    assert!(json.contains("\"node\": null"), "{json}");
}

#[test]
fn the_same_mod_rs_with_a_use_line_is_unmapped() {
    let dir = billing(
        "mod controller;\nmod service;\npub use service::run::run;\n",
        BY_SUBDIRECTORY,
    );
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(code(&lint), 1, "{lint:?}");
    assert!(stdout(&lint).contains("unmapped-file"), "{}", stdout(&lint));
    let at = driftwall(dir.path(), &["where", "src/features/billing/mod.rs"]);
    assert_eq!(
        (code(&at), stdout(&at)),
        (1, "src/features/billing/mod.rs: unmapped\n".to_owned())
    );
}

#[test]
fn an_inline_module_makes_a_mod_rs_a_declaration() {
    let dir = billing(
        "mod controller;\nmod service;\nmod x { }\n",
        BY_SUBDIRECTORY,
    );
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(code(&lint), 1, "{lint:?}");
}

#[test]
fn a_covered_mod_rs_keeps_its_owner() {
    let maps = format!("{BY_SUBDIRECTORY}%% map billing-controller: src/features/billing/mod.rs\n");
    let dir = billing(DECLARATIONS, &maps);
    let at = driftwall(dir.path(), &["where", "src/features/billing/mod.rs"]);
    assert_eq!(
        (code(&at), stdout(&at)),
        (
            0,
            "src/features/billing/mod.rs: billing-controller (via architecture.mmd:6 (map src/features/billing/mod.rs))\n"
                .to_owned()
        )
    );
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, CLEAN.to_owned()),
        "{lint:?}"
    );
}

#[test]
fn lib_rs_and_init_py_no_rule_covers_are_folded_under_plain_maps() {
    let dir = repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  core\n  tools\n%% map core: src/core/\n%% map tools: tools/\n",
        ),
        ("src/lib.rs", "pub mod core;\n"),
        ("src/core/run.rs", "pub fn run() {}\n"),
        ("tools/__init__.py", "\"\"\"Tools.\"\"\"\n"),
        ("tools/run.py", "def run():\n    pass\n"),
        ("src/main.rs", "mod core;\n"),
    ]);
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, CLEAN.to_owned()),
        "{lint:?}"
    );
    let at = driftwall(dir.path(), &["where", "src/lib.rs"]);
    assert_eq!(
        stdout(&at),
        "src/lib.rs: folded, no node (no rule covers it)\n"
    );
    let at = driftwall(dir.path(), &["where", "src/main.rs"]);
    assert_eq!(
        stdout(&at),
        "src/main.rs: folded, no node (no rule covers it)\n"
    );
}

#[test]
fn lib_rs_and_init_py_no_tag_covers_are_folded_under_plain_tags() {
    let dir = repo(&[
        ("architecture.mmd", "flowchart LR\n  core\n  tools\n"),
        ("src/core/.arch", "node: core\n"),
        ("src/core/run.rs", "pub fn run() {}\n"),
        ("src/lib.rs", "pub mod core;\n"),
        ("tools/.arch", "node: tools\n"),
        ("tools/run.py", "def run():\n    pass\n"),
        ("pkg/__init__.py", ""),
        ("src/features/mod.rs", "mod billing;\n"),
    ]);
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, CLEAN.to_owned()),
        "{lint:?}"
    );
    for file in ["src/lib.rs", "pkg/__init__.py", "src/features/mod.rs"] {
        let at = driftwall(dir.path(), &["where", file]);
        assert_eq!(
            (code(&at), stdout(&at)),
            (0, format!("{file}: folded, no node (no rule covers it)\n"))
        );
    }
}

#[test]
fn a_covered_entry_file_under_a_plain_tag_keeps_its_owner() {
    let dir = repo(&[
        ("architecture.mmd", "flowchart LR\n  core\n"),
        ("src/.arch", "node: core\n"),
        ("src/lib.rs", "pub mod a;\n"),
        ("src/mod.rs", "mod a;\n"),
        ("src/a.rs", "pub fn a() {}\n"),
    ]);
    for file in ["src/lib.rs", "src/mod.rs"] {
        let at = driftwall(dir.path(), &["where", file]);
        assert_eq!(
            stdout(&at),
            format!("{file}: core (via src/.arch)\n"),
            "{at:?}"
        );
    }
}

#[test]
fn a_module_form_map_over_a_feature_folds_its_declarations_only_mod_rs() {
    let dir = repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  billing-controller\n  billing-service\n%% map billing-*: src/features/billing/\n",
        ),
        (
            "src/features/billing/mod.rs",
            "mod controller;\nmod service;\n",
        ),
        ("src/features/billing/service/run.rs", "pub fn run() {}\n"),
        (
            "src/features/billing/controller/handle.rs",
            "pub fn handle() {}\n",
        ),
    ]);
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, CLEAN.to_owned()),
        "{lint:?}"
    );
    let at = driftwall(dir.path(), &["where", "src/features/billing/mod.rs"]);
    assert_eq!(
        (code(&at), stdout(&at)),
        (
            0,
            "src/features/billing/mod.rs: folded, no node (via architecture.mmd:4 (map src/features/billing/))\n"
                .to_owned()
        )
    );
    // why: with a declaration in it, the file is named after its directory: an undeclared node.
    let used = repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  billing-controller\n  billing-service\n%% map billing-*: src/features/billing/\n",
        ),
        (
            "src/features/billing/mod.rs",
            "pub use service::run::run;\nmod service;\n",
        ),
        ("src/features/billing/service/run.rs", "pub fn run() {}\n"),
        (
            "src/features/billing/controller/handle.rs",
            "pub fn handle() {}\n",
        ),
    ]);
    let lint = driftwall(used.path(), &["lint", ".", "--strict"]);
    assert!(stdout(&lint).contains("unknown-node"), "{}", stdout(&lint));
}

#[test]
fn a_mod_rs_under_node_star_folds_unless_a_module_has_its_name() {
    let dir = repo(&[
        ("architecture.mmd", "flowchart LR\n  billing\n  ledger\n"),
        ("src/features/.arch", "node: *\n"),
        ("src/features/mod.rs", "mod billing;\nmod ledger;\n"),
        ("src/features/billing/run.rs", "pub fn run() {}\n"),
        ("src/features/ledger/entries.rs", "pub fn e() {}\n"),
        ("src/features/ledger/mod.rs", "mod entries;\n"),
    ]);
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, CLEAN.to_owned()),
        "{lint:?}"
    );
    let top = driftwall(dir.path(), &["where", "src/features/mod.rs"]);
    assert_eq!(
        stdout(&top),
        "src/features/mod.rs: folded, no node (via src/features/.arch)\n"
    );
    let named = driftwall(dir.path(), &["where", "src/features/ledger/mod.rs"]);
    assert_eq!(
        stdout(&named),
        "src/features/ledger/mod.rs: ledger (via src/features/.arch)\n"
    );
}

#[test]
fn init_keeps_planning_an_uncovered_entry_file_and_lint_agrees() {
    let dir = repo(&[
        ("src/lib.rs", "pub mod a;\npub mod b;\n"),
        ("src/a.rs", "pub fn a() {}\n"),
        ("src/b.rs", "pub fn b() {}\n"),
    ]);
    let init = driftwall(dir.path(), &["init", "."]);
    assert_eq!(code(&init), 0, "{init:?}");
    let lint = driftwall(dir.path(), &["lint", ".", "--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, CLEAN.to_owned()),
        "{lint:?}"
    );
}
