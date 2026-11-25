//! `%% each <boundary>: map <dir>/` through the CLI: one line maps every feature's part folders,
//! `lint` reports what an expansion gets wrong at the `%% each` line, and `where` names it (see
//! "Edge templates" in `docs/DESIGN.md`).

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use serde_json::Value;
    use tempfile::TempDir;

    /// Three features with a controller, a service and a view each, and a shared `request` layer.
    /// `extra` follows the map lines.
    pub fn contract(extra: &str) -> String {
        let mut text = String::from("flowchart LR\n  subgraph features\n");
        for feature in ["billing", "orders", "search"] {
            let lines = [
                format!("    subgraph {feature}\n"),
                format!("      {feature}-controller\n"),
                format!("      {feature}-service\n"),
                format!("      {feature}-view\n"),
                "    end\n".to_owned(),
            ];
            text.extend(lines);
        }
        text.push_str("  end\n  subgraph shared\n    request\n  end\n");
        text.push_str(
            "  %% map request: src/request/\n  %% each features: map src/features/{child}/\n",
        );
        text.push_str(extra);
        text
    }

    /// A repository with `contract` and, for each of `parts`, one file in `src/features/<part>`.
    pub fn repo(contract: &str, parts: &[&str]) -> TempDir {
        let dir = TempDir::new().unwrap();
        let mut files: Vec<(String, String)> = vec![
            ("architecture.mmd".to_owned(), contract.to_owned()),
            (
                "src/request/r.ts".to_owned(),
                "export const r = 1;\n".to_owned(),
            ),
        ];
        files.extend(parts.iter().map(|part| {
            (
                format!("src/features/{part}/index.ts"),
                "export const x = 1;\n".to_owned(),
            )
        }));
        for (path, body) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, body).unwrap();
        }
        dir
    }

    /// Every part folder of the three features.
    pub fn all_parts() -> Vec<String> {
        let mut parts = Vec::new();
        for feature in ["billing", "orders", "search"] {
            for part in ["controller", "service", "view"] {
                parts.push(format!("{feature}/{part}"));
            }
        }
        parts
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

    /// `kind file:line message (hint)` of every violation.
    pub fn found(dir: &TempDir) -> Vec<(String, String, String, String)> {
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
                    v["hint"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }
}

use support::{all_parts, contract, found, out, repo, run};

/// The line of the `%% each` map line in the fixture contract: three features of five lines, and
/// the header, the boundary and its end make the shared layer start at 19.
const EACH_LINE: &str = "architecture.mmd:23";

fn parts_as_refs(parts: &[String]) -> Vec<&str> {
    parts.iter().map(String::as_str).collect()
}

#[test]
fn one_each_map_line_maps_every_feature_and_lints_clean() {
    let parts = all_parts();
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    assert_eq!(found(&dir), Vec::new());
    assert_eq!(run(&dir, &["lint", "--strict"]).status.code(), Some(0));
}

#[test]
fn a_feature_folder_without_a_subgraph_is_unmapped_with_the_draw_it_hint() {
    let mut parts = all_parts();
    parts.push("shipping/service".to_owned());
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    let findings = found(&dir);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let (kind, at, _, hint) = &findings[0];
    assert_eq!(kind, "unmapped-file");
    assert_eq!(at, "src/features/shipping/service/index.ts:1");
    assert_eq!(
        hint,
        "draw a `shipping` subgraph inside `features`, or ask the user to approve adding it"
    );
}

#[test]
fn an_unmapped_file_elsewhere_keeps_the_generic_hint() {
    let parts = all_parts();
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    std::fs::write(dir.path().join("src/stray.ts"), "export const s = 1;\n").unwrap();
    let findings = found(&dir);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].0, "unmapped-file");
    assert!(findings[0].3.contains("`.arch` tag"), "{findings:?}");
}

#[test]
fn a_part_folder_with_no_declared_node_is_unknown_node_naming_the_child_and_the_line() {
    let mut parts = all_parts();
    parts.push("billing/report".to_owned());
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    let findings = found(&dir);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let (kind, at, message, _) = &findings[0];
    assert_eq!(kind, "unknown-node");
    assert_eq!(at, EACH_LINE);
    assert!(
        message.contains("each features map src/features/{child}/")
            && message.contains("child `billing`")
            && message.contains("`billing-report`"),
        "{message}"
    );
}

#[test]
fn a_child_with_no_folder_is_a_stale_map_naming_the_child() {
    let parts: Vec<String> = all_parts()
        .into_iter()
        .filter(|part| !part.starts_with("search/"))
        .collect();
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    let findings = found(&dir);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let (kind, at, message, _) = &findings[0];
    assert_eq!(kind, "stale-map");
    assert_eq!(at, EACH_LINE);
    assert!(
        message.contains("child `search`") && message.contains("src/features/search/"),
        "{message}"
    );
}

#[test]
fn a_written_map_line_beside_the_template_follows_specificity() {
    // why: the written line is more specific for `orders/view`, so that folder joins `request`.
    let parts = all_parts();
    let text = contract("  %% map request: src/features/orders/view/\n");
    let dir = repo(&text, &parts_as_refs(&parts));
    assert_eq!(found(&dir), Vec::new());
    let moved = run(&dir, &["where", "src/features/orders/view/index.ts"]);
    assert!(
        out(&moved).contains("request (via architecture.mmd:24 (map src/features/orders/view/))"),
        "{}",
        out(&moved)
    );
    let kept = run(&dir, &["where", "src/features/orders/service/index.ts"]);
    assert!(out(&kept).contains("orders-service"), "{}", out(&kept));
}

#[test]
fn where_names_the_each_line() {
    let parts = all_parts();
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    let answer = run(&dir, &["where", "src/features/billing/service/index.ts"]);
    assert_eq!(
        out(&answer),
        "src/features/billing/service/index.ts: billing-service (via architecture.mmd:23 (each features map src/features/{child}/))\n"
    );
    let json = run(
        &dir,
        &[
            "where",
            "src/features/billing/service/index.ts",
            "--format",
            "json",
        ],
    );
    let text = out(&json);
    assert!(
        text.contains("\"each\": \"each features map src/features/{child}/\""),
        "{text}"
    );
}

#[test]
fn brief_shows_the_directories_of_derived_nodes() {
    let parts = all_parts();
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    let text = out(&run(&dir, &["brief"]));
    assert!(
        text.contains("  billing-service: src/features/billing/service (1 file)\n"),
        "{text}"
    );
}

#[test]
fn a_changed_template_is_a_rule_change_and_its_files_are_moved() {
    let parts = all_parts();
    let dir = repo(&contract(""), &parts_as_refs(&parts));
    let at = dir.path();
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    ] {
        let mut command = std::process::Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        assert!(
            command
                .args(args)
                .current_dir(at)
                .status()
                .unwrap()
                .success()
        );
    }
    let edited = contract("").replace("src/features/{child}/", "lib/{child}/");
    std::fs::write(at.join("architecture.mmd"), edited).unwrap();
    let output = run(&dir, &["diff", "--base", "HEAD"]);
    let text = out(&output);
    assert!(
        text.contains("  + %% each features: map lib/{child}/\n")
            && text.contains("  - %% each features: map src/features/{child}/\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "src/features/billing/service/index.ts: billing-service -> (none) (mapping changed)"
        ),
        "{text}"
    );
}
