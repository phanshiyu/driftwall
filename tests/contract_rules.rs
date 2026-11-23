//! End-to-end tests of the rule directives of "Contract rules": what `lint`, `brief` and
//! `neighbors` say about them.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    /// Writes `files` (relative path, contents) into a fresh temp directory.
    pub fn repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    /// Runs `driftwall <args...>` with `dir` as the last argument.
    pub fn driftwall(dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(args)
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
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

    pub fn stderr(output: &Output) -> String {
        String::from_utf8(output.stderr.clone()).unwrap()
    }
}

use support::{code, driftwall, json, kinds, repo, stderr, stdout};

/// Two boundaries, `entry` with `app` and `core` with `lib`, a declared `app --> lib` edge and the
/// given extra contract lines. `lib` imports `app` when `upward`.
fn layered(extra: &str, upward: bool) -> tempfile::TempDir {
    let contract = format!(
        "flowchart LR\n  subgraph entry[\"Entry\"]\n    app\n  end\n  subgraph core[\"Core\"]\n    lib\n  end\n  app --> lib\n{extra}"
    );
    let lib = if upward {
        "import { go } from '../app/main';\nexport const x = 1;\n"
    } else {
        "export const x = 1;\n"
    };
    repo(&[
        ("architecture.mmd", &contract),
        ("app/.arch", "node: app\n"),
        ("lib/.arch", "node: lib\n"),
        (
            "app/main.ts",
            "import { x } from '../lib/lib';\nexport const go = x;\n",
        ),
        ("lib/lib.ts", lib),
    ])
}

#[test]
fn an_upward_import_is_an_upward_edge_error_with_its_site() {
    let dir = layered("  %% order: entry > core\n", true);
    let output = driftwall(&dir, &["lint", "--format", "json"]);
    assert_eq!(code(&output), 1);
    let value = json(&output);
    assert_eq!(kinds(&value), ["undeclared-edge", "upward-edge"]);
    let upward = &value["violations"][1];
    assert_eq!(upward["severity"], "error");
    assert_eq!(upward["at"]["file"], "lib/lib.ts");
    assert_eq!(upward["at"]["line"], 1);
    assert!(
        upward["message"]
            .as_str()
            .unwrap()
            .contains("`entry` above `core`"),
        "{upward}"
    );
}

#[test]
fn the_same_code_without_an_order_line_only_lacks_the_edge() {
    let dir = layered("", true);
    let output = driftwall(&dir, &["lint", "--format", "json"]);
    assert_eq!(kinds(&json(&output)), ["undeclared-edge"]);
}

#[test]
fn a_declared_upward_arrow_fails_the_contract_with_its_line() {
    let dir = layered("  lib --> app\n  %% order: entry > core\n", false);
    let output = driftwall(&dir, &["lint"]);
    assert_eq!(code(&output), 2);
    let text = stderr(&output);
    assert!(text.contains("line 9"), "{text}");
    assert!(text.contains("points upward"), "{text}");
}

#[test]
fn brief_and_neighbors_show_the_layer_and_the_order() {
    let dir = layered("  %% order: entry > core\n", false);
    let brief = stdout(&driftwall(&dir, &["brief"]));
    assert!(
        brief.contains("app: app (1 file) layer: entry (1 of 2)"),
        "{brief}"
    );
    assert!(
        brief.contains("lib: lib (1 file) layer: core (2 of 2)"),
        "{brief}"
    );
    assert!(brief.contains("rules:\n  order: entry > core\n"), "{brief}");
    let neighbors = stdout(&driftwall(&dir, &["neighbors", "lib"]));
    assert!(neighbors.contains("layer: core (2 of 2)"), "{neighbors}");
    let value = json(&driftwall(&dir, &["brief", "--format", "json"]));
    assert_eq!(
        value["rules"]["order"],
        serde_json::json!(["entry", "core"])
    );
    assert_eq!(value["nodes"][1]["layer"]["boundary"], "core");
    assert_eq!(value["nodes"][1]["layer"]["rank"], 1);
}

#[test]
fn a_contract_without_rules_has_no_rules_in_brief() {
    let dir = layered("", false);
    let text = stdout(&driftwall(&dir, &["brief"]));
    assert!(
        !text.contains("rules:") && !text.contains("layer:"),
        "{text}"
    );
    let value = json(&driftwall(&dir, &["brief", "--format", "json"]));
    assert!(value.get("rules").is_none(), "{value}");
    assert!(value["nodes"][0].get("layer").is_none(), "{value}");
}

#[test]
fn brief_lists_the_no_cycles_lines_and_json_flags_them() {
    let dir = layered("  %% no-cycles\n  %% no-cycles: boundaries\n", false);
    let text = stdout(&driftwall(&dir, &["brief"]));
    assert!(
        text.contains("rules:\n  no-cycles\n  no-cycles: boundaries\n"),
        "{text}"
    );
    let value = json(&driftwall(&dir, &["brief", "--format", "json"]));
    assert_eq!(value["rules"]["no_cycles"], true);
    assert_eq!(value["rules"]["no_cycles_boundaries"], true);
    assert!(value["rules"].get("order").is_none(), "{value}");
}

#[test]
fn a_declared_cycle_fails_lint_with_exit_two_and_the_cycle() {
    let dir = layered("  lib --> app\n  %% no-cycles\n", false);
    let output = driftwall(&dir, &["lint"]);
    assert_eq!(code(&output), 2);
    let text = stderr(&output);
    assert!(text.contains("line 10"), "{text}");
    assert!(
        text.contains("app --> lib (line 8), lib --> app (line 9)"),
        "{text}"
    );
}

#[test]
fn an_over_budget_node_is_a_warning_at_the_budget_line_naming_the_numbers() {
    // why: `app` owns 1 file of 2 lines and depends on 1 node, so lines 1 and fan-out 0 are over.
    let dir = layered("  %% budget app: fan-out 0, files 1, lines 1\n", false);
    let output = driftwall(&dir, &["lint", "--format", "json"]);
    assert_eq!(code(&output), 0);
    let value = json(&output);
    assert_eq!(kinds(&value), ["over-budget", "over-budget"]);
    let first = &value["violations"][0];
    assert_eq!(first["severity"], "warning");
    assert_eq!(first["at"]["file"], "architecture.mmd");
    assert_eq!(first["at"]["line"], 9);
    let messages: Vec<&str> = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["message"].as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "node `app` is over its budget: fan-out 1 exceeds the limit 0",
            "node `app` is over its budget: lines 2 exceeds the limit 1",
        ]
    );
    assert_eq!(code(&driftwall(&dir, &["lint", "--strict"])), 1);
}

#[test]
fn a_node_within_its_budget_is_clean() {
    let dir = layered("  %% budget app: fan-out 1, files 1, lines 2\n", false);
    assert_eq!(code(&driftwall(&dir, &["lint"])), 0);
}

#[test]
fn brief_and_neighbors_show_the_budget_with_its_current_values() {
    let dir = layered("  %% budget app: fan-out 0, files 1, lines 9\n", false);
    let brief = stdout(&driftwall(&dir, &["brief"]));
    assert!(
        brief.contains("app: app (1 file) budget: fan-out 1/0 over, files 1/1, lines 2/9\n"),
        "{brief}"
    );
    let neighbors = stdout(&driftwall(&dir, &["neighbors", "app"]));
    assert!(
        neighbors.contains("budget: fan-out 1/0 over, files 1/1, lines 2/9\n"),
        "{neighbors}"
    );
    let value = json(&driftwall(&dir, &["brief", "--format", "json"]));
    assert_eq!(
        value["nodes"][0]["budget"],
        serde_json::json!({
            "fan_out": {"limit": 0, "actual": 1},
            "files": {"limit": 1, "actual": 1},
            "lines": {"limit": 9, "actual": 2},
        })
    );
    assert!(value["nodes"][1].get("budget").is_none(), "{value}");
}
