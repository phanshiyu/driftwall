//! `%% each` with package rules and denials through the CLI: `lint` enforces every expansion,
//! `brief` shows them, and `diff` reports the template line (see
//! "Edge templates" in `docs/DESIGN.md`).

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use serde_json::Value;
    use tempfile::TempDir;

    /// Two features and a shared `request` layer under an open chain; `search` has no view. The
    /// first line after the maps is `extra`.
    pub fn contract(extra: &str) -> String {
        format!(
            "flowchart LR
  subgraph features
    subgraph billing
      billing-service
      billing-view
    end
    subgraph search
      search-service
    end
  end
  subgraph shared
    request
  end
  %% order open: features > shared
  %% map billing-*: src/billing/
  %% map search-*: src/search/
  %% map request: src/request/
{extra}"
        )
    }

    pub fn repo(contract: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let files = [
            ("architecture.mmd", contract),
            (
                "package.json",
                r#"{"dependencies": {"maud": "1", "axum": "1", "zod": "1"}}"#,
            ),
            (
                "src/billing/service.ts",
                "import { m } from 'maud';\nimport { z } from 'zod';\nexport const s = [m, z];\n",
            ),
            (
                "src/billing/view.ts",
                "import { z } from 'zod';\nimport { r } from '../request/r';\nexport const v = [z, r];\n",
            ),
            (
                "src/search/service.ts",
                "import { a } from 'axum';\nexport const s = a;\n",
            ),
            ("src/request/r.ts", "export const r = 1;\n"),
        ];
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
            .current_dir(dir.path())
            .output()
            .unwrap()
    }

    pub fn out(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub fn err(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    /// `kind file:line message` of every violation.
    pub fn found(dir: &TempDir) -> Vec<(String, String, String)> {
        let output = run(dir, &["lint", "--format", "json"]);
        let json: Value = serde_json::from_str(&out(&output)).unwrap();
        json["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                (
                    v["kind"].as_str().unwrap().to_owned(),
                    format!(
                        "{}:{}",
                        v["at"]["file"].as_str().unwrap_or_default(),
                        v["at"]["line"].as_u64().unwrap_or_default()
                    ),
                    v["message"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }
}

use support::{contract, err, found, out, repo, run};

const FORBID: &str = "  %% each features: forbid-packages service: maud, axum\n";

#[test]
fn a_forbid_packages_template_fails_the_lint_for_every_child_with_the_role() {
    let dir = repo(&contract(FORBID));
    let findings = found(&dir);
    let kinds: Vec<(&str, &str)> = findings
        .iter()
        .map(|(kind, at, _)| (kind.as_str(), at.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("forbidden-package", "src/billing/service.ts:1"),
            ("forbidden-package", "src/search/service.ts:1"),
        ]
    );
    let message = &findings[0].2;
    assert!(
        message.contains(
            "`billing-service` imports `maud` (third-party) but architecture.mmd:18 `%% each features: forbid-packages service` forbids it"
        ),
        "{message}"
    );
    assert_eq!(run(&dir, &["lint"]).status.code(), Some(1));
}

#[test]
fn a_packages_template_is_an_allow_list_per_child_and_a_child_without_the_role_is_skipped() {
    // why: `search` has no view, so the allow-list of views restricts billing's view alone.
    let dir = repo(&contract("  %% each features: packages view: maud\n"));
    let findings = found(&dir);
    let kinds: Vec<(&str, &str)> = findings
        .iter()
        .map(|(kind, at, _)| (kind.as_str(), at.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("forbidden-package", "src/billing/view.ts:1"),
            ("stale-package", "architecture.mmd:18"),
        ]
    );
    assert!(
        findings[1]
            .2
            .contains("`%% each features: packages view` lists `maud`"),
        "{}",
        findings[1].2
    );
}

#[test]
fn an_empty_packages_template_allows_no_third_party_package() {
    let dir = repo(&contract("  %% each features: packages service:\n"));
    let kinds: Vec<(String, String)> = found(&dir)
        .into_iter()
        .map(|(kind, at, _)| (kind, at))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                "forbidden-package".to_owned(),
                "src/billing/service.ts:1".to_owned()
            ),
            (
                "forbidden-package".to_owned(),
                "src/billing/service.ts:2".to_owned()
            ),
            (
                "forbidden-package".to_owned(),
                "src/search/service.ts:1".to_owned()
            ),
        ]
    );
}

#[test]
fn a_deny_template_denies_the_pair_for_each_child() {
    let dir = repo(&contract("  %% each features: deny view: request\n"));
    let findings = found(&dir);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].0, "denied-edge");
    assert_eq!(findings[0].1, "src/billing/view.ts:2");
    assert!(
        findings[0].2.contains(
            "`billing-view` imports `request` (src/request/r.ts) but architecture.mmd:18 `%% each features: deny view: request` forbids it"
        ),
        "{}",
        findings[0].2
    );
}

#[test]
fn the_template_forms_lint_clean_when_the_code_keeps_to_them() {
    let rules = "  %% each features: forbid-packages view: maud\n  %% each features: deny service: request\n";
    let dir = repo(&contract(rules));
    let output = run(&dir, &["lint", "--strict"]);
    assert_eq!(out(&output), "driftwall: clean\n", "{}", err(&output));
}

#[test]
fn a_role_that_resolves_in_no_child_is_a_contract_error() {
    let dir = repo(&contract(
        "  %% each features: forbid-packages servce: maud\n",
    ));
    let output = run(&dir, &["lint"]);
    assert_eq!(output.status.code(), Some(2));
    let message = err(&output);
    assert!(message.contains("line 18"), "{message}");
    assert!(
        message.contains("role `servce` resolves in no child of `features`"),
        "{message}"
    );
}

#[test]
fn a_name_forbidden_by_a_written_line_and_a_template_is_the_duplicate_error() {
    let dir = repo(&contract(
        "  %% forbid-packages billing-service: maud\n  %% each features: forbid-packages service: maud\n",
    ));
    let output = run(&dir, &["lint"]);
    assert_eq!(output.status.code(), Some(2));
    let message = err(&output);
    assert!(message.contains("line 19"), "{message}");
    assert!(
        message.contains("`maud` for `billing-service` is already listed on line 18"),
        "{message}"
    );
}

#[test]
fn a_denial_expansion_that_contradicts_an_edge_names_the_child_at_the_each_line() {
    let dir = repo(&contract(
        "  billing-view --> request\n  %% each features: deny view: request\n",
    ));
    let output = run(&dir, &["lint"]);
    assert_eq!(output.status.code(), Some(2));
    let message = err(&output);
    assert!(message.contains("line 19"), "{message}");
    assert!(
        message.contains("(expanded by `%% each features: deny view: request` for `billing`)"),
        "{message}"
    );
}

#[test]
fn a_denial_that_removes_nothing_and_a_pair_denied_twice_are_contract_errors() {
    let dead = repo(&contract("  %% each features: deny service: view\n"));
    let message = err(&run(&dead, &["lint"]));
    assert!(message.contains("removes nothing"), "{message}");
    assert!(message.contains("for `billing`"), "{message}");
    let twice = repo(&contract(
        "  %% deny billing-view: request\n  %% each features: deny view: request\n",
    ));
    let message = err(&run(&twice, &["lint"]));
    assert!(
        message.contains("`billing-view` to `request` is already denied on line 18"),
        "{message}"
    );
}

#[test]
fn brief_shows_the_expanded_rules_and_the_template_lines() {
    let rules = format!("{FORBID}  %% each features: deny view: request\n");
    let dir = repo(&contract(&rules));
    let text = out(&run(&dir, &["brief"]));
    assert!(
        text.contains("  each features: forbid-packages service: maud, axum\n"),
        "{text}"
    );
    assert!(
        text.contains("  deny: billing-view -> request (line 19)\n"),
        "{text}"
    );
    assert!(text.contains("maud (third-party"), "{text}");
    let neighbors = out(&run(&dir, &["neighbors", "billing-view"]));
    assert!(neighbors.contains("DENIED"), "{neighbors}");
    let json: serde_json::Value =
        serde_json::from_str(&out(&run(&dir, &["brief", "--format", "json"]))).unwrap();
    let each = &json["rules"]["each"];
    assert_eq!(each[0]["form"], "forbid-packages");
    assert_eq!(each[0]["arrows"][0], "forbid-packages service: maud, axum");
    assert_eq!(each[1]["form"], "deny");
    assert_eq!(json["rules"]["deny"][0]["from"], "billing-view");
}

#[test]
fn diff_reports_a_template_line_as_a_rule_change() {
    let dir = repo(&contract(""));
    let new = contract("  %% each features: forbid-packages service: maud\n");
    let git = |args: &[&str]| {
        let mut command = std::process::Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        let output = command.args(args).current_dir(dir.path()).output().unwrap();
        assert!(output.status.success(), "{}", err(&output));
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "-m",
        "base",
    ]);
    std::fs::write(dir.path().join("architecture.mmd"), &new).unwrap();
    let text = out(&run(&dir, &["diff", "--base", "HEAD"]));
    assert!(
        text.contains("%% each features: forbid-packages service: maud"),
        "{text}"
    );
}
