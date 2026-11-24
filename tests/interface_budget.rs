//! End-to-end tests of the `interface` budget through the `driftwall` binary: the warning, the
//! budget line of `brief` and `neighbors`, the `interface:` block and the JSON.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use super::Value;
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    const CONTRACT: &str = "flowchart LR\n  billing[\"billing\"]\n  api[\"api\"]\n  store[\"store\"]\n  billing --> store\n  api --> store\n";

    /// `billing` and `api` use three symbols of `store` between them, `open` twice.
    pub fn repo(budget: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let files = [
            ("architecture.mmd", format!("{CONTRACT}{budget}")),
            ("store/.arch", "node: store\n".to_owned()),
            (
                "store/db.ts",
                "export const open = 1;\nexport const close = 2;\n".to_owned(),
            ),
            ("store/cache.ts", "export const get = 1;\n".to_owned()),
            ("billing/.arch", "node: billing\n".to_owned()),
            (
                "billing/main.ts",
                "import { open, close } from '../store/db';\n".to_owned(),
            ),
            ("api/.arch", "node: api\n".to_owned()),
            (
                "api/main.ts",
                "import { open } from '../store/db';\nimport { get } from '../store/cache';\n"
                    .to_owned(),
            ),
        ];
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    pub fn run(args: &[&str], root: &Path) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_driftwall"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command.args(args).arg(root).output().unwrap()
    }

    pub fn text(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    pub fn json(args: &[&str], root: &Path) -> Value {
        serde_json::from_str(&text(&run(args, root))).unwrap()
    }
}

use serde_json::Value;
use support::{json, repo, run, text};

const ENTRIES: &str = "\
interface:
  store/db.ts open (2 sites: api 1, billing 1)
  store/cache.ts get (1 site: api 1)
  store/db.ts close (1 site: billing 1)
";

#[test]
fn an_interface_over_its_limit_is_a_warning_and_equal_is_within() {
    let over = repo("  %% budget store: interface 2\n");
    let value = json(&["lint", "--format", "json"], over.path());
    let violations = value["violations"].as_array().unwrap();
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0]["kind"], "over-budget");
    assert_eq!(
        violations[0]["message"],
        "node `store` is over its budget: interface 3 exceeds the limit 2"
    );
    let within = repo("  %% budget store: interface 3\n");
    let value = json(&["lint", "--format", "json"], within.path());
    assert_eq!(value["violations"].as_array().map(Vec::len), Some(0));
}

#[test]
fn brief_and_neighbors_show_the_interface_and_neighbors_lists_the_entries() {
    let dir = repo("  %% budget store: interface 2\n");
    let brief = text(&run(&["brief"], dir.path()));
    assert!(brief.contains("budget: interface 3/2 over\n"), "{brief}");
    let neighbors = text(&run(&["neighbors", "store"], dir.path()));
    assert!(
        neighbors.contains("budget: interface 3/2 over\n"),
        "{neighbors}"
    );
    assert!(neighbors.contains(ENTRIES), "{neighbors}");
}

#[test]
fn the_entries_are_hidden_without_a_limit_unless_all_sites_is_given() {
    let dir = repo("");
    let plain = text(&run(&["neighbors", "store"], dir.path()));
    assert!(!plain.contains("interface:"), "{plain}");
    let all = text(&run(&["neighbors", "store", "--all-sites"], dir.path()));
    assert!(all.contains(ENTRIES), "{all}");
}

#[test]
fn json_has_the_budget_measure_and_the_entries() {
    let dir = repo("  %% budget store: interface 5\n");
    let value = json(&["neighbors", "store", "--format", "json"], dir.path());
    let interface = &value["node"]["budget"]["interface"];
    assert_eq!(interface["limit"], 5);
    assert_eq!(interface["actual"], 3);
    let entries = value["interface"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["file"], "store/db.ts");
    assert_eq!(entries[0]["symbol"], "open");
    assert_eq!(entries[0]["sites"], 2);
    assert_eq!(entries[0]["users"]["billing"], 1);
    let value = json(&["brief", "--format", "json"], dir.path());
    let store = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "store")
        .unwrap();
    assert_eq!(store["budget"]["interface"]["actual"], 3);
}
