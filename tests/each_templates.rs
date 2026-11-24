//! `%% each` edge templates through the CLI: the output of `brief` and `neighbors`, the contract
//! errors, and what a new feature folder does to a module-form `%% map` (see "Edge templates" in
//! `docs/DESIGN.md`).

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    /// Two features of three parts each, `search` without a view, and one shared node, all mapped
    /// by module-form `%% map` lines, one per feature. The template is on line 16.
    pub const CONTRACT: &str = "flowchart LR
  subgraph features[\"Features\"]
    subgraph billing[\"Billing\"]
      billing-controller
      billing-service
      billing-view
    end
    subgraph search[\"Search\"]
      search-controller
      search-service
    end
  end
  subgraph shared[\"Shared\"]
    shared-ui
  end
  %% each features: controller --> service, controller --> view
  features --> shared
  %% map billing-*: src/features/billing/
  %% map search-*: src/features/search/
  %% map shared-ui: src/shared/
";

    pub fn repo(contract: &str, extra: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        let files = [
            ("architecture.mmd", contract),
            (
                "src/features/billing/controller.ts",
                "import { s } from './service';\nimport { v } from './view';\nimport { u } from '../../shared/ui';\nexport const c = [s, v, u];\n",
            ),
            ("src/features/billing/service.ts", "export const s = 1;\n"),
            ("src/features/billing/view.ts", "export const v = 1;\n"),
            (
                "src/features/search/controller.ts",
                "import { s } from './service';\nexport const c = s;\n",
            ),
            ("src/features/search/service.ts", "export const s = 1;\n"),
            ("src/shared/ui.ts", "export const u = 1;\n"),
        ];
        for (path, body) in files.into_iter().chain(extra.iter().copied()) {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, body).unwrap();
        }
        dir
    }

    pub fn run(root: &Path, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_driftwall"));
        // why: a hook that runs the tests must not point git at the repository being committed.
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command.args(args).current_dir(root).output().unwrap()
    }

    pub fn out(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub fn err(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    pub fn json(output: &Output) -> serde_json::Value {
        serde_json::from_str(&out(output)).unwrap()
    }
}

use support::{CONTRACT, err, json, out, repo, run};

#[test]
fn the_template_lints_clean_on_the_layout() {
    let dir = repo(CONTRACT, &[]);
    let output = run(dir.path(), &["lint", "--strict"]);
    assert_eq!(out(&output), "driftwall: clean\n", "{}", err(&output));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn brief_shows_the_template_in_rules_and_names_it_as_the_reason_a_pair_is_allowed() {
    let dir = repo(CONTRACT, &[]);
    let text = out(&run(dir.path(), &["brief"]));
    assert!(
        text.contains("  each features: controller --> service, controller --> view\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "billing-controller --> billing-service (1 import) via each features: controller --> service, line 16"
        ),
        "{text}"
    );
    // why: an expansion is no boundary edge, so the block lists only `features --> shared`.
    assert!(
        text.contains("boundary edges:\n  features --> shared"),
        "{text}"
    );
    assert!(!text.contains("(line 16): "), "{text}");
    let value = json(&run(dir.path(), &["brief", "--format", "json"]));
    assert_eq!(
        value["rules"]["each"],
        serde_json::json!([{
            "boundary": "features",
            "line": 16,
            "arrows": ["controller --> service", "controller --> view"]
        }])
    );
    let edge = value["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["from"] == "billing-controller" && e["to"] == "billing-view")
        .unwrap();
    assert_eq!(
        edge["via"],
        serde_json::json!({"edge": "each features: controller --> view", "line": 16})
    );
    assert_eq!(edge["kind"], "implemented");
    assert_eq!(value["boundary_edges"].as_array().unwrap().len(), 1);
}

#[test]
fn neighbors_names_the_template_too() {
    let dir = repo(CONTRACT, &[]);
    let text = out(&run(dir.path(), &["neighbors", "billing-controller"]));
    assert!(
        text.contains("via each features: controller --> service, line 16"),
        "{text}"
    );
}

#[test]
fn a_template_that_nothing_realises_is_one_warning_pointing_at_its_line() {
    let dir = repo(
        CONTRACT
            .replace("controller --> view", "view --> service")
            .as_str(),
        &[],
    );
    let output = run(dir.path(), &["lint", "--format", "json"]);
    let value = json(&output);
    let stale: Vec<_> = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["kind"] == "stale-edge")
        .collect();
    assert_eq!(stale.len(), 1, "{value}");
    let message = stale[0]["message"].as_str().unwrap();
    assert!(
        message.contains("`%% each features` on line 16"),
        "{message}"
    );
    assert!(message.contains("view --> service"), "{message}");
}

#[test]
fn a_typo_in_a_role_is_a_contract_error() {
    let dir = repo(&CONTRACT.replace("--> service", "--> servce"), &[]);
    let output = run(dir.path(), &["lint"]);
    assert_eq!(output.status.code(), Some(2));
    let message = err(&output);
    assert!(message.contains("line 16"), "{message}");
    assert!(
        message.contains("role `servce` resolves in no child"),
        "{message}"
    );
}

#[test]
fn a_new_feature_folder_is_reported_not_silently_clean() {
    // why: `account` has code and no `%% map` line, so its files are unmapped; the template
    // cannot declare nodes for it, and that must stay loud.
    let dir = repo(
        CONTRACT,
        &[
            (
                "src/features/account/controller.ts",
                "export const c = 1;\n",
            ),
            ("src/features/account/service.ts", "export const s = 1;\n"),
        ],
    );
    let output = run(dir.path(), &["lint", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let value = json(&output);
    let found: Vec<(&str, &str)> = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["kind"].as_str().unwrap(),
                v["at"]["file"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("unmapped-file", "src/features/account/controller.ts"),
            ("unmapped-file", "src/features/account/service.ts")
        ]
    );
}

#[test]
fn a_new_part_in_a_mapped_feature_is_an_unknown_node() {
    // why: the module form derives `billing-store` from the new file, and the contract does not
    // declare it: the template does not declare parts either.
    let dir = repo(
        CONTRACT,
        &[("src/features/billing/store.ts", "export const s = 1;\n")],
    );
    let output = run(dir.path(), &["lint", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let value = json(&output);
    let kinds: Vec<&str> = value["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["unknown-node"]);
}
