//! A site that breaks the layer order or an isolation has one finding: the arrow that
//! `undeclared-edge` would ask for is a contract error, so the fix cannot work.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
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

    pub fn lint(dir: &TempDir, format: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(["lint", "--format", format])
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn out(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    pub fn json(output: &Output) -> serde_json::Value {
        serde_json::from_str(&out(output)).unwrap()
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }
}

use support::{code, json, lint, out, repo};

const ISOLATED_HINT: &str = "remove the import, or move the shared code to a node outside the isolated boundary, or ask the user to approve changing the `%% isolated` line";

fn features(imports: &str) -> tempfile::TempDir {
    repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  subgraph features[\"Features\"]\n    subgraph billing[\"Billing\"]\n      billing_n\n    end\n    subgraph search[\"Search\"]\n      search_n\n    end\n  end\n  %% isolated: features\n",
        ),
        ("billing_n/.arch", "node: billing_n\n"),
        ("search_n/.arch", "node: search_n\n"),
        ("billing_n/a.ts", imports),
        ("search_n/b.ts", "export const b = 1;\n"),
    ])
}

#[test]
fn a_cross_feature_import_is_one_finding_in_every_format() {
    let dir = features("import { b } from '../search_n/b';\nexport const a = b;\n");
    let output = lint(&dir, "json");
    assert_eq!(code(&output), 1);
    let value = json(&output);
    let found = value["violations"].as_array().unwrap();
    assert_eq!(found.len(), 1, "{value}");
    assert_eq!(found[0]["kind"], "isolation-breach");
    assert_eq!(found[0]["hint"], ISOLATED_HINT);
    assert_eq!(value["summary"]["errors"], 1);

    let text = out(&lint(&dir, "text"));
    assert_eq!(text.matches("isolation-breach").count(), 1, "{text}");
    assert!(!text.contains("undeclared-edge"), "{text}");
    assert!(text.contains(ISOLATED_HINT), "{text}");

    let sarif: serde_json::Value = serde_json::from_str(&out(&lint(&dir, "sarif"))).unwrap();
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{sarif}");
    assert_eq!(results[0]["ruleId"], "isolation-breach");
    assert!(
        results[0]["message"]["text"]
            .as_str()
            .unwrap()
            .contains(ISOLATED_HINT)
    );
}

fn layered(order_mmd: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let contract = format!(
        "flowchart LR\n  subgraph entry[\"Entry\"]\n    app\n  end\n  subgraph core[\"Core\"]\n    lib\n  end\n{order_mmd}"
    );
    let mut all = vec![
        ("architecture.mmd", contract.as_str()),
        ("app/.arch", "node: app\n"),
        ("lib/.arch", "node: lib\n"),
        ("app/main.ts", "export const m = 1;\n"),
    ];
    all.extend_from_slice(files);
    repo(&all)
}

#[test]
fn an_upward_production_import_is_one_upward_edge() {
    let dir = layered(
        "  %% order: entry > core\n",
        &[(
            "lib/l.ts",
            "import { m } from '../app/main';\nexport const l = m;\n",
        )],
    );
    let output = lint(&dir, "json");
    assert_eq!(code(&output), 1);
    let value = json(&output);
    let found = value["violations"].as_array().unwrap();
    assert_eq!(found.len(), 1, "{value}");
    assert_eq!(found[0]["kind"], "upward-edge");
    assert_eq!(
        found[0]["hint"],
        "remove the import, or ask the user to approve changing the `%% order` line"
    );
}

#[test]
fn an_upward_production_import_on_a_pair_with_only_a_test_edge_is_one_upward_edge() {
    let dir = layered(
        "  %% order: entry > core\n  lib -->|test| app\n",
        &[(
            "lib/l.ts",
            "import { m } from '../app/main';\nexport const l = m;\n",
        )],
    );
    let value = json(&lint(&dir, "json"));
    let kinds: Vec<_> = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["kind"].as_str().unwrap())
        .collect();
    // why: the test edge is stale (no test site), which is about the edge, not the site.
    assert_eq!(kinds, ["stale-edge", "upward-edge"], "{value}");
}

#[test]
fn an_upward_test_import_names_the_test_edge_and_declaring_it_makes_lint_clean() {
    let test_file = (
        "lib/l.test.ts",
        "import { m } from '../app/main';\nexport const t = m;\n",
    );
    let dir = layered("  %% order: entry > core\n", &[test_file]);
    let output = lint(&dir, "json");
    assert_eq!(code(&output), 1);
    let value = json(&output);
    let found = value["violations"].as_array().unwrap();
    assert_eq!(found.len(), 1, "{value}");
    assert_eq!(found[0]["kind"], "upward-edge");
    let hint = found[0]["hint"].as_str().unwrap();
    assert!(hint.contains("`lib -->|test| app`"), "{hint}");
    assert!(hint.starts_with("remove the import, or ask"), "{hint}");

    let dir = layered(
        "  %% order: entry > core\n  lib -->|test| app\n",
        &[test_file],
    );
    let output = lint(&dir, "json");
    assert_eq!(code(&output), 0, "{}", out(&output));
    assert_eq!(json(&output)["violations"].as_array().unwrap().len(), 0);
}
