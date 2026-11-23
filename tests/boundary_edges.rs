//! Boundary edges (`features --> core`) through the CLI: lint results match the node arrows they
//! replace, and `brief`, `neighbors` and `diff` show them.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    pub fn repo(files: &[(String, String)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
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

    /// Runs git in `dir` with a fixed identity and no signing or hooks.
    pub fn git(dir: &TempDir, args: &[&str]) {
        let output = clean(Command::new("git"))
            .current_dir(dir.path())
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
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn run(dir: &TempDir, args: &[&str]) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(args)
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn text(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub const FEATURES: [&str; 6] = ["f1", "f2", "f3", "f4", "f5", "f6"];
    pub const CORE: [&str; 4] = ["c1", "c2", "c3", "c4"];

    /// A contract with the six feature nodes in `features` and the four core nodes in `core`, then
    /// `arrows` and `rules`.
    pub fn contract(arrows: &str, rules: &str) -> String {
        let nodes = |boundary: &str, ids: &[&str]| {
            let members: String = ids
                .iter()
                .map(|id| format!("    {id}\n"))
                .collect::<Vec<_>>()
                .concat();
            format!("  subgraph {boundary}[\"{boundary}\"]\n{members}  end\n")
        };
        format!(
            "flowchart LR\n{}{}{arrows}{rules}",
            nodes("features", &FEATURES),
            nodes("core", &CORE),
        )
    }

    /// Every feature imports every core node, so every one of the 24 pairs is realised, plus the
    /// `extra` files.
    pub fn sources(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut files: Vec<(String, String)> = Vec::new();
        for id in FEATURES.iter().chain(&CORE) {
            files.push((format!("{id}/.arch"), format!("node: {id}\n")));
        }
        for feature in FEATURES {
            let imports: String = CORE
                .iter()
                .map(|core| format!("import {{ v }} from '../{core}/v';\n"))
                .collect::<Vec<_>>()
                .concat();
            files.push((format!("{feature}/x.ts"), imports));
        }
        for core in CORE {
            files.push((format!("{core}/v.ts"), "export const v = 1;\n".to_owned()));
        }
        for (path, contents) in extra {
            files.push(((*path).to_owned(), (*contents).to_owned()));
        }
        files
    }

    pub fn node_arrows() -> String {
        FEATURES
            .iter()
            .flat_map(|f| CORE.iter().map(move |c| format!("  {f} --> {c}\n")))
            .collect::<Vec<_>>()
            .concat()
    }

    pub fn lint(arrows: &str, rules: &str, extra: &[(&str, &str)]) -> (i32, String) {
        let dir = repo(
            &[
                sources(extra),
                vec![("architecture.mmd".to_owned(), contract(arrows, rules))],
            ]
            .concat(),
        );
        let output = run(&dir, &["lint", "--format", "json"]);
        let mut json = text(&output);
        // why: the temp dir name differs between runs, and the `%% order` line follows the arrows.
        json = json.replace(&*dir.path().to_string_lossy(), "<repo>");
        let mark = "`%% order` on line ";
        if let Some((before, after)) = json.split_once(mark) {
            let rest = after.trim_start_matches(|c: char| c.is_ascii_digit());
            json = format!("{before}{mark}N{rest}");
        }
        (output.status.code().unwrap_or(-1), json)
    }

    /// One `  <id> --> core` line per id.
    pub fn arrows_to_core(ids: &[&str]) -> String {
        ids.iter()
            .map(|id| format!("  {id} --> core\n"))
            .collect::<Vec<_>>()
            .concat()
    }

    /// The findings of a lint run, one `kind file:line` per violation, from its JSON.
    pub fn findings(json: &str) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        value["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                format!(
                    "{} {}:{}",
                    v["kind"].as_str().unwrap(),
                    v["at"]["file"].as_str().unwrap_or("-"),
                    v["at"]["line"]
                )
            })
            .collect()
    }
}

use support::{
    FEATURES, arrows_to_core, contract, findings, git, lint, node_arrows, repo, run, sources, text,
};

#[test]
fn one_boundary_arrow_gives_the_lint_result_of_the_twenty_four_node_arrows() {
    let (code_nodes, nodes) = lint(&node_arrows(), "", &[]);
    let (code_boundary, boundary) = lint("  features --> core\n", "", &[]);
    assert_eq!((code_nodes, code_boundary), (0, 0), "{nodes}\n{boundary}");
    assert_eq!(nodes, boundary);
    assert_eq!(findings(&boundary), Vec::<String>::new());
}

#[test]
fn an_upward_import_is_reported_the_same_with_either_arrow() {
    let up = [("c1/up.ts", "import { x } from '../f1/x';\n")];
    let order = "  %% order: features > core\n";
    let (code_nodes, nodes) = lint(&node_arrows(), order, &up);
    let (code_boundary, boundary) = lint("  features --> core\n", order, &up);
    assert_eq!((code_nodes, code_boundary), (1, 1), "{nodes}");
    assert_eq!(nodes, boundary);
    let kinds = findings(&boundary);
    assert!(
        kinds
            .iter()
            .any(|k| k.starts_with("undeclared-edge c1/up.ts")),
        "{kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k.starts_with("upward-edge c1/up.ts")),
        "{kinds:?}"
    );
}

#[test]
fn without_a_cover_the_pair_stays_undeclared_exactly_as_before() {
    let extra = [("f1/own.ts", "import { x } from '../f2/x';\n")];
    let (code, json) = lint("  features --> core\n", "", &extra);
    assert_eq!(code, 1, "{json}");
    assert!(json.contains("declares no edge f1 --> f2"), "{json}");
    let (code, json) = lint("  features --> core\n  features --> features\n", "", &extra);
    assert_eq!(code, 0, "{json}");
}

#[test]
fn a_boundary_edge_with_no_realised_pair_is_stale_and_one_realised_pair_is_enough() {
    let (code, json) = lint("  core --> features\n", "", &[]);
    assert_eq!(code, 1, "{json}");
    assert!(
        json.contains("edge core --> features is declared but no import realises it"),
        "{json}"
    );
    let one = [("c1/up.ts", "import { x } from '../f1/x';\n")];
    let (_, json) = lint("  features --> core\n  core --> features\n", "", &one);
    assert!(!json.contains("stale-edge"), "{json}");
}

#[test]
fn a_planned_boundary_edge_is_realised_by_any_covered_pair() {
    let (code, json) = lint("  features -.-> core\n", "", &[]);
    assert_eq!(code, 0, "{json}");
    assert!(json.contains("planned-realized"), "{json}");
    assert!(
        json.contains("planned edge features -.-> core is now realised by code"),
        "{json}"
    );
    let (_, json) = lint("  features -.-> features\n  features --> core\n", "", &[]);
    assert!(!json.contains("planned-realized"), "{json}");
}

#[test]
fn restricted_boundary_edges_allow_only_their_sites() {
    let test_site = [("f1/x.test.ts", "import { v } from '../c1/v';\n")];
    // why: the production files import every core node, so a test edge realises none of its pairs.
    let (code, json) = lint("  features -->|test| core\n", "", &test_site);
    assert_eq!(code, 1, "{json}");
    assert!(
        json.contains("declares only a test edge features -->|test| core (line 16)"),
        "{json}"
    );
    let (code, json) = lint("  features -->|type| core\n", "", &[]);
    assert_eq!(code, 1, "{json}");
    assert!(
        json.contains("declares only a type edge features -->|type| core (line 16)"),
        "{json}"
    );
    assert!(
        json.contains(
            "edge features -->|type| core is declared but no type-only import realises it"
        ),
        "{json}"
    );
}

#[test]
fn a_site_is_allowed_when_any_covering_edge_allows_it() {
    let solid_nodes = arrows_to_core(&FEATURES);
    let arrows = format!("  features -->|type, test| core\n{solid_nodes}");
    let (code, json) = lint(&arrows, "", &[]);
    assert_eq!(code, 0, "{json}");
    assert!(json.contains("edge features -->|type, test| core is declared but no type-only or test import realises it"), "{json}");
}

#[test]
fn when_no_covering_edge_allows_a_site_the_winner_is_named_and_the_others_counted() {
    let rest = arrows_to_core(&FEATURES[1..]);
    let arrows = format!("  features -->|test| core\n  f1 -->|type| core\n{rest}");
    let (code, json) = lint(&arrows, "", &[]);
    assert_eq!(code, 1, "{json}");
    assert!(
        json.contains("declares only a type edge f1 -->|type| core (line 17)"),
        "{json}"
    );
    assert!(
        json.contains("one more covering edge does not allow it either"),
        "{json}"
    );
}

#[test]
fn a_node_edge_a_boundary_edge_already_covers_is_a_redundant_warning_naming_the_line() {
    let arrows = "  features --> core\n  f1 --> c1\n  f2 -.-> c1\n  f3 -->|test| c1\n";
    let (code, json) = lint(arrows, "", &[]);
    assert_eq!(code, 0, "warnings do not fail: {json}");
    for pair in ["f1 --> c1", "f2 -.-> c1", "f3 -->|test| c1"] {
        let message =
            format!("edge {pair} is redundant: features --> core (line 16) already covers it");
        assert!(json.contains(&message), "{message}\n{json}");
    }
    let dir = repo(
        &[
            sources(&[]),
            vec![("architecture.mmd".to_owned(), contract(arrows, ""))],
        ]
        .concat(),
    );
    assert_eq!(run(&dir, &["lint", "--strict"]).status.code(), Some(1));
}

#[test]
fn a_narrower_or_dotted_cover_leaves_the_node_edge_alone() {
    let arrows = "  features -->|test| core\n  f1 --> c1\n  features -.-> core\n  f2 --> c1\n";
    let (_, json) = lint(arrows, "", &[]);
    assert!(!json.contains("redundant-edge"), "{json}");
}

#[test]
fn an_upward_boundary_edge_is_a_contract_error_and_a_downward_one_is_fine() {
    let dir = repo(
        &[
            sources(&[]),
            vec![(
                "architecture.mmd".to_owned(),
                contract("  core --> features\n", "  %% order: features > core\n"),
            )],
        ]
        .concat(),
    );
    let output = run(&dir, &["lint"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("points upward"), "{stderr}");
    assert!(stderr.contains("`core` is below `features`"), "{stderr}");
}

#[test]
fn no_cycles_finds_a_cycle_through_a_boundary_edge_and_names_the_lines() {
    let dir = repo(
        &[
            sources(&[]),
            vec![(
                "architecture.mmd".to_owned(),
                contract("  features --> core\n  c1 --> f1\n", "  %% no-cycles\n"),
            )],
        ]
        .concat(),
    );
    let output = run(&dir, &["lint"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("via features --> core, line 16"),
        "{stderr}"
    );
    assert!(stderr.contains("c1 --> f1 (line 17)"), "{stderr}");
}

#[test]
fn brief_counts_the_boundary_edge_and_names_the_edge_behind_each_pair() {
    let dir = repo(
        &[
            sources(&[]),
            vec![(
                "architecture.mmd".to_owned(),
                contract("  features --> core\n", ""),
            )],
        ]
        .concat(),
    );
    let output = run(&dir, &["brief"]);
    let out = text(&output);
    assert!(
        out.contains("10 nodes, 1 edge (1 implemented, 0 planned)"),
        "{out}"
    );
    assert!(
        out.contains("f1 --> c1 (1 import) via features --> core, line 16"),
        "{out}"
    );
    assert!(
        out.contains("boundary edges:\n  features --> core (line 16): 24 of 24 pairs realised\n"),
        "{out}"
    );
    assert!(!out.contains("UNDECLARED"), "{out}");
    let json = text(&run(&dir, &["brief", "--format", "json"]));
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edge = &value["edges"][0];
    assert_eq!(
        edge["via"],
        serde_json::json!({"edge": "features --> core", "line": 16})
    );
    assert_eq!(edge["kind"], "implemented");
    assert_eq!(
        value["boundary_edges"][0],
        serde_json::json!({"from": "features", "to": "core", "kind": "implemented", "line": 16, "covers": 24, "realised": 24})
    );
}

#[test]
fn a_node_edge_has_no_via_and_neighbors_shows_the_edge_behind_the_pair() {
    let dir = repo(
        &[
            sources(&[]),
            vec![(
                "architecture.mmd".to_owned(),
                contract("  features --> core\n  f1 --> c1\n", ""),
            )],
        ]
        .concat(),
    );
    let out = text(&run(&dir, &["neighbors", "f1"]));
    assert!(out.contains("  f1 --> c1 (1 import) uses: v\n"), "{out}");
    assert!(
        out.contains("  f1 --> c2 (1 import) via features --> core, line 16 uses: v\n"),
        "{out}"
    );
    let json = text(&run(&dir, &["neighbors", "f1", "--format", "json"]));
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value["outgoing"][0].get("via").is_none(), "{json}");
    assert_eq!(value["outgoing"][1]["via"]["line"], 16);
}

#[test]
fn diff_reports_a_boundary_edge_like_a_node_edge() {
    let dir = repo(
        &[
            sources(&[]),
            vec![(
                "architecture.mmd".to_owned(),
                contract("  features --> core\n  core -.-> features\n", ""),
            )],
        ]
        .concat(),
    );
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    let edited = contract("  features -.-> core\n  features --> features\n", "");
    std::fs::write(dir.path().join("architecture.mmd"), edited).unwrap();
    let out = text(&run(&dir, &["diff", "--base", "HEAD"]));
    assert_eq!(
        out,
        "+ edge features --> features\n- edge core -.-> features (planned)\n~ edge features --> core => features -.-> core (planned) (implemented -> planned)\n"
    );
    let json = text(&run(&dir, &["diff", "--base", "HEAD", "--format", "json"]));
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        value["added_boundary_edges"],
        serde_json::json!([{"from": "features", "to": "features", "kind": "implemented"}])
    );
    assert_eq!(value["changed_boundary_edges"][0]["new"], "planned");
}

#[test]
fn init_still_writes_node_edges_and_leaves_no_boundary_edge() {
    let dir = repo(&sources(&[]));
    let output = run(&dir, &["init"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(written.contains("f1 --> c1"), "{written}");
    assert!(!written.contains("features -->"), "{written}");
}
