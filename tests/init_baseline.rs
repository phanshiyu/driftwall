//! The fresh-baseline invariant: after `init`, `lint` records no `undeclared-owner` skipped
//! reference, because every owner `init`'s tags give a file is declared in its contract.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

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

    pub fn run(subcommand: &str, root: &Path, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .arg(subcommand)
            .arg(root)
            .args(extra)
            .args(["--contract", "architecture.mmd"])
            .output()
            .unwrap()
    }

    /// Initialises `files` and returns the `blind_spots.skipped` counts of the lint that follows.
    pub fn skipped_after_init(files: &[(&str, &str)]) -> Value {
        let dir = repo(files);
        let init = run("init", dir.path(), &[]);
        assert!(init.status.success(), "{init:?}");
        let lint = run("lint", dir.path(), &["--format", "json"]);
        let json: Value = serde_json::from_slice(&lint.stdout).unwrap();
        json.pointer("/blind_spots/skipped").cloned().unwrap()
    }
}

use support::skipped_after_init;

#[test]
fn an_asset_whose_name_differs_from_the_source_only_by_case_is_owned_by_a_declared_node() {
    let skipped = skipped_after_init(&[
        ("src/App.ts", "export const app = 1;\n"),
        ("src/app.css", "body {}\n"),
        (
            "src/main.ts",
            "import './app.css';\nimport { app } from './App';\nexport const main = app;\n",
        ),
    ]);
    assert_eq!(skipped.get("undeclared-owner"), None, "{skipped}");
}

#[test]
fn an_asset_only_directory_and_an_entry_asset_are_owned_by_declared_nodes() {
    let skipped = skipped_after_init(&[
        (
            "src/fonts.ts",
            "import f from './assets/a.woff2';\nexport default f;\n",
        ),
        ("src/assets/a.woff2", ""),
        (
            "src/main.ts",
            "import './index.css';\nimport f from './fonts';\nexport default f;\n",
        ),
        ("src/index.css", "body {}\n"),
    ]);
    assert_eq!(skipped.get("undeclared-owner"), None, "{skipped}");
}
