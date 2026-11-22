//! End-to-end tests of the `driftwall` binary against TypeScript fixture repositories.

use std::io::Read;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

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

    /// Runs `driftwall <subcommand> <root> <extra...>`.
    pub fn run_on(subcommand: &str, root: &Path, extra: &[&str]) -> Output {
        let mut args = vec![subcommand, root.to_str().unwrap()];
        args.extend_from_slice(extra);
        driftwall(&args)
    }

    /// Runs `driftwall where <file> --root <dir> <extra...>`.
    pub fn locate(dir: &TempDir, file: &str, extra: &[&str]) -> Output {
        let root = dir.path().to_str().unwrap();
        let mut args = vec!["where", file, "--root", root];
        args.extend_from_slice(extra);
        driftwall(&args)
    }

    /// The exit code, which must exist (no signal death).
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

use support::{code, driftwall, locate, repo, run_on, stderr, stdout};

const TWO_NODES: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";

/// Two tagged directories where `a` imports `b`, exactly as the contract says.
fn clean_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        (
            "a/main.ts",
            "import { b } from '../b/lib';\nexport const a = b;\n",
        ),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", "export const b = 1;\n"),
    ]
}

/// `clean_files` plus a `b` -> `a` import the contract does not declare, on line 2.
fn undeclared_files() -> Vec<(&'static str, &'static str)> {
    let mut files = clean_files();
    files.push((
        "b/other.ts",
        "export const x = 1;\nimport { a } from '../a/main';\n",
    ));
    files
}

/// A contract with a solid edge nothing backs: one warning, no errors.
fn stale_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", "export const a = 1;\n"),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", "export const b = 1;\n"),
    ]
}

fn lint(dir: &TempDir, extra: &[&str]) -> Output {
    run_on("lint", dir.path(), extra)
}

#[test]
fn clean_repo_exits_0_and_says_clean() {
    let dir = repo(&clean_files());
    let output = lint(&dir, &[]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "driftwall: clean\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn undeclared_edge_exits_1_and_names_file_and_line() {
    let dir = repo(&undeclared_files());
    let output = lint(&dir, &[]);
    assert_eq!(code(&output), 1);
    let out = stdout(&output);
    assert!(
        out.contains("b/other.ts:2: error[undeclared-edge]"),
        "{out}"
    );
    assert!(out.ends_with("driftwall: 1 error, 0 warnings\n"), "{out}");
    assert_eq!(stderr(&output), "");
}

#[test]
fn warnings_only_exit_0_but_still_print() {
    let dir = repo(&stale_files());
    let output = lint(&dir, &[]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    assert!(out.contains("warning[stale-edge]"), "{out}");
}

#[test]
fn strict_turns_warnings_into_exit_1() {
    let dir = repo(&stale_files());
    let output = lint(&dir, &["--strict"]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).contains("warning[stale-edge]"));
}

#[test]
fn strict_on_a_clean_repo_still_exits_0() {
    let dir = repo(&clean_files());
    assert_eq!(code(&lint(&dir, &["--strict"])), 0);
}

#[test]
fn path_defaults_to_the_current_directory() {
    let dir = repo(&undeclared_files());
    let output = Command::new(env!("CARGO_BIN_EXE_driftwall"))
        .arg("lint")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).contains("b/other.ts:2:"));
}

#[test]
fn contract_is_read_relative_to_path() {
    let mut files = clean_files();
    files.retain(|(path, _)| *path != "architecture.mmd");
    files.push(("docs/arch.mmd", "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n"));
    let dir = repo(&files);
    let output = lint(&dir, &["--contract", "docs/arch.mmd"]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).contains("error[undeclared-edge]"));
}

#[test]
fn json_format_has_version_summary_and_violations() {
    let dir = repo(&undeclared_files());
    let output = lint(&dir, &["--format", "json"]);
    assert_eq!(code(&output), 1);
    let out = stdout(&output);
    assert!(out.ends_with("}\n"), "{out:?}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert!(doc.get("version").is_some());
    assert_eq!(doc["summary"]["errors"], 1);
    assert_eq!(doc["summary"]["warnings"], 0);
    let violation = &doc["violations"][0];
    assert_eq!(violation["kind"], "undeclared-edge");
    assert_eq!(violation["severity"], "error");
    assert_eq!(violation["at"]["file"], "b/other.ts");
    assert_eq!(violation["at"]["line"], 2);
}

#[test]
fn json_format_of_a_clean_repo_has_no_violations() {
    let dir = repo(&clean_files());
    let output = lint(&dir, &["--format", "json"]);
    assert_eq!(code(&output), 0);
    let doc: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(doc["violations"].as_array().unwrap().len(), 0);
}

#[test]
fn sarif_format_is_version_2_1_0() {
    let dir = repo(&undeclared_files());
    let output = lint(&dir, &["--format", "sarif"]);
    assert_eq!(code(&output), 1);
    let out = stdout(&output);
    assert!(out.ends_with('\n'));
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["version"], "2.1.0");
    assert_eq!(doc["runs"][0]["results"].as_array().unwrap().len(), 1);
}

#[test]
fn unknown_format_is_a_usage_error() {
    let dir = repo(&clean_files());
    let output = lint(&dir, &["--format", "yaml"]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
}

#[test]
fn invalid_contract_exits_2_with_the_line_on_stderr() {
    let mut files = clean_files();
    files.retain(|(path, _)| *path != "architecture.mmd");
    files.push((
        "architecture.mmd",
        "flowchart LR\n  a[\"a\"]\n  a ==> b\n  b[\"b\"]\n",
    ));
    let dir = repo(&files);
    let output = lint(&dir, &[]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    let err = stderr(&output);
    assert!(err.starts_with("driftwall: error: "), "{err}");
    assert!(
        err.starts_with("driftwall: error: architecture.mmd: line 3: "),
        "{err}"
    );
    assert!(!err.contains("Backtrace"), "{err}");
    assert!(!err.contains("Error {"), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
}

#[test]
fn missing_contract_exits_2() {
    let dir = repo(&[("a/.arch", "node: a\n"), ("a/main.ts", "export {};\n")]);
    let output = lint(&dir, &[]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    let err = stderr(&output);
    assert!(
        err.starts_with("driftwall: error: cannot read architecture.mmd"),
        "{err}"
    );
    // why: the OS reason must appear exactly once, not once per layer of the error chain.
    assert_eq!(err.matches("No such file").count(), 1, "{err}");
}

#[test]
fn nonexistent_path_exits_2() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nope");
    let output = driftwall(&["lint", missing.to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).starts_with("driftwall: error: "));
}

#[test]
fn path_that_is_a_file_exits_2() {
    let dir = repo(&clean_files());
    let file = dir.path().join("architecture.mmd");
    let output = driftwall(&["lint", file.to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("is not a directory"));
}

#[test]
fn unknown_flag_exits_2() {
    let output = driftwall(&["lint", "--no-such-flag"]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("--no-such-flag"));
}

#[test]
fn unknown_subcommand_exits_2() {
    assert_eq!(code(&driftwall(&["frobnicate"])), 2);
}

#[test]
fn no_subcommand_exits_2() {
    let output = driftwall(&[]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("Usage:"));
}

#[test]
fn help_exits_0_on_stdout() {
    for args in [&["--help"][..], &["lint", "--help"][..]] {
        let output = driftwall(args);
        assert_eq!(code(&output), 0, "{args:?}");
        assert!(stdout(&output).contains("Usage:"), "{args:?}");
        assert_eq!(stderr(&output), "");
    }
}

#[test]
fn lint_help_lists_its_options() {
    let out = stdout(&driftwall(&["lint", "--help"]));
    for flag in ["--contract", "--format", "--strict"] {
        assert!(out.contains(flag), "{flag} missing from {out}");
    }
}

#[test]
fn version_exits_0() {
    let output = driftwall(&["--version"]);
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).starts_with("driftwall "));
}

#[test]
fn output_is_identical_across_runs() {
    let mut files = undeclared_files();
    files.push(("b/more.ts", "import { a } from '../a/main';\n"));
    files.push(("c/loose.ts", "export {};\n"));
    let dir = repo(&files);
    for format in ["text", "json", "sarif"] {
        let first = lint(&dir, &["--format", format]);
        let second = lint(&dir, &["--format", format]);
        assert_eq!(stdout(&first), stdout(&second), "{format}");
        assert_eq!(code(&first), code(&second), "{format}");
    }
}

#[test]
fn a_closed_stdout_pipe_is_quiet_and_keeps_the_exit_code() {
    let mut files = vec![("architecture.mmd", TWO_NODES)];
    let names: Vec<String> = (0..4000).map(|i| format!("loose/file{i}.ts")).collect();
    files.extend(names.iter().map(|name| (name.as_str(), "export {};\n")));
    let dir = repo(&files);

    let mut child = Command::new(env!("CARGO_BIN_EXE_driftwall"))
        .args(["lint", dir.path().to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    let status = child.wait().unwrap();
    assert_eq!(err, "");
    assert_eq!(status.code(), Some(1));
}

/// Two nodes with a nested override, plus a source file nothing owns.
fn where_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", "export const a = 1;\n"),
        ("a/inner/.arch", "node: b\n"),
        ("a/inner/deep/lib.ts", "export const b = 1;\n"),
        ("loose/free.ts", "export const c = 1;\n"),
    ]
}

#[test]
fn where_reports_the_owner_and_the_arch_file() {
    let dir = repo(&where_files());
    let output = locate(&dir, "a/main.ts", &[]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "a/main.ts: a (via a/.arch)\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn where_follows_a_nested_override() {
    let dir = repo(&where_files());
    let output = locate(&dir, "a/inner/deep/lib.ts", &[]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "a/inner/deep/lib.ts: b (via a/inner/.arch)\n"
    );
}

#[test]
fn where_on_an_unmapped_file_exits_1() {
    let dir = repo(&where_files());
    let output = locate(&dir, "loose/free.ts", &[]);
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "loose/free.ts: unmapped\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn where_json_for_a_mapped_file() {
    let dir = repo(&where_files());
    let output = locate(&dir, "a/inner/deep/lib.ts", &["--format", "json"]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    assert_eq!(
        out,
        "{\n  \"file\": \"a/inner/deep/lib.ts\",\n  \"node\": \"b\",\n  \"arch_file\": \"a/inner/.arch\",\n  \"kind\": \"source\"\n}\n"
    );
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["node"], "b");
}

#[test]
fn where_json_for_an_unmapped_file_has_nulls_and_exits_1() {
    let dir = repo(&where_files());
    let output = locate(&dir, "loose/free.ts", &["--format", "json"]);
    assert_eq!(code(&output), 1);
    let doc: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(doc["file"], "loose/free.ts");
    assert!(doc["node"].is_null());
    assert!(doc["arch_file"].is_null());
}

#[test]
fn where_accepts_an_absolute_path_inside_the_root() {
    let dir = repo(&where_files());
    let absolute = dir.path().join("a/main.ts");
    let output = locate(&dir, absolute.to_str().unwrap(), &[]);
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).ends_with(": a (via a/.arch)\n"));
}

#[test]
fn where_root_defaults_to_the_current_directory() {
    let dir = repo(&where_files());
    let output = Command::new(env!("CARGO_BIN_EXE_driftwall"))
        .args(["where", "a/main.ts"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "a/main.ts: a (via a/.arch)\n");
}

#[test]
fn where_outside_the_root_exits_2() {
    let dir = repo(&where_files());
    let other = repo(&where_files());
    let elsewhere = other.path().join("a/main.ts");
    for file in ["../x.ts", "a/../../x.ts", elsewhere.to_str().unwrap()] {
        let output = locate(&dir, file, &[]);
        assert_eq!(code(&output), 2, "{file}");
        assert_eq!(stdout(&output), "", "{file}");
        let err = stderr(&output);
        assert!(err.starts_with("driftwall: error: "), "{err}");
        assert!(err.contains("not inside"), "{err}");
    }
}

#[test]
fn where_with_a_missing_root_exits_2() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nope");
    let output = driftwall(&["where", "a.ts", "--root", missing.to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).starts_with("driftwall: error: cannot access"));
}

#[test]
fn where_with_an_invalid_arch_file_exits_2_with_its_line() {
    let dir = repo(&[("a/.arch", "# tag\nnope\n"), ("a/main.ts", "")]);
    let output = locate(&dir, "a/main.ts", &[]);
    assert_eq!(code(&output), 2);
    let err = stderr(&output);
    assert!(err.contains("a/.arch:2"), "{err}");
}

#[test]
fn where_requires_a_file() {
    assert_eq!(code(&driftwall(&["where"])), 2);
}

#[test]
fn where_rejects_the_sarif_format() {
    let dir = repo(&where_files());
    assert_eq!(code(&locate(&dir, "a/main.ts", &["--format", "sarif"])), 2);
}

#[test]
fn where_help_exits_0() {
    let output = driftwall(&["where", "--help"]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    assert!(out.contains("--root") && out.contains("--format"), "{out}");
}

#[test]
fn where_output_is_identical_across_runs() {
    let dir = repo(&where_files());
    let first = locate(&dir, "a/inner/deep/lib.ts", &["--format", "json"]);
    let second = locate(&dir, "a/inner/deep/lib.ts", &["--format", "json"]);
    assert_eq!(stdout(&first), stdout(&second));
}
