//! End-to-end tests of `%% order open:`: what `lint`, `brief` and `neighbors` say about it.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fmt::Write as _;
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    pub const CHAIN: &str = "app > features > shared > core > domain > infra > base";

    const LAYERS: [&str; 7] = [
        "app", "features", "shared", "core", "domain", "infra", "base",
    ];

    /// Seven layers, one node and one directory each, an open chain on line 25, `extra` after it,
    /// and the given imports as `(from, to)` pairs of nodes. `features` also owns `feat2` and
    /// `core` also owns `core2`.
    pub fn repo(extra: &str, imports: &[(&str, &str)]) -> TempDir {
        let mut contract = String::from("flowchart LR\n");
        for layer in LAYERS {
            let _ = write!(contract, "  subgraph {layer}[\"{layer}\"]\n    {layer}_n\n");
            match layer {
                "features" => contract.push_str("    feat2\n"),
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
        nodes.extend(["feat2".to_owned(), "core2".to_owned()]);
        for node in &nodes {
            write(&format!("{node}/.arch"), &format!("node: {node}\n"));
            let mut body = String::new();
            for (_, to) in imports.iter().filter(|(from, _)| from == node) {
                let _ = writeln!(body, "import {{ x }} from '../{to}/m';");
            }
            write(
                &format!("{node}/m.ts"),
                &format!("{body}export const x = 1;\n"),
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

use support::{CHAIN, json, kinds, repo, run, stdout};

#[test]
fn a_downward_import_across_three_layers_is_clean_with_zero_arrows() {
    let dir = repo("", &[("app_n", "core_n"), ("features_n", "base_n")]);
    let output = run(&dir, &["lint", "--format", "json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_eq!(kinds(&json(&output)), Vec::<&str>::new());
}

#[test]
fn an_upward_import_is_reported() {
    let dir = repo("", &[("core_n", "app_n")]);
    let output = run(&dir, &["lint", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(kinds(&json(&output)), ["upward-edge"]);
}

#[test]
fn one_arrow_narrows_features_to_core_and_leaves_features_to_shared() {
    let dir = repo(
        "  features_n --> core_n\n",
        &[
            ("features_n", "core_n"),
            ("feat2", "core2"),
            ("feat2", "shared_n"),
        ],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_eq!(kinds(&value), ["undeclared-edge"]);
    let violation = &value["violations"][0];
    assert_eq!(violation["at"]["file"], "feat2/m.ts");
    let message = violation["message"].as_str().unwrap();
    assert!(
        message.contains("declares no edge feat2 --> core2"),
        "{message}"
    );
    assert!(
        message.contains(
            "limits `features` to the edges it declares towards `core` (1 declared) and this pair is not among them"
        ),
        "{message}"
    );
}

#[test]
fn a_test_arrow_narrows_too() {
    let dir = repo("  features_n -->|test| core_n\n", &[("feat2", "core2")]);
    let output = run(&dir, &["lint", "--format", "json"]);
    let value = json(&output);
    assert_eq!(kinds(&value), ["undeclared-edge", "stale-edge"]);
}

#[test]
fn brief_names_the_open_chain_for_a_pair_only_it_allows() {
    let dir = repo("", &[("app_n", "core_n")]);
    let brief = stdout(&run(&dir, &["brief"]));
    assert!(
        brief.contains(&format!(
            "app_n --> core_n (1 import) via order open: {CHAIN}, line 25"
        )),
        "{brief}"
    );
    assert!(
        brief.contains(&format!("  order open: {CHAIN}\n")),
        "{brief}"
    );
    assert!(!brief.contains("UNDECLARED"), "{brief}");
    let neighbors = stdout(&run(&dir, &["neighbors", "core_n"]));
    assert!(neighbors.contains("via order open:"), "{neighbors}");
    let value = json(&run(&dir, &["brief", "--format", "json"]));
    let edge = value["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["from"] == "app_n")
        .unwrap();
    assert_eq!(edge["kind"], "implemented");
    assert_eq!(edge["via"]["edge"], format!("order open: {CHAIN}"));
    assert_eq!(edge["via"]["line"], 25);
    assert_eq!(value["rules"]["open_order"][0][0], "app");
}
