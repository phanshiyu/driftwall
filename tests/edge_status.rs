//! `brief` and `lint` tell the same story: the status of every edge in `brief` has exactly the
//! finding in `lint` that "Edge status" in `docs/DESIGN.md` gives it, and `lint` has no edge
//! finding `brief` does not show (see `edge_status` in the `model` module).

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::collections::BTreeSet;
    use std::fs;
    use std::process::Command;

    use serde_json::Value;
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

    /// The JSON `driftwall <command> --format json <root>` prints.
    pub fn json(dir: &TempDir, command: &str) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args([command, "--format", "json"])
            .arg(dir.path())
            .output()
            .unwrap();
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub const LIB: &str = "export const x = 1;\nexport type T = number;\n";
    pub const RUNTIME: &str = "import { x } from '../lib/m';\nexport const y = x;\n";
    pub const TYPE_ONLY: &str = "import type { T } from '../lib/m';\nexport type U = T;\n";

    /// One edge per state towards `lib`, and two boundary edges, one stale and one planned-realised.
    pub const CONTRACT: &str = "flowchart LR
      lib
      s-ok
      s-stale
      s-planned
      s-planned-realised
      s-test-only
      s-planned-test-only
      s-undeclared
      s-restricted
      s-type
      s-planned-type
      subgraph quiet[\"Quiet\"]
        q1
        q2
      end
      subgraph busy[\"Busy\"]
        b1
        b2
      end
      s-ok --> lib
      s-stale --> lib
      s-planned -.-> lib
      s-planned-realised -.-> lib
      s-test-only --> lib
      s-planned-test-only -.-> lib
      s-restricted -->|test| lib
      s-type -->|type| lib
      s-planned-type -.->|type| lib
      quiet --> lib
      busy -.-> lib
    ";

    /// The sources of the fixture: each `s-*` node imports `lib` the way its name says.
    pub const SOURCES: [(&str, &str, &str); 9] = [
        ("s-ok", "main.ts", RUNTIME),
        ("s-planned-realised", "main.ts", RUNTIME),
        ("s-test-only", "main.test.ts", RUNTIME),
        ("s-planned-test-only", "main.test.ts", RUNTIME),
        ("s-undeclared", "main.ts", RUNTIME),
        ("s-restricted", "main.ts", RUNTIME),
        ("s-type", "main.ts", TYPE_ONLY),
        ("s-planned-type", "main.ts", TYPE_ONLY),
        ("b1", "main.ts", RUNTIME),
    ];

    pub fn fixture() -> TempDir {
        let nodes = [
            "lib",
            "s-ok",
            "s-stale",
            "s-planned",
            "s-planned-realised",
            "s-test-only",
            "s-planned-test-only",
            "s-undeclared",
            "s-restricted",
            "s-type",
            "s-planned-type",
            "q1",
            "q2",
            "b1",
            "b2",
        ];
        let arch: Vec<(String, String)> = nodes
            .iter()
            .map(|node| (format!("{node}/.arch"), format!("node: {node}\n")))
            .collect();
        let mut files: Vec<(&str, &str)> = vec![("architecture.mmd", CONTRACT), ("lib/m.ts", LIB)];
        files.extend(
            arch.iter()
                .map(|(path, text)| (path.as_str(), text.as_str())),
        );
        let sources: Vec<(String, &str)> = SOURCES
            .iter()
            .map(|(node, file, text)| (format!("{node}/{file}"), *text))
            .collect();
        files.extend(sources.iter().map(|(path, text)| (path.as_str(), *text)));
        // why: a node with no source file is unmapped-free, but needs one file to be read at all.
        let filler: Vec<String> = nodes
            .iter()
            .filter(|node| !SOURCES.iter().any(|(owner, ..)| owner == *node) && **node != "lib")
            .map(|node| format!("{node}/m.ts"))
            .collect();
        files.extend(
            filler
                .iter()
                .map(|path| (path.as_str(), "export const z = 1;\n")),
        );
        repo(&files)
    }

    /// The arrow the contract writes `kind` with, from the JSON name of the kind.
    pub fn arrow(kind: &str) -> &'static str {
        match kind {
            "implemented" => "-->",
            "planned" => "-.->",
            "test" => "-->|test|",
            "planned-test" => "-.->|test|",
            "type" => "-->|type|",
            "planned-type" => "-.->|type|",
            "type-test" => "-->|type, test|",
            "planned-type-test" => "-.->|type, test|",
            other => panic!("unknown kind {other}"),
        }
    }

    /// The finding kind a status has, written out here on purpose: it is the table of the design.
    pub fn finding(status: &str) -> Option<&'static str> {
        match status {
            "ok" | "open" | "planned" => None,
            "undeclared" => Some("undeclared-edge"),
            "stale" => Some("stale-edge"),
            "planned-realized" => Some("planned-realized"),
            "test-only" => Some("test-only-edge"),
            other => panic!("unknown status {other}"),
        }
    }

    pub fn text(value: &Value) -> &str {
        value.as_str().unwrap()
    }

    /// `(finding kind, key)` of every edge `brief` gives a status that has a finding, with the statuses
    /// seen and the pairs that have a declared edge.
    pub fn from_brief(
        brief: &Value,
    ) -> (
        BTreeSet<(String, String)>,
        BTreeSet<String>,
        BTreeSet<String>,
    ) {
        let (mut expected, mut statuses, mut declared) =
            (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for edge in brief["edges"].as_array().unwrap() {
            let status = text(&edge["status"]);
            statuses.insert(status.to_owned());
            let (from, to) = (text(&edge["from"]), text(&edge["to"]));
            if !edge["kind"].is_null() {
                declared.insert(format!("{from} --> {to}"));
            }
            let key = match (edge.get("via"), edge["kind"].as_str()) {
                (Some(via), _) => text(&via["edge"]).to_owned(),
                (None, Some(kind)) => format!("{from} {} {to}", arrow(kind)),
                (None, None) => format!("{from} --> {to}"),
            };
            if let Some(found) = finding(status) {
                expected.insert((found.to_owned(), key));
            }
        }
        for edge in brief["boundary_edges"].as_array().into_iter().flatten() {
            let status = text(&edge["status"]);
            statuses.insert(status.to_owned());
            if let Some(found) = finding(status) {
                let key = format!(
                    "{} {} {}",
                    text(&edge["from"]),
                    arrow(text(&edge["kind"])),
                    text(&edge["to"])
                );
                expected.insert((found.to_owned(), key));
            }
        }
        (expected, statuses, declared)
    }

    /// `(finding kind, key)` of every edge finding in `lint`. An `undeclared-edge` on a pair that has
    /// an edge is about one site that edge does not allow, not about the edge, so it is left out.
    pub fn from_lint(lint: &Value, declared: &BTreeSet<String>) -> BTreeSet<(String, String)> {
        let mut found = BTreeSet::new();
        for violation in lint["violations"].as_array().unwrap() {
            let kind = text(&violation["kind"]);
            if ![
                "undeclared-edge",
                "stale-edge",
                "planned-realized",
                "test-only-edge",
            ]
            .contains(&kind)
            {
                continue;
            }
            let key = text(&violation["key"])
                .strip_prefix(kind)
                .unwrap()
                .trim_start()
                .trim_end_matches(" (test)")
                .to_owned();
            if kind == "undeclared-edge" && declared.contains(&key) {
                continue;
            }
            found.insert((kind.to_owned(), key));
        }
        found
    }
}

use support::*;

#[test]
fn brief_and_lint_agree_on_every_edge() {
    let dir = fixture();
    let brief = json(&dir, "brief");
    let (expected, statuses, declared) = from_brief(&brief);
    // why: the guard is only as good as the fixture, so it must keep one edge in every state.
    for status in [
        "ok",
        "undeclared",
        "stale",
        "planned",
        "planned-realized",
        "test-only",
    ] {
        assert!(
            statuses.contains(status),
            "no edge is {status}: {statuses:?}"
        );
    }
    assert_eq!(expected.len(), 9, "{expected:?}");
    assert_eq!(from_lint(&json(&dir, "lint"), &declared), expected);
}

/// A dotted `%% each` arrow that code realises in one feature: one finding for the arrow, and the
/// pair that realises it shows the same status.
#[test]
fn a_template_arrow_has_one_status_for_all_its_expansions() {
    let dir = repo(&[
        (
            "architecture.mmd",
            "flowchart LR
  subgraph features[\"Features\"]
    subgraph billing[\"Billing\"]
      billing-controller
      billing-service
    end
    subgraph search[\"Search\"]
      search-controller
      search-service
    end
  end
  %% each features: controller -.-> service
  %% map billing-*: src/billing/
  %% map search-*: src/search/
",
        ),
        (
            "src/billing/controller.ts",
            "import { s } from './service';\nexport const c = s;\n",
        ),
        ("src/billing/service.ts", "export const s = 1;\n"),
        ("src/search/controller.ts", "export const c = 1;\n"),
        ("src/search/service.ts", "export const s = 1;\n"),
    ]);
    let brief = json(&dir, "brief");
    let (expected, _, declared) = from_brief(&brief);
    assert_eq!(brief["edges"][0]["status"], "planned-realized", "{brief}");
    assert_eq!(expected.len(), 1, "{expected:?}");
    assert_eq!(from_lint(&json(&dir, "lint"), &declared), expected);
}

#[test]
fn a_restricted_edge_used_from_production_is_stale() {
    let dir = fixture();
    let brief = json(&dir, "brief");
    let edge = brief["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["from"] == "s-restricted")
        .unwrap();
    assert_eq!(
        (edge["kind"].as_str(), edge["status"].as_str()),
        (Some("test"), Some("stale"))
    );
}

/// Nodes in layers under `%% order open`, `app` importing `base` with no edge written.
#[test]
fn a_pair_an_open_chain_allows_is_open_and_has_no_finding() {
    let dir = repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  subgraph top[\"Top\"]\n    app\n  end\n  subgraph bottom[\"Bottom\"]\n    base\n  end\n  %% order open: top > bottom\n",
        ),
        ("app/.arch", "node: app\n"),
        ("base/.arch", "node: base\n"),
        (
            "app/main.ts",
            "import { x } from '../base/m';\nexport const y = x;\n",
        ),
        ("base/m.ts", LIB),
    ]);
    let brief = json(&dir, "brief");
    assert_eq!(brief["edges"][0]["status"], "open");
    assert_eq!(brief["edges"][0]["kind"], "implemented");
    let (expected, _, declared) = from_brief(&brief);
    assert!(expected.is_empty());
    assert!(from_lint(&json(&dir, "lint"), &declared).is_empty());
}
