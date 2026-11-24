//! Nested boundaries and `%% isolated` through the CLI, on a feature-module layout: seven layers,
//! a `features` boundary that holds one boundary per feature, and four nodes in each feature.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fmt::Write as _;
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    pub const FEATURES: [&str; 3] = ["billing", "search", "account"];
    pub const PARTS: [&str; 4] = ["controller", "service", "view", "store"];

    /// The contract: seven layers, `features` with three feature boundaries, the arrows of the
    /// layout, then `extra` lines.
    pub fn contract(extra: &str) -> String {
        let mut out = String::from("flowchart LR\n");
        out.push_str("  subgraph app[\"App\"]\n    app-shell\n  end\n");
        out.push_str("  subgraph features[\"Features\"]\n");
        for feature in FEATURES {
            let _ = writeln!(out, "    subgraph {feature}[\"{feature}\"]");
            for part in PARTS {
                let _ = writeln!(out, "      {feature}-{part}");
            }
            out.push_str("    end\n");
        }
        out.push_str("  end\n");
        out.push_str("  subgraph shared[\"Shared\"]\n    shared-ui\n  end\n");
        out.push_str("  subgraph core[\"Core\"]\n    core-kernel\n  end\n");
        out.push_str("  subgraph domain[\"Domain\"]\n    domain-model\n  end\n");
        out.push_str("  subgraph data[\"Data\"]\n    data-store\n  end\n");
        out.push_str("  subgraph platform[\"Platform\"]\n    platform-env\n  end\n");
        out.push_str(
            "  app --> features\n  features --> shared\n  features --> core\n  shared --> core\n  core --> domain\n  core --> platform\n  data --> platform\n",
        );
        for feature in FEATURES {
            let _ = writeln!(out, "  {feature} --> {feature}");
        }
        out.push_str("  %% order: app > features > shared > core > domain > data > platform\n");
        out.push_str(extra);
        out
    }

    /// The sources of the layout: each node is a directory with one file that imports what its
    /// arrows allow.
    pub fn sources() -> Vec<(String, String)> {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut node = |id: &str, imports: &[String]| {
            files.push((format!("{id}/.arch"), format!("node: {id}\n")));
            let mut body = String::new();
            for target in imports {
                let _ = writeln!(body, "import {{ v }} from '../{target}/v';");
            }
            body.push_str("export const v = 1;\n");
            files.push((format!("{id}/v.ts"), body));
        };
        let features: Vec<String> = FEATURES
            .iter()
            .map(|feature| format!("{feature}-controller"))
            .collect();
        node("app-shell", &features);
        for feature in FEATURES {
            node(
                &format!("{feature}-controller"),
                &[
                    format!("{feature}-service"),
                    format!("{feature}-view"),
                    "shared-ui".to_owned(),
                ],
            );
            node(
                &format!("{feature}-service"),
                &[format!("{feature}-store"), "core-kernel".to_owned()],
            );
            node(&format!("{feature}-view"), &["shared-ui".to_owned()]);
            node(&format!("{feature}-store"), &[]);
        }
        node("shared-ui", &["core-kernel".to_owned()]);
        node(
            "core-kernel",
            &["domain-model".to_owned(), "platform-env".to_owned()],
        );
        node("domain-model", &[]);
        node("data-store", &["platform-env".to_owned()]);
        node("platform-env", &[]);
        files
    }

    pub fn repo(contract: &str, extra: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        let files: Vec<(String, String)> = sources()
            .into_iter()
            .chain([("architecture.mmd".to_owned(), contract.to_owned())])
            .chain(
                extra
                    .iter()
                    .map(|(path, body)| ((*path).to_owned(), (*body).to_owned())),
            )
            .collect();
        for (path, body) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, body).unwrap();
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

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8(output.stderr.clone()).unwrap()
    }

    pub fn json(output: &Output) -> serde_json::Value {
        serde_json::from_str(&stdout(output)).unwrap()
    }

    /// `kind` of every violation, in report order.
    pub fn kinds(value: &serde_json::Value) -> Vec<String> {
        value["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["kind"].as_str().unwrap().to_owned())
            .collect()
    }
}

use support::{code, contract, json, kinds, repo, run, stderr, stdout};

const ISOLATED: &str = "  %% isolated: features\n";

#[test]
fn the_layout_lints_clean_with_nesting_and_isolation() {
    let dir = repo(&contract(ISOLATED), &[]);
    let output = run(&dir, &["lint", "--strict", "--format", "json"]);
    assert_eq!(kinds(&json(&output)), Vec::<String>::new());
    assert_eq!(code(&output), 0);
}

#[test]
fn a_cross_feature_import_is_an_isolation_breach_even_with_the_nodes_declared() {
    let dir = repo(
        &contract(ISOLATED),
        &[(
            "billing-service/cross.ts",
            "import { v } from '../search-view/v';\nexport const w = v;\n",
        )],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    let value = json(&output);
    // why: the pair is also undeclared, and the two findings have two fixes.
    assert_eq!(kinds(&value), ["undeclared-edge", "isolation-breach"]);
    let breach = &value["violations"][1];
    assert_eq!(breach["at"]["file"], "billing-service/cross.ts");
    let message = breach["message"].as_str().unwrap();
    assert!(message.contains("`features`") && message.contains("`billing`"));
    assert!(message.contains("`search`"), "{message}");
    assert_eq!(code(&output), 1);
}

#[test]
fn an_arrow_for_the_cross_feature_import_is_a_contract_error_not_a_way_out() {
    let sources = [(
        "billing-service/cross.ts",
        "import { v } from '../search-view/v';\nexport const w = v;\n",
    )];
    for arrow in [
        "  billing-service --> search-view\n",
        "  billing-service -.-> search-view\n",
        "  billing-service -->|test| search-view\n",
        "  billing-service -->|type| search-view\n",
        "  billing --> search\n",
        "  features --> features\n",
        "  billing --> search-view\n",
    ] {
        let dir = repo(&contract(&format!("{ISOLATED}{arrow}")), &sources);
        let output = run(&dir, &["lint"]);
        assert_eq!(code(&output), 2, "{arrow}");
        let error = stderr(&output);
        assert!(error.contains("keeps apart"), "{arrow}: {error}");
        assert!(error.contains("%% isolated"), "{arrow}: {error}");
    }
}

#[test]
fn the_arrow_error_names_the_arrow_line_the_children_and_the_isolated_line() {
    let dir = repo(&contract(&format!("{ISOLATED}  billing --> search\n")), &[]);
    let output = run(&dir, &["lint"]);
    let error = stderr(&output);
    assert!(
        error.contains(
            "edge `billing --> search` joins `billing` and `search`, children of `features`"
        ),
        "{error}"
    );
    // why: the arrow follows the `%% isolated` line, which follows the 50 lines of the layout.
    assert!(error.contains("line 52: edge"), "{error}");
    assert!(error.contains("`%% isolated` on line 51"), "{error}");
}

#[test]
fn a_test_site_across_features_is_a_breach_too() {
    let dir = repo(
        &contract(ISOLATED),
        &[(
            "billing-service/cross.test.ts",
            "import { v } from '../search-view/v';\nexport const w = v;\n",
        )],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    let value = json(&output);
    assert!(kinds(&value).contains(&"isolation-breach".to_owned()));
    let breach = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "isolation-breach")
        .unwrap();
    assert_eq!(breach["test_only"], true);
}

#[test]
fn without_isolated_the_same_import_is_only_undeclared() {
    let dir = repo(
        &contract(""),
        &[(
            "billing-service/cross.ts",
            "import { v } from '../search-view/v';\nexport const w = v;\n",
        )],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    assert_eq!(kinds(&json(&output)), ["undeclared-edge"]);
}

#[test]
fn an_import_inside_one_feature_and_out_of_features_are_not_breaches() {
    let dir = repo(
        &contract(ISOLATED),
        &[(
            "billing-view/more.ts",
            "import { v } from '../billing-store/v';\nimport { s } from '../shared-ui/v';\n",
        )],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    assert_eq!(kinds(&json(&output)), Vec::<String>::new());
}

#[test]
fn an_upward_import_from_shared_into_a_feature_is_an_upward_edge_through_nesting() {
    let dir = repo(
        &contract(ISOLATED),
        &[(
            "shared-ui/up.ts",
            "import { v } from '../billing-view/v';\nexport const w = v;\n",
        )],
    );
    let output = run(&dir, &["lint", "--format", "json"]);
    let found = kinds(&json(&output));
    assert!(found.contains(&"upward-edge".to_owned()), "{found:?}");
}

#[test]
fn order_listing_a_boundary_with_one_nested_in_it_is_a_contract_error() {
    let dir = repo(&contract("  %% order: features > billing\n"), &[]);
    let output = run(&dir, &["lint"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("`features` contains `billing`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn brief_shows_the_isolation_in_text_and_json() {
    let dir = repo(&contract(ISOLATED), &[]);
    let output = run(&dir, &["brief"]);
    assert!(
        stdout(&output).contains("  isolated: features (account, billing, search)"),
        "{}",
        stdout(&output)
    );
    let output = run(&dir, &["brief", "--format", "json"]);
    let value = json(&output);
    assert_eq!(
        value["rules"]["isolated"],
        serde_json::json!([{"boundary": "features", "children": ["account", "billing", "search"]}])
    );
    let plain = repo(&contract(""), &[]);
    assert!(json(&run(&plain, &["brief", "--format", "json"]))["rules"]["isolated"].is_null());
}

#[test]
fn a_boundary_edge_from_features_covers_every_node_at_any_depth() {
    // why: `features --> shared` and `features --> core` are the only arrows to those layers, and
    // every nested node imports them, so the clean run above already proves coverage; here a
    // node edge for one covered pair is redundant.
    let dir = repo(&contract("  billing-view --> shared-ui\n"), &[]);
    let output = run(&dir, &["lint", "--format", "json"]);
    assert_eq!(kinds(&json(&output)), ["redundant-edge"]);
}
