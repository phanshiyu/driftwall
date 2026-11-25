//! End-to-end tests of `%% deny`: what `lint`, `brief` and `neighbors` say about it, and every
//! contract error.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fmt::Write as _;
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    pub const CHAIN: &str = "app > ui > shared > core > domain > infra > base";

    const LAYERS: [&str; 7] = ["app", "ui", "shared", "core", "domain", "infra", "base"];

    /// One import: `(from node, to node, file name, type-only)`.
    pub type Import<'a> = (&'a str, &'a str, &'a str, bool);

    /// Seven layers, one node and one directory each, an open chain on line 25, `extra` after it,
    /// and the given imports. `ui` also owns `ui2` and `core` also owns `core2`.
    pub fn repo(extra: &str, imports: &[Import<'_>]) -> TempDir {
        let mut contract = String::from("flowchart LR\n");
        for layer in LAYERS {
            let _ = write!(contract, "  subgraph {layer}[\"{layer}\"]\n    {layer}_n\n");
            match layer {
                "ui" => contract.push_str("    ui2\n"),
                "core" => contract.push_str("    core2\n"),
                _ => {}
            }
            contract.push_str("  end\n");
        }
        let _ = write!(contract, "  %% order open: {CHAIN}\n{extra}");
        let dir = TempDir::new().unwrap();
        let write = |path: &str, text: &str| {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, text).unwrap();
        };
        write("architecture.mmd", &contract);
        let mut nodes: Vec<String> = LAYERS.iter().map(|l| format!("{l}_n")).collect();
        nodes.extend(["ui2".to_owned(), "core2".to_owned()]);
        for node in &nodes {
            write(&format!("{node}/.arch"), &format!("node: {node}\n"));
            write(
                &format!("{node}/m.ts"),
                "export const x = 1;\nexport type T = 1;\n",
            );
        }
        for (from, to, file, type_only) in imports {
            let kind = if *type_only {
                "import type { T }"
            } else {
                "import { x }"
            };
            write(
                &format!("{from}/{file}"),
                &format!("{kind} from '../{to}/m';\n"),
            );
        }
        dir
    }

    pub fn run(dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(args)
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8(output.stderr.clone()).unwrap()
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }

    pub fn json(output: &Output) -> serde_json::Value {
        serde_json::from_str(&stdout(output)).unwrap()
    }

    pub fn kinds(value: &serde_json::Value) -> Vec<&str> {
        value["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["kind"].as_str().unwrap())
            .collect()
    }
}

use support::{code, json, kinds, repo, run, stderr, stdout};

const HINT: &str = "remove the import, or ask the user to approve removing the denial";

#[test]
fn a_denial_removes_one_layer_pair_and_leaves_the_others() {
    let dir = repo(
        "  %% deny ui: core\n",
        &[
            ("ui_n", "core_n", "m2.ts", false),
            ("ui_n", "shared_n", "m3.ts", false),
            ("app_n", "core_n", "m4.ts", false),
            ("ui_n", "domain_n", "m5.ts", false),
        ],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    assert_eq!(code(&output), 1, "{}", stdout(&output));
    let value = json(&output);
    assert_eq!(kinds(&value), ["denied-edge"], "{value}");
    let found = &value["violations"][0];
    assert_eq!(found["severity"], "error");
    assert_eq!(found["at"]["file"], "ui_n/m2.ts");
    assert_eq!(found["at"]["line"], 1);
    assert_eq!(found["hint"], HINT);
    assert_eq!(
        found["message"],
        "`ui_n` imports `core_n` (core_n/m.ts) but architecture.mmd:26 `%% deny ui: core` forbids it"
    );
    assert_eq!(found["key"], "denied-edge ui_n --> core_n");
}

#[test]
fn a_node_level_denial_removes_one_node_pair_only() {
    let dir = repo(
        "  %% deny ui_n: core_n\n",
        &[
            ("ui_n", "core_n", "m2.ts", false),
            ("ui2", "core_n", "m3.ts", false),
            ("ui_n", "core2", "m4.ts", false),
        ],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    let value = json(&output);
    assert_eq!(kinds(&value), ["denied-edge"], "{value}");
    assert_eq!(value["violations"][0]["at"]["file"], "ui_n/m2.ts");
    assert_eq!(code(&output), 1);
}

#[test]
fn a_denial_may_mix_a_node_and_a_boundary_and_name_several_targets() {
    let dir = repo(
        "  %% deny ui_n: core, base\n",
        &[
            ("ui_n", "core2", "m2.ts", false),
            ("ui_n", "base_n", "m3.ts", false),
            ("ui2", "base_n", "m4.ts", false),
        ],
    );
    let value = json(&run(&dir, &["lint", "--format", "json"]));
    assert_eq!(kinds(&value), ["denied-edge", "denied-edge"], "{value}");
    let files: Vec<&str> = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["at"]["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["ui_n/m2.ts", "ui_n/m3.ts"]);
}

#[test]
fn every_kind_of_site_on_a_denied_pair_is_denied_edge_and_nothing_else() {
    let dir = repo(
        "  %% deny ui: core\n",
        &[
            ("ui_n", "core_n", "prod.ts", false),
            ("ui_n", "core_n", "types.ts", true),
            ("ui2", "core2", "case.test.ts", false),
        ],
    );
    let value = json(&run(&dir, &["lint", "--format", "json"]));
    assert_eq!(kinds(&value), ["denied-edge"; 3], "{value}");
    let keys: Vec<&str> = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["key"].as_str().unwrap())
        .collect();
    assert!(
        keys.contains(&"denied-edge ui2 --> core2 (test)"),
        "{keys:?}"
    );
    assert_eq!(
        keys.iter()
            .filter(|key| **key == "denied-edge ui_n --> core_n")
            .count(),
        2,
        "{keys:?}"
    );
}

#[test]
fn text_json_and_sarif_agree() {
    let dir = repo(
        "  %% deny ui: core\n",
        &[("ui_n", "core_n", "m2.ts", false)],
    );
    let text = stdout(&run(&dir, &["lint"]));
    assert!(
        text.contains("ui_n/m2.ts:1: error[denied-edge]: `ui_n` imports `core_n` (core_n/m.ts) but architecture.mmd:26 `%% deny ui: core` forbids it"),
        "{text}"
    );
    assert!(text.contains(&format!("hint: {HINT}")), "{text}");
    assert!(!text.contains("undeclared-edge"), "{text}");
    assert!(text.contains("1 error"), "{text}");

    let sarif: serde_json::Value =
        serde_json::from_str(&stdout(&run(&dir, &["lint", "--format", "sarif"]))).unwrap();
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{sarif}");
    assert_eq!(results[0]["ruleId"], "denied-edge");
    assert_eq!(results[0]["level"], "error");
    let rules = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["id"], "denied-edge");
}

#[test]
fn a_group_of_denied_sites_has_one_header() {
    let dir = repo(
        "  %% deny ui: core\n",
        &[
            ("ui_n", "core_n", "a.ts", false),
            ("ui_n", "core_n", "b.ts", false),
        ],
    );
    let text = stdout(&run(&dir, &["lint"]));
    assert!(
        text.contains("error[denied-edge]: `ui_n` imports `core_n` in 2 places but architecture.mmd:26 `%% deny ui: core` forbids it"),
        "{text}"
    );
}

#[test]
fn a_baseline_records_a_denied_edge_and_suppresses_it() {
    let dir = repo(
        "  %% deny ui: core\n",
        &[
            ("ui_n", "core_n", "a.ts", false),
            ("ui_n", "core_n", "b.ts", false),
            ("ui_n", "core_n", "c.test.ts", false),
        ],
    );
    let written = run(&dir, &["lint", "--write-baseline", "debt.txt"]);
    assert_eq!(code(&written), 0, "{}", stderr(&written));
    let file = std::fs::read_to_string(dir.path().join("debt.txt")).unwrap();
    assert!(file.contains("denied-edge 2 ui_n --> core_n\n"), "{file}");
    assert!(
        file.contains("denied-edge 1 ui_n --> core_n (test)\n"),
        "{file}"
    );
    let output = run(&dir, &["lint", "--baseline", "debt.txt", "--strict"]);
    assert_eq!(code(&output), 0, "{}", stdout(&output));
    assert!(stdout(&output).starts_with("driftwall: clean, 3 baselined"));
    // why: one site more than recorded is reported in full.
    std::fs::write(
        dir.path().join("ui_n/d.ts"),
        "import { x } from '../core_n/m';\n",
    )
    .unwrap();
    let output = run(&dir, &["lint", "--baseline", "debt.txt"]);
    assert_eq!(code(&output), 1);
    assert!(
        stdout(&output).contains("the baseline allows 2, found 3"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn brief_shows_the_denial_and_marks_the_denied_pair() {
    let dir = repo(
        "  %% deny ui: core\n",
        &[
            ("ui_n", "core_n", "m2.ts", false),
            ("ui_n", "shared_n", "m3.ts", false),
        ],
    );
    let text = stdout(&run(&dir, &["brief"]));
    assert!(text.contains("  deny: ui -> core (line 26)\n"), "{text}");
    assert!(text.contains("ui_n --> core_n (1 import) DENIED"), "{text}");
    assert!(
        !text.contains("ui_n --> shared_n (1 import) UNDECLARED"),
        "{text}"
    );
    let value = json(&run(&dir, &["brief", "--format", "json"]));
    assert_eq!(
        value["rules"]["deny"],
        serde_json::json!([{"from": "ui", "to": "core", "line": 26}])
    );
    let edges = value["edges"].as_array().unwrap();
    let denied = edges.iter().find(|e| e["to"] == "core_n").unwrap();
    assert_eq!(denied["status"], "denied");
    assert!(denied["kind"].is_null());
    assert!(denied.get("via").is_none());
    let allowed = edges.iter().find(|e| e["to"] == "shared_n").unwrap();
    assert_eq!(allowed["status"], "open");
    let neighbors = stdout(&run(&dir, &["neighbors", "ui_n"]));
    assert!(neighbors.contains("DENIED"), "{neighbors}");
}

#[test]
fn without_a_denial_the_json_has_no_deny_list() {
    let dir = repo("", &[]);
    let value = json(&run(&dir, &["brief", "--format", "json"]));
    assert!(value["rules"].get("deny").is_none(), "{value}");
}

/// The tool error a contract with `extra` after its chain gives: the exit code and the message.
fn contract_error(extra: &str) -> (i32, String) {
    let dir = repo(extra, &[]);
    let output = run(&dir, &["lint"]);
    (code(&output), stderr(&output))
}

#[test]
fn every_contract_error_exits_two_on_the_line_of_the_directive() {
    for (extra, line, needle) in [
        (
            "  %% deny ghost: core\n",
            26,
            "`ghost` is not a declared node or boundary",
        ),
        (
            "  %% deny ui: ghost\n",
            26,
            "`ghost` is not a declared node or boundary",
        ),
        (
            "  %% deny ui:\n",
            26,
            "expected `%% deny <from>: <to>, <to>`",
        ),
        (
            "  %% deny ui\n",
            26,
            "expected `%% deny <from>: <to>, <to>`",
        ),
        ("  %% deny\n", 26, "expected `%% deny <from>: <to>, <to>`"),
        (
            "  %% deny ui core\n",
            26,
            "expected `%% deny <from>: <to>, <to>`",
        ),
        (
            "  %% deny ui: core, core\n",
            26,
            "already denied on line 26",
        ),
        (
            "  %% deny ui: core\n  %% deny ui: shared\n  %% deny ui: core\n",
            28,
            "already denied on line 26",
        ),
        (
            "  ui_n --> core_n\n  %% deny ui: core\n",
            27,
            "`ui_n --> core_n` on line 26",
        ),
        (
            "  ui_n -.-> core_n\n  %% deny ui: core\n",
            27,
            "`ui_n -.-> core_n` on line 26",
        ),
        (
            "  ui_n -->|test| core_n\n  %% deny ui: core\n",
            27,
            "on line 26",
        ),
        (
            "  ui_n -->|type| core_n\n  %% deny ui: core\n",
            27,
            "on line 26",
        ),
        (
            "  ui --> core_n\n  %% deny ui_n: core\n",
            27,
            "`ui --> core_n` on line 26",
        ),
        (
            "  ui_n --> core\n  %% deny ui: core2\n",
            27,
            "`ui_n --> core` on line 26",
        ),
        (
            "  %% deny core: ui\n",
            26,
            "no open chain lets `core` use `ui`",
        ),
        (
            "  %% deny ui_n: ui2\n",
            26,
            "no open chain lets `ui_n` use `ui2`",
        ),
        (
            "  ui_n --> shared_n\n  %% deny ui2: shared_n\n",
            27,
            "no open chain lets `ui2` use `shared_n`",
        ),
    ] {
        let (status, message) = contract_error(extra);
        assert_eq!(status, 2, "{extra}: {message}");
        assert!(
            message.contains(&format!("line {line}:")),
            "{extra}: {message}"
        );
        assert!(message.contains(needle), "{extra}: {message}");
    }
}

#[test]
fn a_denied_pair_that_code_does_not_use_is_not_a_finding() {
    let dir = repo(
        "  %% deny ui: core\n",
        &[("ui_n", "shared_n", "m2.ts", false)],
    );
    let output = run(&dir, &["lint", "--strict", "--format", "json"]);
    assert_eq!(code(&output), 0, "{}", stdout(&output));
    assert_eq!(kinds(&json(&output)), Vec::<&str>::new());
}

#[test]
fn a_comment_that_merely_starts_with_deny_is_not_a_directive() {
    let dir = repo("  %% denylist for old modules\n", &[]);
    let output = run(&dir, &["lint", "--strict"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
}
