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
use support::{findings, pick, repo_with, run, stderr, text};

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
