//! End-to-end tests of `driftwall where` for every kind of file. The census in `brief`,
//! `neighbors`, `lint` and `init` is tested in `blind_report.rs`.

use serde_json::{Value, json};

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    /// Writes `files` (relative path, contents) into a fresh temp directory.
    pub fn repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    /// Runs the binary with `args`.
    pub fn driftwall(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(args)
            .output()
            .unwrap()
    }

    /// Runs `driftwall where <file> --root <dir> <extra...>`.
    pub fn locate(dir: &TempDir, file: &str, extra: &[&str]) -> Output {
        let mut args = vec!["where", file, "--root", dir.path().to_str().unwrap()];
        args.extend_from_slice(extra);
        driftwall(&args)
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
}

use support::{code, locate, repo, stderr, stdout};

const TWO_NODES: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";

/// A repository with one file of every kind `where` tells apart.
fn where_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", TWO_NODES),
        (".gitignore", "a/secret.ts\n"),
        (".driftwallignore", "a/hidden.ts\n"),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", "export const a = 1;\n"),
        ("a/data.json", "{}\n"),
        ("a/view.go", "package a\n"),
        ("a/vendor/lib.js", "export const v = 1;\n"),
        ("a/hidden.ts", "export const h = 1;\n"),
        ("loose/vendor/x.js", "export const x = 1;\n"),
        ("loose/free.ts", "export const c = 1;\n"),
        ("loose/free.json", "{}\n"),
        ("loose/run.pl", "print 1;\n"),
    ]
}

#[test]
fn where_says_what_every_kind_of_file_is() {
    let dir = repo(&where_files());
    for (file, expected, exit) in [
        ("a/main.ts", "a/main.ts: a (via a/.arch)\n", 0),
        (
            "a/data.json",
            "a/data.json: asset of node a (via a/.arch)\n",
            0,
        ),
        (
            "a/view.go",
            "a/view.go: not read (go) in node a (via a/.arch)\n",
            0,
        ),
        (
            "a/vendor/lib.js",
            "a/vendor/lib.js: excluded (vendored), owned by a (via a/.arch)\n",
            0,
        ),
        (
            "loose/vendor/x.js",
            "loose/vendor/x.js: excluded (vendored)\n",
            0,
        ),
        (
            "a/hidden.ts",
            "a/hidden.ts: ignored by .driftwallignore, owned by a (via a/.arch)\n",
            0,
        ),
        ("a/.arch", "a/.arch: tag file\n", 0),
        ("architecture.mmd", "architecture.mmd: contract\n", 0),
        ("loose/free.ts", "loose/free.ts: unmapped\n", 1),
        ("loose/free.json", "loose/free.json: asset, unmapped\n", 0),
        ("loose/run.pl", "loose/run.pl: not read (pl), unmapped\n", 0),
    ] {
        let output = locate(&dir, file, &[]);
        assert_eq!(stdout(&output), expected, "{file}");
        assert_eq!(code(&output), exit, "{file}");
        assert_eq!(stderr(&output), "", "{file}");
    }
}

#[test]
fn where_json_carries_the_kind_and_its_detail() {
    let dir = repo(&where_files());
    let json_of = |file: &str| -> Value {
        let output = locate(&dir, file, &["--format", "json"]);
        serde_json::from_str(&stdout(&output)).unwrap()
    };
    assert_eq!(
        json_of("a/view.go"),
        json!({"file": "a/view.go", "node": "a", "arch_file": "a/.arch",
               "kind": "unread", "detail": "go", "owned_by": "tag"})
    );
    assert_eq!(
        json_of("a/vendor/lib.js"),
        json!({"file": "a/vendor/lib.js", "node": "a", "arch_file": "a/.arch",
               "kind": "excluded", "detail": "vendored", "owned_by": "tag"})
    );
    assert_eq!(
        json_of("a/hidden.ts"),
        json!({"file": "a/hidden.ts", "node": "a", "arch_file": "a/.arch", "kind": "ignored", "owned_by": "tag"})
    );
    assert_eq!(
        json_of("a/.arch"),
        json!({"file": "a/.arch", "node": null, "arch_file": null, "kind": "tag-file"})
    );
    assert_eq!(
        json_of("architecture.mmd"),
        json!({"file": "architecture.mmd", "node": null, "arch_file": null, "kind": "contract"})
    );
    assert_eq!(
        json_of("a/data.json"),
        json!({"file": "a/data.json", "node": "a", "arch_file": "a/.arch", "kind": "asset", "owned_by": "tag"})
    );
}

#[test]
fn where_on_a_file_that_does_not_exist_is_a_tool_error() {
    let dir = repo(&where_files());
    let output = locate(&dir, "a/nope.ts", &[]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "driftwall: error: no such file: a/nope.ts\n"
    );
}

#[test]
fn where_accepts_a_contract_and_rejects_a_bad_one() {
    let mut files = where_files();
    files.push(("other.mmd", TWO_NODES));
    files.push(("broken.mmd", "this is not a diagram\n"));
    let dir = repo(&files);

    let ok = locate(&dir, "a/main.ts", &["--contract", "other.mmd"]);
    assert_eq!(code(&ok), 0);
    assert_eq!(stdout(&ok), "a/main.ts: a (via a/.arch)\n");

    let missing = locate(&dir, "a/main.ts", &["--contract", "missing.mmd"]);
    assert_eq!(code(&missing), 2);
    assert!(
        stderr(&missing).contains("cannot read missing.mmd"),
        "{}",
        stderr(&missing)
    );

    let broken = locate(&dir, "a/main.ts", &["--contract", "broken.mmd"]);
    assert_eq!(code(&broken), 2);
    assert!(
        stderr(&broken).contains("broken.mmd"),
        "{}",
        stderr(&broken)
    );
}
