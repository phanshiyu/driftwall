//! End-to-end tests of public surfaces (`%% exports`) through the `driftwall` binary: the
//! `private-symbol` rule on TypeScript and Rust repositories, and symbols and surfaces in `brief`,
//! `neighbors` and `diff`.

use serde_json::{Value, json};

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    /// Writes `files` (relative path, contents) into a fresh temp directory.
    pub fn repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        write(dir.path(), files);
        dir
    }

    /// Writes `files` (relative path, contents) under `root`, replacing what is there.
    pub fn write(root: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
    }

    /// A command with every `GIT_*` variable removed, so a hook that runs the tests cannot point
    /// git at the repository being committed.
    fn clean(mut command: Command) -> Command {
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
    }

    /// Runs the binary with `args` on the repository at `root`, as its last argument.
    pub fn driftwall(args: &[&str], root: &Path) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(args)
            .arg(root)
            .output()
            .unwrap()
    }

    /// Runs `driftwall diff --base HEAD <extra> <root>`.
    pub fn diff(root: &Path, extra: &[&str]) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(["diff", "--base", "HEAD"])
            .args(extra)
            .arg(root)
            .output()
            .unwrap()
    }

    /// Runs git in `dir` with a fixed identity and no signing or hooks.
    fn git(dir: &Path, args: &[&str]) {
        let output = clean(Command::new("git"))
            .current_dir(dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Initialises a repository in `dir` and commits everything in it.
    pub fn commit_all(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "base"]);
    }

    /// The violations of `driftwall lint --format json` on `root`, with the exit code.
    pub fn lint_json(root: &Path) -> (i32, Vec<serde_json::Value>) {
        let output = driftwall(&["lint", "--format", "json"], root);
        let value: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
        (
            code(&output),
            value["violations"].as_array().unwrap().clone(),
        )
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }
}

use support::{code, commit_all, diff, driftwall, lint_json, repo, stdout, write};

/// `a` imports `b`; `b` declares a surface.
const SURFACE_CONTRACT: &str =
    "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n  %% exports b: Client, fetch\n";

const TS_LIB: &str =
    "export const Client = 1;\nexport const fetch = 2;\nexport const secret = 3;\n";

fn ts_files(contract: &'static str, main: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", contract),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", main),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", TS_LIB),
    ]
}

#[test]
fn brief_lists_the_symbols_each_edge_uses_and_marks_a_surface() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch, Client } from '../b/lib';\n",
    ));
    let output = driftwall(&["brief"], dir.path());
    assert_eq!(code(&output), 0);
    let expected = "\
driftwall: 2 nodes, 1 edge (1 implemented, 0 planned), 0 errors, 0 warnings
nodes:
  a: a (1 file)
  b: b (1 file) exports: Client, fetch
edges:
  a --> b (1 import) uses: Client, fetch
blind spots: none
";
    assert_eq!(stdout(&output), expected);
}

#[test]
fn brief_shows_six_symbols_then_counts_the_rest() {
    let dir = repo(&ts_files(
        "flowchart LR\n  a\n  b\n  a --> b\n",
        "import { s1, s2, s3, s4, s5, s6, s7, s8 } from '../b/lib';\n",
    ));
    let output = driftwall(&["brief"], dir.path());
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).contains("  a --> b (1 import) uses: s1, s2, s3, s4, s5, s6 +2 more\n"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn brief_json_carries_symbols_and_exports() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch, Client } from '../b/lib';\nimport * as all from '../b/lib';\n",
    ));
    let output = driftwall(&["brief", "--format", "json"], dir.path());
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(value["nodes"][0].get("exports").is_none());
    assert_eq!(value["nodes"][1]["exports"], json!(["Client", "fetch"]));
    assert_eq!(
        value["edges"][0]["symbols"],
        json!(["Client", "fetch", "*"])
    );
}

#[test]
fn neighbors_lists_symbols_per_edge_and_the_surface() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch } from '../b/lib';\n",
    ));
    let output = driftwall(&["neighbors", "b"], dir.path());
    assert_eq!(code(&output), 0);
    let expected = "\
node: b
label: b
dirs: b
files: 1
exports: Client, fetch
outgoing:
  (none)
incoming:
  a --> b (1 import) uses: fetch
    a/main.ts:1
blind spots: none
";
    assert_eq!(stdout(&output), expected);
}

#[test]
fn neighbors_json_carries_symbols_and_exports() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch } from '../b/lib';\n",
    ));
    let output = driftwall(&["neighbors", "b", "--format", "json"], dir.path());
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["node"]["exports"], json!(["Client", "fetch"]));
    assert_eq!(value["incoming"][0]["symbols"], json!(["fetch"]));
}

#[test]
fn a_side_effect_import_is_a_module_use_and_lists_no_symbols() {
    let dir = repo(&ts_files(SURFACE_CONTRACT, "import '../b/lib';\n"));
    let output = driftwall(&["brief"], dir.path());
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).contains("  a --> b (1 import) uses: module\n"),
        "{}",
        stdout(&output)
    );
}

const BEFORE: &str =
    "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n  %% exports b: Client, fetch\n";

#[test]
fn diff_reports_surface_changes_as_plus_and_minus_lines() {
    let dir = repo(&ts_files(BEFORE, "import { fetch } from '../b/lib';\n"));
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n  %% exports b: fetch, secret\n",
        )],
    );
    let output = diff(dir.path(), &[]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "+ exports b: secret\n- exports b: Client\n"
    );
    let gated = diff(dir.path(), &["--exit-code"]);
    assert_eq!(code(&gated), 1);
}

#[test]
fn diff_reports_a_new_and_a_dropped_surface_and_json_lists_them() {
    let dir = repo(&ts_files(BEFORE, "import { fetch } from '../b/lib';\n"));
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n  %% exports a: main\n",
        )],
    );
    let output = diff(dir.path(), &[]);
    assert_eq!(
        stdout(&output),
        "+ exports a: main\n- exports b: Client\n- exports b: fetch\n"
    );
    let output = diff(dir.path(), &["--format", "json"]);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        value["added_exports"],
        json!([{ "node": "a", "symbol": "main" }])
    );
    assert_eq!(
        value["removed_exports"],
        json!([
            { "node": "b", "symbol": "Client" },
            { "node": "b", "symbol": "fetch" }
        ])
    );
}

#[test]
fn diff_without_surface_changes_stays_quiet_about_exports() {
    let dir = repo(&ts_files(BEFORE, "import { fetch } from '../b/lib';\n"));
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  b[\"b\"]\n  a[\"a\"]\n  a --> b\n  %% exports b: fetch, Client\n",
        )],
    );
    let output = diff(dir.path(), &["--exit-code"]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "driftwall: no contract changes\n");
}

#[test]
fn typescript_private_symbol_fails_the_lint_at_the_import_and_passes_once_fixed() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { Client } from '../b/lib';\nimport { secret } from '../b/lib';\n",
    ));
    let (exit, violations) = lint_json(dir.path());
    assert_eq!(exit, 1);
    assert_eq!(violations.len(), 1, "{violations:?}");
    let found = &violations[0];
    assert_eq!(found["kind"], json!("private-symbol"));
    assert_eq!(found["severity"], json!("error"));
    assert_eq!(found["at"], json!({ "file": "a/main.ts", "line": 2 }));
    let message = found["message"].as_str().unwrap();
    assert!(
        message.contains("`secret`") && message.contains("`a`") && message.contains("`b`"),
        "{message}"
    );
    assert!(message.contains("Client, fetch"), "{message}");
    assert!(found["hint"].as_str().unwrap().contains("approve"));

    let text = driftwall(&["lint"], dir.path());
    assert_eq!(code(&text), 1);
    assert!(
        stdout(&text).contains("a/main.ts:2: error[private-symbol]: "),
        "{}",
        stdout(&text)
    );

    write(
        dir.path(),
        &[(
            "a/main.ts",
            "import { Client } from '../b/lib';\nimport { fetch } from '../b/lib';\n",
        )],
    );
    let (exit, violations) = lint_json(dir.path());
    assert_eq!((exit, violations), (0, Vec::new()));
}

#[test]
fn typescript_namespace_and_default_imports_need_the_surface_to_allow_them() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import * as lib from '../b/lib';\nimport def from '../b/lib';\n",
    ));
    let (exit, violations) = lint_json(dir.path());
    assert_eq!(exit, 1);
    let lines: Vec<(&str, i64)> = violations
        .iter()
        .map(|v| {
            (
                v["kind"].as_str().unwrap(),
                v["at"]["line"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(lines, [("private-symbol", 1), ("private-symbol", 2)]);
    assert!(violations[0]["message"].as_str().unwrap().contains("`*`"));
    assert!(
        violations[1]["message"]
            .as_str()
            .unwrap()
            .contains("`default`")
    );
}

#[test]
fn typescript_side_effect_imports_and_unrestricted_nodes_pass() {
    let dir = repo(&ts_files(SURFACE_CONTRACT, "import '../b/lib';\n"));
    assert_eq!(lint_json(dir.path()), (0, Vec::new()));

    let dir = repo(&ts_files(
        "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n",
        "import * as lib from '../b/lib';\nimport { secret } from '../b/lib';\n",
    ));
    assert_eq!(lint_json(dir.path()), (0, Vec::new()));
}

#[test]
fn typescript_imports_inside_one_node_are_never_private() {
    let mut files = ts_files(SURFACE_CONTRACT, "import { Client } from '../b/lib';\n");
    files.push(("b/more.ts", "import { secret } from './lib';\n"));
    let dir = repo(&files);
    assert_eq!(lint_json(dir.path()), (0, Vec::new()));
}

#[test]
fn a_surface_naming_an_undeclared_node_is_a_tool_error_with_its_line() {
    let dir = repo(&ts_files(
        "flowchart LR\n  a\n  b\n  a --> b\n  %% exports ghost: X\n",
        "import { Client } from '../b/lib';\n",
    ));
    let output = driftwall(&["lint"], dir.path());
    assert_eq!(code(&output), 2);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("line 5"), "{stderr}");
    assert!(stderr.contains("ghost"), "{stderr}");
}

const RUST_CONTRACT: &str =
    "flowchart LR\n  lib\n  app\n  gitlab\n  app --> gitlab\n  %% exports gitlab: Client, fetch\n";

const RUST_GITLAB: &str = "pub struct Client;\npub fn fetch() {}\npub fn secret() {}\n";

fn rust_files(app: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", RUST_CONTRACT),
        ("src/.arch", "node: *\n"),
        ("src/lib.rs", "pub mod app;\npub mod gitlab;\n"),
        ("src/app.rs", app),
        ("src/gitlab.rs", RUST_GITLAB),
    ]
}

#[test]
fn rust_private_symbol_fails_the_lint_at_the_use_and_passes_once_fixed() {
    let dir = repo(&rust_files(
        "use crate::gitlab::Client;\nuse crate::gitlab::secret;\n",
    ));
    let (exit, violations) = lint_json(dir.path());
    assert_eq!(exit, 1);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0]["kind"], json!("private-symbol"));
    assert_eq!(
        violations[0]["at"],
        json!({ "file": "src/app.rs", "line": 2 })
    );
    assert!(
        violations[0]["message"]
            .as_str()
            .unwrap()
            .contains("`secret`")
    );

    write(
        dir.path(),
        &[(
            "src/app.rs",
            "use crate::gitlab::Client;\nuse crate::gitlab::fetch;\n",
        )],
    );
    assert_eq!(lint_json(dir.path()), (0, Vec::new()));
}

#[test]
fn rust_qualified_paths_in_code_are_checked_too() {
    let dir = repo(&rust_files(
        "pub fn f() {\n    crate::gitlab::fetch();\n    crate::gitlab::secret();\n}\n",
    ));
    let (exit, violations) = lint_json(dir.path());
    assert_eq!(exit, 1);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(
        violations[0]["at"],
        json!({ "file": "src/app.rs", "line": 3 })
    );
}

#[test]
fn rust_module_name_use_is_allowed_and_the_items_used_through_it_are_checked() {
    let dir = repo(&rust_files(
        "use crate::gitlab;\npub fn f() {\n    gitlab::fetch();\n    gitlab::secret();\n}\n",
    ));
    let (exit, violations) = lint_json(dir.path());
    assert_eq!(exit, 1);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(
        violations[0]["at"],
        json!({ "file": "src/app.rs", "line": 4 })
    );
    assert!(
        violations[0]["message"]
            .as_str()
            .unwrap()
            .contains("`secret`")
    );

    write(
        dir.path(),
        &[(
            "src/app.rs",
            "use crate::gitlab;\npub fn f() {\n    gitlab::fetch();\n    let _ = gitlab::Client;\n}\n",
        )],
    );
    assert_eq!(lint_json(dir.path()), (0, Vec::new()));
}

#[test]
fn rust_aliased_and_self_module_bindings_are_not_private_but_their_items_are_checked() {
    let dir = repo(&rust_files(
        "use crate::gitlab as gl;\nuse crate::gitlab::{self};\npub fn f() {\n    gl::fetch();\n    gl::secret();\n}\n",
    ));
    let (exit, violations) = lint_json(dir.path());
    assert_eq!(exit, 1);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(
        violations[0]["at"],
        json!({ "file": "src/app.rs", "line": 5 })
    );
    assert!(
        violations[0]["message"]
            .as_str()
            .unwrap()
            .contains("`secret`")
    );
}

#[test]
fn rust_glob_import_is_private() {
    let dir = repo(&rust_files("use crate::gitlab::*;\n"));
    let (exit, violations) = lint_json(dir.path());
    assert_eq!(exit, 1);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(
        violations[0]["at"],
        json!({ "file": "src/app.rs", "line": 1 })
    );
    assert!(violations[0]["message"].as_str().unwrap().contains("`*`"));
}

#[test]
fn test_code_may_use_names_outside_a_surface() {
    let dir = repo(&[
        ("architecture.mmd", SURFACE_CONTRACT),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", "import { Client } from \"../b/lib\";\n"),
        (
            "a/main.test.ts",
            "import { secret } from \"../b/lib\";\nimport * as all from \"../b/lib\";\n",
        ),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", TS_LIB),
    ]);
    let output = driftwall(&["lint", "--strict"], dir.path());
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(!text.contains("private-symbol"), "{text}");
}
