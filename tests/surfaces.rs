//! End-to-end tests of public surfaces (`%% exports`) through the `driftwall` binary: the
//! `private-symbol` rule on TypeScript and Rust repositories, and symbols and surfaces in `brief`,
//! `neighbors` and `diff`.

use serde_json::{Value, json};

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    /// Writes `files` (relative path, contents) into a fresh temp directory.
    pub fn repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        write(dir.path(), files);
        dir
    }

    /// Writes `files` (relative path, contents) under `root`, replacing what is there.
    pub fn write(root: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
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

    /// Runs the binary with `args` on the repository at `root`, as its last argument.
    pub fn driftwall(args: &[&str], root: &Path) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(args)
            .arg(root)
            .output()
            .unwrap()
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }
}

use support::{code, driftwall, repo, stdout};

/// `a` imports `b`; `b` declares a surface.
const SURFACE_CONTRACT: &str =
    "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n  %% exports b: Client, fetch\n";

const TS_LIB: &str =
    "export const Client = 1;\nexport const fetch = 2;\nexport const secret = 3;\n";

fn ts_files(contract: &'static str, main: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", contract),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", main),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", TS_LIB),
    ]
}

#[test]
fn brief_lists_the_symbols_each_edge_uses_and_marks_a_surface() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch, Client } from '../b/lib';\n",
    ));
    let output = driftwall(&["brief"], dir.path());
    assert_eq!(code(&output), 0);
    let expected = "\
driftwall: 2 nodes, 1 edges (1 implemented, 0 planned), 0 errors, 0 warnings
nodes:
  a: a (1 file)
  b: b (1 file) exports: Client, fetch
edges:
  a --> b (1 import) uses: Client, fetch
";
    assert_eq!(stdout(&output), expected);
}

#[test]
fn brief_shows_six_symbols_then_counts_the_rest() {
    let dir = repo(&ts_files(
        "flowchart LR\n  a\n  b\n  a --> b\n",
        "import { s1, s2, s3, s4, s5, s6, s7, s8 } from '../b/lib';\n",
    ));
    let output = driftwall(&["brief"], dir.path());
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).contains("  a --> b (1 import) uses: s1, s2, s3, s4, s5, s6 +2 more\n"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn brief_json_carries_symbols_and_exports() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch, Client } from '../b/lib';\nimport * as all from '../b/lib';\n",
    ));
    let output = driftwall(&["brief", "--format", "json"], dir.path());
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(value["nodes"][0].get("exports").is_none());
    assert_eq!(value["nodes"][1]["exports"], json!(["Client", "fetch"]));
    assert_eq!(
        value["edges"][0]["symbols"],
        json!(["Client", "fetch", "*"])
    );
}

#[test]
fn neighbors_lists_symbols_per_edge_and_the_surface() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch } from '../b/lib';\n",
    ));
    let output = driftwall(&["neighbors", "b"], dir.path());
    assert_eq!(code(&output), 0);
    let expected = "\
node: b
label: b
dirs: b
files: 1
exports: Client, fetch
outgoing:
  (none)
incoming:
  a --> b (1 import) uses: fetch
    a/main.ts:1
";
    assert_eq!(stdout(&output), expected);
}

#[test]
fn neighbors_json_carries_symbols_and_exports() {
    let dir = repo(&ts_files(
        SURFACE_CONTRACT,
        "import { fetch } from '../b/lib';\n",
    ));
    let output = driftwall(&["neighbors", "b", "--format", "json"], dir.path());
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["node"]["exports"], json!(["Client", "fetch"]));
    assert_eq!(value["incoming"][0]["symbols"], json!(["fetch"]));
}

#[test]
fn a_side_effect_import_lists_no_symbols() {
    let dir = repo(&ts_files(SURFACE_CONTRACT, "import '../b/lib';\n"));
    let output = driftwall(&["brief"], dir.path());
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).contains("  a --> b (1 import)\n"),
        "{}",
        stdout(&output)
    );
}
