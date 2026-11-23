//! Package rules (`%% packages`, `%% forbid-packages`) through the CLI: `lint` enforces them.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use serde_json::Value;
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

    pub fn run(command: &str, dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .arg(command)
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap()
    }

    pub fn run_in(dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap()
    }

    pub fn text(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    const CONTRACT: &str =
        "flowchart LR\n  web\n  core\n  %% map web: web/\n  %% map core: core/\n";

    pub fn files(rules: &str) -> Vec<(&'static str, String)> {
        vec![
            ("architecture.mmd", format!("{CONTRACT}{rules}")),
            (
                "package.json",
                r#"{"dependencies": {"lit": "1", "three": "1", "zod": "1"}}"#.to_owned(),
            ),
            (
                "web/a.ts",
                "import { html } from 'lit';\nimport * as T from 'three';\nimport fs from 'node:fs';\n"
                    .to_owned(),
            ),
            ("web/a.test.ts", "import { x } from 'vitest';\n".to_owned()),
            (
                "core/b.ts",
                "import { z } from 'zod';\nimport { spawn } from 'node:child_process';\nimport { html } from 'lit';\n"
                    .to_owned(),
            ),
        ]
    }

    pub fn repo_with(rules: &str) -> TempDir {
        let owned = files(rules);
        let borrowed: Vec<(&str, &str)> = owned.iter().map(|(p, c)| (*p, c.as_str())).collect();
        repo(&borrowed)
    }

    pub fn findings(dir: &TempDir, extra: &[&str]) -> (Option<i32>, Vec<Value>) {
        let mut args = vec!["--format", "json"];
        args.extend_from_slice(extra);
        let out = run("lint", dir, &args);
        let json: Value = serde_json::from_str(&text(&out)).unwrap();
        (
            out.status.code(),
            json["violations"].as_array().unwrap().clone(),
        )
    }

    pub fn pick(found: &[Value]) -> Vec<(String, String, u64)> {
        found
            .iter()
            .map(|v| {
                (
                    v["kind"].as_str().unwrap().to_owned(),
                    v["at"]["file"].as_str().unwrap_or_default().to_owned(),
                    v["at"]["line"].as_u64().unwrap_or_default(),
                )
            })
            .collect()
    }
}

use serde_json::Value;
use support::{findings, pick, repo, repo_with, run, run_in, stderr, text};

#[test]
fn without_package_rules_nothing_changes() {
    let dir = repo_with("");
    let (code, found) = findings(&dir, &[]);
    assert_eq!(code, Some(0));
    assert_eq!(found, Vec::<Value>::new());
}

#[test]
fn an_allow_list_and_a_deny_list_fail_the_lint_at_the_import_line() {
    let dir =
        repo_with("  %% packages web: lit\n  %% forbid-packages core: node:child_process, lit\n");
    let (code, found) = findings(&dir, &[]);
    assert_eq!(code, Some(1));
    assert_eq!(
        pick(&found),
        [
            ("forbidden-package".to_owned(), "core/b.ts".to_owned(), 2),
            ("forbidden-package".to_owned(), "core/b.ts".to_owned(), 3),
            ("forbidden-package".to_owned(), "web/a.ts".to_owned(), 2),
        ]
    );
    assert_eq!(found[0]["severity"], "error");
    let human = run("lint", &dir, &[]);
    let human = text(&human);
    assert!(human.contains("error[forbidden-package]"), "{human}");
    assert!(human.contains("core/b.ts:2"), "{human}");
}

#[test]
fn a_stale_entry_is_a_warning_that_strict_turns_into_a_failure() {
    let dir = repo_with("  %% packages web: lit, three, redis\n");
    let (code, found) = findings(&dir, &[]);
    assert_eq!(code, Some(0));
    assert_eq!(
        pick(&found),
        [("stale-package".to_owned(), "architecture.mmd".to_owned(), 6)]
    );
    let (strict, _) = findings(&dir, &["--strict"]);
    assert_eq!(strict, Some(1));
}

#[test]
fn an_unknown_node_is_a_contract_error_with_its_line() {
    let dir = repo_with("  %% packages ghost: lit\n");
    let out = run("lint", &dir, &[]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("line 6"), "{err}");
    assert!(err.contains("ghost"), "{err}");
}

#[test]
fn test_files_are_exempt() {
    // why: web/a.test.ts imports vitest, which the allow-list never names.
    let dir = repo_with("  %% packages web: lit, three\n");
    let (code, found) = findings(&dir, &[]);
    assert_eq!(code, Some(0));
    assert_eq!(found, Vec::<Value>::new());
}

#[test]
fn brief_shows_each_nodes_packages_third_party_first_with_counts() {
    let dir = repo_with("");
    let out = text(&run("brief", &dir, &[]));
    assert!(
        out.contains("  web: web/ (2 files) packages: lit (third-party, 1), three (third-party, 1), vitest (third-party, 1, 1 test), node:fs (stdlib, 1) blind: 1 skipped\n"),
        "{out}"
    );
    assert!(
        out.contains("  core: core/ (1 file) packages: lit (third-party, 1), zod (third-party, 1), node:child_process (stdlib, 1)\n"),
        "{out}"
    );
}

#[test]
fn brief_json_lists_name_kind_count_and_test_uses() {
    let dir = repo_with("");
    let json: Value =
        serde_json::from_str(&text(&run("brief", &dir, &["--format", "json"]))).unwrap();
    let core = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "core")
        .unwrap();
    assert_eq!(
        core["packages"],
        serde_json::json!([
            {"kind": "third-party", "name": "lit", "count": 1, "tests": 0},
            {"kind": "third-party", "name": "zod", "count": 1, "tests": 0},
            {"kind": "stdlib", "name": "node:child_process", "count": 1, "tests": 0},
        ])
    );
}

#[test]
fn a_node_that_uses_no_package_has_no_packages_key_and_no_packages_text() {
    let dir = repo(&[
        ("architecture.mmd", "flowchart LR\n  a\n  %% map a: a/\n"),
        ("a/x.ts", "export const x = 1;\n"),
    ]);
    let brief = text(&run("brief", &dir, &[]));
    assert!(!brief.contains("packages"), "{brief}");
    let json = text(&run("brief", &dir, &["--format", "json"]));
    assert!(!json.contains("packages"), "{json}");
}

#[test]
fn neighbors_lists_the_packages_of_the_node_and_in_json() {
    let dir = repo_with("");
    let out = text(&run_in(&dir, &["neighbors", "web"]));
    assert!(
        out.contains("packages:\n  lit (third-party, 1)\n  three (third-party, 1)\n  vitest (third-party, 1, 1 test)\n  node:fs (stdlib, 1)\n"),
        "{out}"
    );
    let json: Value = serde_json::from_str(&text(&run_in(
        &dir,
        &["neighbors", "web", "--format", "json"],
    )))
    .unwrap();
    assert_eq!(json["node"]["packages"].as_array().unwrap().len(), 4);
}

#[test]
fn where_lists_the_package_uses_of_a_source_file() {
    let dir = repo_with("");
    let out = text(&run_in(&dir, &["where", "web/a.ts"]));
    assert_eq!(
        out,
        "web/a.ts: web (via architecture.mmd:4 (map web/))\n  package lit (third-party) line 1\n  package three (third-party) line 2\n  package node:fs (stdlib) line 3\n"
    );
    let test = text(&run_in(&dir, &["where", "web/a.test.ts"]));
    assert!(
        test.ends_with("  package vitest (third-party) line 1 (test)\n"),
        "{test}"
    );
    let json: Value = serde_json::from_str(&text(&run_in(
        &dir,
        &["where", "web/a.ts", "--format", "json"],
    )))
    .unwrap();
    assert_eq!(
        json["packages"][1],
        serde_json::json!({"line": 2, "name": "three", "kind": "third-party", "test_only": false})
    );
}

#[test]
fn where_adds_nothing_for_a_file_without_uses_and_for_a_directory() {
    let dir = repo_with("");
    let json = text(&run_in(&dir, &["where", "web", "--format", "json"]));
    assert!(!json.contains("packages"), "{json}");
    let none = repo(&[
        ("architecture.mmd", "flowchart LR\n  a\n  %% map a: a/\n"),
        ("a/x.ts", "export const x = 1;\n"),
    ]);
    assert_eq!(
        text(&run_in(&none, &["where", "a/x.ts"])),
        "a/x.ts: a (via architecture.mmd:3 (map a/))\n"
    );
}

#[test]
fn init_drafts_observed_third_party_packages_as_plain_comments_that_lint_ignores() {
    let dir = repo(&[
        (
            "package.json",
            r#"{"dependencies": {"lit": "1", "three": "1"}}"#,
        ),
        ("a/.arch", "node: a\n"),
        ("b/.arch", "node: b\n"),
        (
            "a/x.ts",
            "import { html } from 'lit';\nimport fs from 'node:fs';\nimport { y } from '../b/y';\n",
        ),
        ("a/x.test.ts", "import { it } from 'vitest';\n"),
        (
            "b/y.ts",
            "import * as T from 'three';\nexport const y = 1;\n",
        ),
    ]);
    let init = run("init", &dir, &[]);
    assert!(init.status.success(), "{init:?}");
    let contract = std::fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(
        contract.contains("  %% observed packages a: lit\n  %% observed packages b: three\n"),
        "{contract}"
    );
    assert!(!contract.contains("vitest"), "{contract}");
    assert!(!contract.contains("node:fs"), "{contract}");
    assert!(contract.contains("not enforced"), "{contract}");
    let strict = run("lint", &dir, &["--strict"]);
    assert!(strict.status.success(), "{strict:?}");
}

#[test]
fn init_writes_no_observed_line_for_a_repository_without_third_party_packages() {
    let dir = repo(&[
        ("a/.arch", "node: a\n"),
        ("a/x.ts", "import fs from 'node:fs';\n"),
    ]);
    let init = run("init", &dir, &[]);
    assert!(init.status.success(), "{init:?}");
    let contract = std::fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(!contract.contains("observed"), "{contract}");
}
