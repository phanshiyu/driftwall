//! `%% map` lines: the contract maps code to nodes with no `.arch` file (see "Contract map" in
//! `docs/DESIGN.md`).

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

    /// A command with every `GIT_*` variable removed, so a hook that runs the tests cannot point
    /// git at the repository being committed.
    pub fn clean(mut command: Command) -> Command {
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
    }

    pub fn driftwall(root: &Path, args: &[&str]) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(args)
            .current_dir(root)
            .output()
            .unwrap()
    }

    pub fn text(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub fn error(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    pub fn walk_arch(root: &Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.file_name().is_some_and(|n| n == ".arch") {
                    found.push(path);
                }
            }
        }
        found
    }

    pub const CONTRACT: &str = "docs/architecture.mmd";

    /// Two nodes in separate directories, `ui` importing `api`, mapped from a contract in `docs/`.
    pub fn two_nodes(maps: &str) -> TempDir {
        let contract = format!("flowchart LR\n  ui\n  api\n  ui --> api\n{maps}");
        repo(&[
            (CONTRACT, &contract),
            (
                "src/ui/app.ts",
                "import { call } from '../api/client';\nexport const app = call;\n",
            ),
            ("src/api/client.ts", "export const call = 1;\n"),
            ("src/api/data.json", "{}\n"),
        ])
    }
}

use support::{CONTRACT, driftwall, error, text, two_nodes};

const MAPS: &str = "%% map ui: src/ui/\n%% map api: src/api/\n";

#[test]
fn a_contract_in_docs_maps_the_code_with_root_relative_globs() {
    let dir = two_nodes(MAPS);
    let output = driftwall(dir.path(), &["lint", "--strict", "--contract", CONTRACT]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("clean"), "{}", text(&output));
}

#[test]
fn an_unmapped_file_is_reported_as_without_a_map() {
    let dir = two_nodes("%% map ui: src/ui/\n");
    let output = driftwall(dir.path(), &["lint", "--contract", CONTRACT]);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(
        text(&output).contains("src/api/client.ts:1: error[unmapped-file]"),
        "{}",
        text(&output)
    );
}

#[test]
fn a_map_to_an_undeclared_node_is_a_contract_error_with_its_line() {
    let dir = two_nodes("%% map ghost: src/\n");
    let output = driftwall(dir.path(), &["lint", "--contract", CONTRACT]);
    assert_eq!(output.status.code(), Some(2));
    let message = error(&output);
    assert!(
        message.contains("line 5") && message.contains("ghost"),
        "{message}"
    );
}

#[test]
fn a_malformed_map_line_is_a_contract_error_not_a_comment() {
    for bad in [
        "%% map of the system\n",
        "%% map ui: \n",
        "%% map ui: !src/\n",
    ] {
        let dir = two_nodes(bad);
        let output = driftwall(dir.path(), &["lint", "--contract", CONTRACT]);
        assert_eq!(output.status.code(), Some(2), "{bad}");
        assert!(
            error(&output).contains("line 5"),
            "{bad}: {}",
            error(&output)
        );
    }
}

#[test]
fn a_tie_between_different_nodes_is_a_contract_error_naming_both_lines_and_a_file() {
    let dir = two_nodes("%% map ui: src/*/*.ts\n%% map api: src/*/c*.ts, src/ui/\n");
    // why: src/api/client.ts matches both `src/*/*.ts` and `src/*/c*.ts` at specificity (1, 4) and (1, 4).
    let output = driftwall(dir.path(), &["lint", "--contract", CONTRACT]);
    assert_eq!(output.status.code(), Some(2), "{}", text(&output));
    let message = error(&output);
    assert!(
        message.contains("architecture.mmd:5")
            && message.contains("architecture.mmd:6")
            && message.contains("src/api/client.ts"),
        "{message}"
    );
}

#[test]
fn a_glob_that_matches_nothing_is_a_stale_map_warning_that_strict_fails() {
    let dir = two_nodes(&format!("{MAPS}%% map api: gone/\n"));
    let relaxed = driftwall(dir.path(), &["lint", "--contract", CONTRACT]);
    assert_eq!(relaxed.status.code(), Some(0), "{}", text(&relaxed));
    assert!(
        text(&relaxed).contains("docs/architecture.mmd:7: warning[stale-map]"),
        "{}",
        text(&relaxed)
    );
    let strict = driftwall(dir.path(), &["lint", "--strict", "--contract", CONTRACT]);
    assert_eq!(strict.status.code(), Some(1));
}

#[test]
fn arch_files_are_not_read_and_are_reported_once() {
    let dir = two_nodes(MAPS);
    for (path, body) in [
        ("src/ui/.arch", "node: nope\n"),
        ("src/api/.arch", "this is not valid\n"),
    ] {
        std::fs::write(dir.path().join(path), body).unwrap();
    }
    let output = driftwall(
        dir.path(),
        &["lint", "--contract", CONTRACT, "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let found: Vec<&serde_json::Value> = json["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["kind"] == "ignored-tag-files")
        .collect();
    assert_eq!(found.len(), 1, "{json}");
    let message = found[0]["message"].as_str().unwrap();
    assert!(
        message.contains("2 `.arch` files") && message.contains("src/api/.arch"),
        "{message}"
    );
    assert_eq!(found[0]["severity"], "warning");
}

#[test]
fn a_contract_without_map_lines_still_reads_arch_files() {
    let dir = two_nodes("");
    std::fs::write(dir.path().join("src/ui/.arch"), "node: ui\n").unwrap();
    std::fs::write(dir.path().join("src/api/.arch"), "node: api\n").unwrap();
    let output = driftwall(dir.path(), &["lint", "--strict", "--contract", CONTRACT]);
    assert!(output.status.success(), "{}", text(&output));
}

#[test]
fn where_names_the_contract_line_and_glob() {
    let dir = two_nodes(MAPS);
    let output = driftwall(
        dir.path(),
        &["where", "src/ui/app.ts", "--contract", CONTRACT],
    );
    assert!(output.status.success());
    assert_eq!(
        text(&output),
        "src/ui/app.ts: ui (via docs/architecture.mmd:5 (map src/ui/))\n"
    );
    let json = driftwall(
        dir.path(),
        &[
            "where",
            "src/api/data.json",
            "--contract",
            CONTRACT,
            "--format",
            "json",
        ],
    );
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["node"], "api");
    assert_eq!(value["arch_file"], CONTRACT);
    assert_eq!(value["map"]["line"], 6);
    assert_eq!(value["map"]["glob"], "src/api/");
}

#[test]
fn where_finds_maps_in_the_default_contract_without_the_flag() {
    let dir = support_default_contract();
    let output = driftwall(dir.path(), &["where", "src/a.ts"]);
    assert_eq!(
        text(&output),
        "src/a.ts: a (via architecture.mmd:3 (map src/))\n"
    );
}

fn support_default_contract() -> tempfile::TempDir {
    support::repo(&[
        ("architecture.mmd", "flowchart LR\n  a\n%% map a: src/\n"),
        ("src/a.ts", "export const a = 1;\n"),
    ])
}

#[test]
fn brief_shows_the_globs_as_the_dirs_of_a_node() {
    let dir = two_nodes("%% map ui: src/ui/, src/ui-extra/*.ts\n%% map api: src/api/\n");
    let output = driftwall(dir.path(), &["brief", "--contract", CONTRACT]);
    let shown = text(&output);
    assert!(shown.contains("ui: src/ui/, src/ui-extra/*.ts"), "{shown}");
    assert!(shown.contains("api: src/api/"), "{shown}");
    let neighbors = driftwall(dir.path(), &["neighbors", "ui", "--contract", CONTRACT]);
    assert!(text(&neighbors).contains("src/ui/"), "{}", text(&neighbors));
}

#[test]
fn coarse_maps_report_the_edges_and_violations_the_code_has() {
    let dir = support::repo(&[
        (
            CONTRACT,
            "flowchart LR\n  app\n  core\n  app --> core\n%% map app: src/app/, src/main.ts\n%% map core: src/\n",
        ),
        ("src/main.ts", "import { run } from './app/run';\nrun();\n"),
        (
            "src/app/run.ts",
            "import { x } from '../core/x';\nexport const run = x;\n",
        ),
        (
            "src/core/x.ts",
            "import { run } from '../app/run';\nexport const x = run;\n",
        ),
    ]);
    let output = driftwall(dir.path(), &["lint", "--contract", CONTRACT]);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    // why: src/core/x.ts imports app, and the contract has only app --> core.
    assert!(
        text(&output).contains("src/core/x.ts:1: error[undeclared-edge]"),
        "{}",
        text(&output)
    );
}

#[test]
fn module_form_maps_every_module_to_its_own_node() {
    let dir = support::repo(&[
        (
            CONTRACT,
            "flowchart LR\n  m-a\n  m-b\n  m-b --> m-a\n%% map m-*: src/\n",
        ),
        ("src/a.ts", "export const a = 1;\n"),
        (
            "src/b.ts",
            "import { a } from './a';\nexport const b = a;\n",
        ),
    ]);
    let output = driftwall(dir.path(), &["lint", "--strict", "--contract", CONTRACT]);
    assert!(output.status.success(), "{}", text(&output));
    let where_ = driftwall(dir.path(), &["where", "src/b.ts", "--contract", CONTRACT]);
    assert_eq!(
        text(&where_),
        "src/b.ts: m-b (via docs/architecture.mmd:5 (map src/))\n"
    );
}

#[test]
fn a_module_derived_node_the_contract_lacks_is_an_unknown_node_at_the_map_line() {
    let dir = support::repo(&[
        (CONTRACT, "flowchart LR\n  m-a\n%% map m-*: src/\n"),
        ("src/a.ts", "export const a = 1;\n"),
        ("src/b.ts", "export const b = 1;\n"),
    ]);
    let output = driftwall(dir.path(), &["lint", "--contract", CONTRACT]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output).contains("docs/architecture.mmd:3: error[unknown-node]"),
        "{}",
        text(&output)
    );
}

/// Fixtures `init` runs on, covering a unit split, module nodes, a group and loose files.
fn fixtures() -> Vec<Vec<(&'static str, &'static str)>> {
    vec![
        vec![
            ("src/index.ts", "import './a';\nimport './b';\n"),
            (
                "src/a.ts",
                "import { b } from './b';\nexport const a = b;\n",
            ),
            ("src/b.ts", "export const b = 1;\n"),
            (
                "src/util/x.ts",
                "import { b } from '../b';\nexport const x = b;\n",
            ),
            ("src/b.json", "{}\n"),
            (
                "tools/gen.ts",
                "import { a } from '../src/a';\nexport const g = a;\n",
            ),
            ("main.ts", "import './src/index';\n"),
        ],
        vec![
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "src/main.rs",
                "mod lint;\nmod model;\nfn main() { lint::run(); }\n",
            ),
            (
                "src/lint.rs",
                "use crate::model::M;\npub fn run() { let _ = M; }\n",
            ),
            ("src/model.rs", "pub struct M;\n"),
            ("ui/package.json", "{}\n"),
            (
                "ui/src/app.ts",
                "import { s } from './store';\nexport const a = s;\n",
            ),
            ("ui/src/store.ts", "export const s = 1;\n"),
            ("ui/src/index.css", "body {}\n"),
        ],
    ]
}

#[test]
fn init_map_lints_exactly_like_init_with_arch_files() {
    for files in fixtures() {
        let tagged = support::repo(&files);
        let mapped = support::repo(&files);
        // why: both contracts sit in docs/, so the contract file itself is the same unowned asset.
        let plain = driftwall(tagged.path(), &["init", "--contract", CONTRACT]);
        assert!(plain.status.success(), "{}{}", text(&plain), error(&plain));
        let init = driftwall(mapped.path(), &["init", "--map", "--contract", CONTRACT]);
        assert!(init.status.success(), "{}{}", text(&init), error(&init));
        assert!(!mapped.path().join("src/.arch").exists());
        let has_arch = support::walk_arch(mapped.path());
        assert!(has_arch.is_empty(), "{has_arch:?}");

        let lint = |dir: &tempfile::TempDir, contract: &str| {
            let output = driftwall(
                dir.path(),
                &[
                    "lint",
                    "--strict",
                    "--format",
                    "json",
                    "--contract",
                    contract,
                ],
            );
            assert_eq!(output.status.code(), Some(0), "{}", text(&output));
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        };
        assert_eq!(lint(&tagged, CONTRACT), lint(&mapped, CONTRACT));

        let shape = |dir: &tempfile::TempDir, contract: &str| {
            let output = driftwall(
                dir.path(),
                &["brief", "--format", "json", "--contract", contract],
            );
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let nodes: Vec<(String, u64, u64)> = json["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| {
                    (
                        n["id"].as_str().unwrap().to_owned(),
                        n["files"].as_u64().unwrap(),
                        n["assets"].as_u64().unwrap(),
                    )
                })
                .collect();
            (nodes, json["edges"].clone())
        };
        assert_eq!(shape(&tagged, CONTRACT), shape(&mapped, CONTRACT));
    }
}

#[test]
fn init_map_refuses_no_tags_and_an_existing_contract_without_force() {
    let dir = support::repo(&[("src/a.ts", "export const a = 1;\n"), ("src/b.ts", "")]);
    let both = driftwall(dir.path(), &["init", "--map", "--no-tags"]);
    assert_eq!(both.status.code(), Some(2));
    assert!(driftwall(dir.path(), &["init", "--map"]).status.success());
    let again = driftwall(dir.path(), &["init", "--map"]);
    assert_eq!(again.status.code(), Some(2));
    assert!(error(&again).contains("--force"), "{}", error(&again));
    assert!(
        driftwall(dir.path(), &["init", "--map", "--force"])
            .status
            .success()
    );
}

#[test]
fn init_map_turns_existing_arch_files_into_map_lines() {
    let dir = support::repo(&[
        ("src/.arch", "node: core\n"),
        ("src/a.ts", "export const a = 1;\n"),
        (
            "lib/b.ts",
            "import { a } from '../src/a';\nexport const b = a;\n",
        ),
    ]);
    let init = driftwall(dir.path(), &["init", "--map"]);
    assert!(init.status.success(), "{}{}", text(&init), error(&init));
    let contract = std::fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(contract.contains("%% map core: src/"), "{contract}");
    let lint = driftwall(dir.path(), &["lint"]);
    assert_eq!(lint.status.code(), Some(0), "{}", text(&lint));
    assert!(
        text(&lint).contains("warning[ignored-tag-files]"),
        "{}",
        text(&lint)
    );
}

#[test]
fn diff_reads_a_contract_in_docs_at_a_ref() {
    let dir = two_nodes(MAPS);
    let git = |args: &[&str]| {
        let output = support::clean(std::process::Command::new("git"))
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {}", error(&output));
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "x"]);
    let edited = std::fs::read_to_string(dir.path().join(CONTRACT))
        .unwrap()
        .replace("  ui --> api\n", "  ui --> api\n  api --> ui\n");
    std::fs::write(dir.path().join(CONTRACT), edited).unwrap();
    let output = driftwall(
        dir.path(),
        &[
            "diff",
            "--base",
            "HEAD",
            "--contract",
            CONTRACT,
            "--exit-code",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}{}",
        text(&output),
        error(&output)
    );
    assert!(text(&output).contains("api --> ui"), "{}", text(&output));
}
