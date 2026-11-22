//! End-to-end tests of `driftwall brief`, `neighbors` and `diff` against fixture repositories.

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

    pub fn write(root: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
    }

    /// A command with every `GIT_*` variable removed, so a hook that runs the tests cannot point
    /// git at the repository being committed.
    pub fn clean(mut command: Command) -> Command {
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
    }

    /// Runs the binary with `args` from the current directory.
    pub fn driftwall(args: &[&str]) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(args)
            .output()
            .unwrap()
    }

    /// Runs the binary with `args`, from `dir`.
    pub fn driftwall_in(dir: &Path, args: &[&str]) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap()
    }

    /// Runs git in `dir` with a fixed identity and no signing or hooks.
    pub fn git(dir: &Path, args: &[&str]) {
        let output = clean(Command::new("git"))
            .current_dir(dir)
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
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Initialises a repository in `dir` and commits everything in it.
    pub fn commit_all(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "base"]);
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

use support::{code, commit_all, driftwall, driftwall_in, repo, stderr, stdout, write};

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

/// `a` imports `b` and `b` imports `c`, as the contract says. `b` has an edge each way.
fn chain_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "architecture.mmd",
            "flowchart LR\n  a[\"a: first\"]\n  b[\"b: middle\"]\n  c[\"c: last\"]\n  a --> b\n  b --> c\n",
        ),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", "import { b } from '../b/lib';\n"),
        ("b/.arch", "node: b\n"),
        (
            "b/lib.ts",
            "import { c } from '../c/end';\nexport const b = 1;\n",
        ),
        ("c/.arch", "node: c\n"),
        ("c/end.ts", "export const c = 1;\n"),
    ]
}

#[test]
fn brief_on_a_clean_repo_lists_nodes_and_edges() {
    let dir = repo(&clean_files());
    let output = driftwall(&["brief", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    let expected = "\
driftwall: 2 nodes, 1 edges (1 implemented, 0 planned), 0 errors, 0 warnings
nodes:
  a: a (1 file)
  b: b (1 file)
edges:
  a --> b (1 import)
";
    assert_eq!(stdout(&output), expected);
    assert_eq!(stderr(&output), "");
}

#[test]
fn brief_marks_an_undeclared_observed_edge_and_still_exits_0() {
    let mut files = clean_files();
    files.push((
        "b/other.ts",
        "export const x = 1;\nimport { a } from '../a/main';\n",
    ));
    let dir = repo(&files);
    let output = driftwall(&["brief", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    assert!(
        out.starts_with(
            "driftwall: 2 nodes, 1 edges (1 implemented, 0 planned), 1 errors, 0 warnings\n"
        ),
        "{out}"
    );
    assert!(out.contains("  b --> a (1 import) UNDECLARED\n"), "{out}");
    assert!(out.contains("  a --> b (1 import)\n"), "{out}");
}

#[test]
fn brief_draws_planned_edges_dotted_and_counts_them() {
    let dir = repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a -.-> b\n",
        ),
        ("a/.arch", "node: a\n"),
        ("a/main.ts", "export const a = 1;\n"),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", "export const b = 1;\n"),
    ]);
    let output = driftwall(&["brief", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    assert!(
        out.starts_with(
            "driftwall: 2 nodes, 1 edges (0 implemented, 1 planned), 0 errors, 0 warnings\n"
        ),
        "{out}"
    );
    assert!(out.contains("  a -.-> b (0 imports)\n"), "{out}");
}

#[test]
fn brief_json_parses_and_matches_the_text() {
    let dir = repo(&clean_files());
    let root = dir.path().to_str().unwrap();
    let output = driftwall(&["brief", root, "--format", "json"]);
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["version"], json!(1));
    assert_eq!(value["summary"], json!({ "errors": 0, "warnings": 0 }));
    assert_eq!(value["nodes"][0]["id"], json!("a"));
    assert_eq!(value["nodes"][0]["dirs"], json!(["a"]));
    assert_eq!(value["nodes"][0]["files"], json!(1));
    assert_eq!(
        value["edges"],
        json!([{
            "from": "a",
            "to": "b",
            "kind": "implemented",
            "realised": 1,
            "sites": [{ "file": "a/main.ts", "line": 1 }],
        }])
    );
}

#[test]
fn brief_output_is_identical_across_runs() {
    let dir = repo(&chain_files());
    let root = dir.path().to_str().unwrap();
    let first = driftwall(&["brief", root]);
    let second = driftwall(&["brief", root]);
    assert_eq!(stdout(&first), stdout(&second));
}

#[test]
fn brief_honours_the_contract_option() {
    let mut files = clean_files();
    files.retain(|(path, _)| *path != "architecture.mmd");
    files.push(("docs/arch.mmd", TWO_NODES));
    let dir = repo(&files);
    let root = dir.path().to_str().unwrap();
    assert_eq!(code(&driftwall(&["brief", root])), 2);
    let output = driftwall(&["brief", root, "--contract", "docs/arch.mmd"]);
    assert_eq!(code(&output), 0);
    assert!(stdout(&output).starts_with("driftwall: 2 nodes"));
}

#[test]
fn brief_on_an_invalid_contract_is_a_tool_error() {
    let dir = repo(&[("architecture.mmd", "nonsense\n")]);
    let output = driftwall(&["brief", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).starts_with("driftwall: error: "));
}

#[test]
fn brief_on_a_missing_directory_is_a_tool_error() {
    let dir = repo(&[]);
    let missing = dir.path().join("nope");
    let output = driftwall(&["brief", missing.to_str().unwrap()]);
    assert_eq!(code(&output), 2);
}

#[test]
fn neighbors_shows_incoming_and_outgoing_edges_with_sites() {
    let dir = repo(&chain_files());
    let output = driftwall(&["neighbors", "b", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    let expected = "\
node: b
label: b: middle
dirs: b
files: 1
outgoing:
  b --> c (1 import)
    b/lib.ts:1
incoming:
  a --> b (1 import)
    a/main.ts:1
";
    assert_eq!(stdout(&output), expected);
}

#[test]
fn neighbors_includes_undeclared_observed_edges() {
    let mut files = chain_files();
    files.push(("c/back.ts", "import { a } from '../a/main';\n"));
    let dir = repo(&files);
    let output = driftwall(&["neighbors", "a", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    assert!(
        out.contains("incoming:\n  c --> a (1 import) UNDECLARED\n    c/back.ts:1\n"),
        "{out}"
    );
}

#[test]
fn neighbors_shows_at_most_three_example_sites() {
    let mut files = vec![
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", "export const b = 1;\n"),
    ];
    files.extend([
        ("a/one.ts", "import { b } from '../b/lib';\n"),
        ("a/two.ts", "import { b } from '../b/lib';\n"),
        ("a/three.ts", "import { b } from '../b/lib';\n"),
        ("a/four.ts", "import { b } from '../b/lib';\n"),
        ("a/five.ts", "import { b } from '../b/lib';\n"),
    ]);
    let dir = repo(&files);
    let output = driftwall(&["neighbors", "b", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    let expected_incoming = "\
incoming:
  a --> b (5 imports)
    a/five.ts:1
    a/four.ts:1
    a/one.ts:1
    ... 2 more
";
    let out = stdout(&output);
    assert!(out.ends_with(expected_incoming), "{out}");
}

#[test]
fn neighbors_of_an_unknown_node_is_a_tool_error_listing_the_declared_nodes() {
    let dir = repo(&chain_files());
    let output = driftwall(&["neighbors", "ghost", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "driftwall: error: unknown node `ghost`; declared nodes: a, b, c\n"
    );
}

#[test]
fn neighbors_json_parses() {
    let dir = repo(&chain_files());
    let root = dir.path().to_str().unwrap();
    let output = driftwall(&["neighbors", "b", root, "--format", "json"]);
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["version"], json!(1));
    assert_eq!(value["node"]["id"], json!("b"));
    assert_eq!(value["node"]["label"], json!("b: middle"));
    assert_eq!(value["outgoing"][0]["to"], json!("c"));
    assert_eq!(value["outgoing"][0]["kind"], json!("implemented"));
    assert_eq!(
        value["incoming"][0]["sites"],
        json!([{ "file": "a/main.ts", "line": 1 }])
    );
}

#[test]
fn neighbors_needs_a_node_argument() {
    assert_eq!(code(&driftwall(&["neighbors"])), 2);
}

#[test]
fn diff_reports_an_added_edge() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n  b -.-> a\n",
        )],
    );
    let output = driftwall(&["diff", "--base", "HEAD", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "+ edge b -.-> a (planned)\n");
}

#[test]
fn diff_reports_a_removed_edge_and_a_removed_node() {
    let dir = repo(&chain_files());
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n",
        )],
    );
    let output = driftwall(&["diff", "--base", "HEAD", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "- node c\n- edge b --> c\n");
}

#[test]
fn diff_reports_a_kind_change() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a -.-> b\n",
        )],
    );
    let output = driftwall(&["diff", "--base", "HEAD", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "~ edge a --> b => a -.-> b (planned) (implemented -> planned)\n"
    );
}

#[test]
fn diff_without_changes_says_so_and_exits_0_even_with_exit_code() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    let root = dir.path().to_str().unwrap();
    for extra in [&[][..], &["--exit-code"][..]] {
        let mut args = vec!["diff", "--base", "HEAD", root];
        args.extend_from_slice(extra);
        let output = driftwall(&args);
        assert_eq!(code(&output), 0, "{extra:?}");
        assert_eq!(stdout(&output), "driftwall: no contract changes\n");
    }
}

#[test]
fn diff_exit_code_is_1_only_when_asked_and_the_contract_changed() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n  b --> a\n",
        )],
    );
    let root = dir.path().to_str().unwrap();
    let plain = driftwall(&["diff", "--base", "HEAD", root]);
    assert_eq!(code(&plain), 0);
    let gated = driftwall(&["diff", "--base", "HEAD", root, "--exit-code"]);
    assert_eq!(code(&gated), 1);
    assert_eq!(stdout(&gated), stdout(&plain));
    assert_eq!(stderr(&gated), "");
}

#[test]
fn diff_compares_against_an_older_commit() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a -.-> b\n",
        )],
    );
    support::git(dir.path(), &["commit", "-q", "-a", "-m", "second"]);
    let root = dir.path().to_str().unwrap();
    let output = driftwall(&["diff", "--base", "HEAD~1", root]);
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).starts_with("~ edge a --> b"),
        "{}",
        stdout(&output)
    );
    let same = driftwall(&["diff", "--base", "HEAD", root]);
    assert_eq!(stdout(&same), "driftwall: no contract changes\n");
}

#[test]
fn diff_json_parses() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  c[\"c\"]\n  a -.-> b\n",
        )],
    );
    let root = dir.path().to_str().unwrap();
    let output = driftwall(&["diff", "--base", "HEAD", root, "--format", "json"]);
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["version"], json!(1));
    assert_eq!(value["added_nodes"], json!(["c"]));
    assert_eq!(value["removed_edges"], json!([]));
    assert_eq!(
        value["changed_edges"],
        json!([{ "from": "a", "to": "b", "old": "implemented", "new": "planned" }])
    );
}

#[test]
fn diff_with_an_unknown_ref_is_a_tool_error_with_gits_message() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    let output = driftwall(&[
        "diff",
        "--base",
        "no-such-ref",
        dir.path().to_str().unwrap(),
        "--exit-code",
    ]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    let err = stderr(&output);
    assert!(
        err.starts_with("driftwall: error: git show no-such-ref:./architecture.mmd: "),
        "{err}"
    );
    assert!(err.contains("no-such-ref"), "{err}");
}

#[test]
fn diff_outside_a_git_repository_is_a_tool_error() {
    let dir = repo(&clean_files());
    let output = driftwall(&["diff", "--base", "HEAD", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("not a git repository"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn diff_refuses_a_ref_that_looks_like_an_option() {
    let dir = repo(&clean_files());
    commit_all(dir.path());
    let output = driftwall(&["diff", "--base=--output=leak", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("not a usable git ref"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("leak").exists());
}

#[test]
fn diff_treats_a_contract_absent_at_the_ref_as_empty() {
    let dir = repo(&[("README.md", "hello\n")]);
    commit_all(dir.path());
    write(dir.path(), &[("architecture.mmd", TWO_NODES)]);
    let root = dir.path().to_str().unwrap();
    let output = driftwall(&["diff", "--base", "HEAD", root]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "+ node a\n+ node b\n+ edge a --> b\n");
    assert_eq!(
        code(&driftwall(&["diff", "--base", "HEAD", root, "--exit-code"])),
        1
    );
}

#[test]
fn diff_without_a_working_tree_contract_is_a_tool_error() {
    let dir = repo(&[("README.md", "hello\n")]);
    commit_all(dir.path());
    let output = driftwall(&["diff", "--base", "HEAD", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("cannot read architecture.mmd"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn diff_names_the_old_side_when_the_old_contract_is_invalid() {
    let dir = repo(&[("architecture.mmd", "nonsense\n")]);
    commit_all(dir.path());
    write(dir.path(), &[("architecture.mmd", TWO_NODES)]);
    let output = driftwall(&["diff", "--base", "HEAD", dir.path().to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("the old contract is not valid"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn diff_works_from_a_subdirectory_of_the_repository() {
    let dir = repo(&[("sub/architecture.mmd", TWO_NODES)]);
    commit_all(dir.path());
    write(
        dir.path(),
        &[(
            "sub/architecture.mmd",
            "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  b --> a\n",
        )],
    );
    let output = driftwall(&[
        "diff",
        "--base",
        "HEAD",
        dir.path().join("sub").to_str().unwrap(),
    ]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "+ edge b --> a\n- edge a --> b\n");
}

#[test]
fn diff_honours_the_contract_option() {
    let dir = repo(&[("docs/arch.mmd", TWO_NODES)]);
    commit_all(dir.path());
    write(
        dir.path(),
        &[("docs/arch.mmd", "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n")],
    );
    let root = dir.path().to_str().unwrap();
    let output = driftwall(&[
        "diff",
        "--base",
        "HEAD",
        root,
        "--contract",
        "docs/arch.mmd",
    ]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "- edge a --> b\n");
}

#[test]
fn brief_on_this_repository_lists_its_own_nodes() {
    let output = driftwall_in(env!("CARGO_MANIFEST_DIR").as_ref(), &["brief", "."]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.starts_with("driftwall: "), "{out}");
    for node in ["cli", "lint", "model", "report"] {
        assert!(out.contains(&format!("\n  {node}: ")), "{node}: {out}");
    }
    assert!(out.contains("  cli --> lint ("), "{out}");
    assert!(!out.contains("UNDECLARED"), "{out}");
}

#[test]
fn neighbors_lint_on_this_repository_shows_its_edges() {
    let output = driftwall_in(
        env!("CARGO_MANIFEST_DIR").as_ref(),
        &["neighbors", "lint", "."],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.starts_with("node: lint\n"), "{out}");
    assert!(out.contains("dirs: src/lint\n"), "{out}");
    assert!(out.contains("  lint --> model ("), "{out}");
    assert!(out.contains("  cli --> lint ("), "{out}");
}
