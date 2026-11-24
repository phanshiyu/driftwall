//! End-to-end tests of the debt baseline: `lint --write-baseline`, `lint --baseline` and
//! `diff --baseline`, on a small repository that has debt.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use super::*;

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

    pub fn write(root: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
    }

    /// Runs `driftwall <args>`.
    pub fn driftwall(args: &[&str]) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(args)
            .output()
            .unwrap()
    }

    pub fn lint(root: &Path, extra: &[&str]) -> Output {
        let mut args = vec!["lint", root.to_str().unwrap()];
        args.extend_from_slice(extra);
        driftwall(&args)
    }

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

    /// Three components that declare no edge at all, and one import of `search` from `billing`.
    pub fn indebted() -> TempDir {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            &[
                (
                    "architecture.mmd",
                    "flowchart LR\n  billing[\"billing\"]\n  search[\"search\"]\n  reports[\"reports\"]\n",
                ),
                ("billing/.arch", "node: billing\n"),
                (
                    "billing/main.ts",
                    "import { find } from '../search/lib';\nexport const bill = find;\n",
                ),
                ("search/.arch", "node: search\n"),
                ("search/lib.ts", "export const find = 1;\n"),
                ("reports/.arch", "node: reports\n"),
                ("reports/main.ts", "export const report = 1;\n"),
            ],
        );
        dir
    }

    pub fn baselined(dir: &TempDir) {
        let written = lint(dir.path(), &["--write-baseline", BASELINE]);
        assert_eq!(code(&written), 0, "{}", stderr(&written));
    }

    /// A second import of `search` from `billing`, in its own file.
    pub fn add_second_site(dir: &TempDir) {
        write(
            dir.path(),
            &[(
                "billing/extra.ts",
                "import { find } from '../search/lib';\nexport const more = find;\n",
            )],
        );
    }

    pub fn commit(dir: &Path, message: &str) {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", message]);
    }

    pub fn diff(dir: &Path, base: &str, extra: &[&str]) -> Output {
        let mut args = vec![
            "diff",
            "--base",
            base,
            dir.to_str().unwrap(),
            "--baseline",
            BASELINE,
        ];
        args.extend_from_slice(extra);
        driftwall(&args)
    }

    /// A contract that trips the rule kinds a plain edge test does not: isolation, layer order, a
    /// budget, a forbidden package, a facade nobody implements and an unused allow-list entry.
    pub fn rule_debt() -> TempDir {
        let dir = TempDir::new().unwrap();
        let contract = "\
    flowchart LR
      subgraph features[\"Features\"]
    billing[\"billing\"]
    search[\"search\"]
      end
      subgraph core[\"Core\"]
    store[\"store\"]
      end
      %% order: core > features
      %% isolated: features
      %% budget billing: files 0
      %% forbid-packages billing: lodash
      %% packages search: left-pad
      %% facade store
    ";
        write(
            dir.path(),
            &[
                ("architecture.mmd", contract),
                ("billing/.arch", "node: billing\n"),
                (
                    "billing/main.ts",
                    "import _ from 'lodash';\nimport { find } from '../search/lib';\nimport { keep } from '../store/lib';\nexport const bill = [_, find, keep];\n",
                ),
                ("search/.arch", "node: search\n"),
                ("search/lib.ts", "export const find = 1;\n"),
                ("store/.arch", "node: store\n"),
                (
                    "store/lib.ts",
                    "import { bill } from '../billing/main';\nexport const keep = bill;\n",
                ),
            ],
        );
        dir
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

use support::{
    add_second_site, baselined, code, commit, diff, git, indebted, lint, rule_debt, stderr, stdout,
    write,
};

const BASELINE: &str = "debt.txt";

#[test]
fn writing_a_baseline_exits_zero_and_says_what_it_wrote() {
    let dir = indebted();
    let output = lint(dir.path(), &["--write-baseline", BASELINE]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "driftwall: wrote debt.txt (1 entry, 1 finding)\n"
    );
    let text = fs::read_to_string(dir.path().join(BASELINE)).unwrap();
    assert!(text.starts_with("# driftwall debt baseline"), "{text}");
    assert!(
        text.ends_with("undeclared-edge 1 billing --> search\n"),
        "{text}"
    );
}

#[test]
fn a_baselined_repository_lints_clean() {
    let dir = indebted();
    assert_eq!(code(&lint(dir.path(), &[])), 1);
    baselined(&dir);
    let output = lint(dir.path(), &["--baseline", BASELINE, "--strict"]);
    assert_eq!(code(&output), 0, "{}", stdout(&output));
    assert_eq!(stdout(&output), "driftwall: clean, 1 baselined\n");
}

#[test]
fn another_site_of_a_baselined_edge_reports_every_site() {
    let dir = indebted();
    baselined(&dir);
    add_second_site(&dir);
    let output = lint(dir.path(), &["--baseline", BASELINE]);
    assert_eq!(code(&output), 1);
    let text = stdout(&output);
    assert_eq!(
        text.matches("the baseline allows 1, found 2").count(),
        2,
        "{text}"
    );
    assert!(text.contains("2 errors"), "{text}");
}

#[test]
fn a_new_undeclared_edge_fails() {
    let dir = indebted();
    baselined(&dir);
    write(
        dir.path(),
        &[(
            "reports/main.ts",
            "import { bill } from '../billing/main';\nexport const report = bill;\n",
        )],
    );
    let output = lint(dir.path(), &["--baseline", BASELINE]);
    assert_eq!(code(&output), 1);
    let text = stdout(&output);
    assert!(text.contains("reports/main.ts:1"), "{text}");
    assert!(text.contains("1 error, 0 warnings, 1 baselined"), "{text}");
}

#[test]
fn fixed_debt_is_a_stale_warning_that_only_strict_fails() {
    let dir = indebted();
    baselined(&dir);
    write(
        dir.path(),
        &[("billing/main.ts", "export const bill = 1;\n")],
    );
    let plain = lint(dir.path(), &["--baseline", BASELINE]);
    assert_eq!(code(&plain), 0, "{}", stdout(&plain));
    let text = stdout(&plain);
    assert!(text.contains("debt.txt:"), "{text}");
    assert!(text.contains("warning[stale-baseline]"), "{text}");
    assert!(text.contains("--write-baseline debt.txt"), "{text}");
    let strict = lint(dir.path(), &["--baseline", BASELINE, "--strict"]);
    assert_eq!(code(&strict), 1);
    // Writing the baseline again shrinks it, which clears the warning.
    baselined(&dir);
    let shrunk = lint(dir.path(), &["--baseline", BASELINE, "--strict"]);
    assert_eq!(code(&shrunk), 0);
}

#[test]
fn a_malformed_baseline_is_a_tool_error_naming_file_and_line() {
    let dir = indebted();
    write(
        dir.path(),
        &[(
            BASELINE,
            "# header\n\nundeclared-edge many billing --> search\n",
        )],
    );
    let output = lint(dir.path(), &["--baseline", BASELINE]);
    assert_eq!(code(&output), 2);
    let err = stderr(&output);
    assert!(err.contains("debt.txt"), "{err}");
    assert!(err.contains("line 3"), "{err}");
}

#[test]
fn a_missing_baseline_is_a_tool_error() {
    let dir = indebted();
    let output = lint(dir.path(), &["--baseline", "nope.txt"]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("nope.txt"));
}

#[test]
fn both_flags_together_are_a_usage_error() {
    let dir = indebted();
    let output = lint(
        dir.path(),
        &["--baseline", BASELINE, "--write-baseline", BASELINE],
    );
    assert_eq!(code(&output), 2);
}

#[test]
fn writing_twice_gives_identical_bytes() {
    let dir = indebted();
    baselined(&dir);
    let first = fs::read(dir.path().join(BASELINE)).unwrap();
    baselined(&dir);
    assert_eq!(first, fs::read(dir.path().join(BASELINE)).unwrap());
}

#[test]
fn json_omits_suppressed_findings_and_reports_the_baseline() {
    let dir = indebted();
    baselined(&dir);
    let output = lint(dir.path(), &["--baseline", BASELINE, "--format", "json"]);
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["violations"].as_array().unwrap().len(), 0);
    assert_eq!(value["baseline"]["file"], "debt.txt");
    assert_eq!(value["baseline"]["suppressed"], 1);
    assert_eq!(value["baseline"]["stale"], 0);
}

#[test]
fn sarif_keeps_suppressed_findings_marked_external() {
    let dir = indebted();
    baselined(&dir);
    let output = lint(dir.path(), &["--baseline", BASELINE, "--format", "sarif"]);
    assert_eq!(code(&output), 0);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let results = value["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["suppressions"][0]["kind"], "external");
}

#[test]
fn diff_reports_debt_added_between_two_commits() {
    let dir = indebted();
    git(dir.path(), &["init", "-q"]);
    baselined(&dir);
    commit(dir.path(), "base");
    // Same debt: nothing added.
    let same = diff(dir.path(), "HEAD", &["--exit-code"]);
    assert_eq!(code(&same), 0, "{}", stderr(&same));
    assert_eq!(stdout(&same), "driftwall: no contract changes\n");
    // One more site of the same edge, written back into the baseline.
    add_second_site(&dir);
    baselined(&dir);
    commit(dir.path(), "grow");
    let grown = diff(dir.path(), "HEAD~1", &["--exit-code"]);
    assert_eq!(code(&grown), 1);
    let text = stdout(&grown);
    assert!(text.contains("debt added:"), "{text}");
    assert!(
        text.contains("undeclared-edge billing --> search: 1 -> 2"),
        "{text}"
    );
    // Without --exit-code the report is informational.
    assert_eq!(code(&diff(dir.path(), "HEAD~1", &[])), 0);
    let json = diff(dir.path(), "HEAD~1", &["--format", "json"]);
    let value: Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(value["debt_added"][0]["new"], 2);
}

#[test]
fn a_baseline_missing_at_the_ref_adds_every_entry() {
    let dir = indebted();
    git(dir.path(), &["init", "-q"]);
    commit(dir.path(), "base");
    baselined(&dir);
    let output = diff(dir.path(), "HEAD", &["--exit-code"]);
    assert_eq!(code(&output), 1);
    assert!(
        stdout(&output).contains("undeclared-edge billing --> search: 0 -> 1"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn every_rule_kind_the_fixture_trips_is_recorded_and_then_suppressed() {
    let dir = rule_debt();
    baselined(&dir);
    let text = fs::read_to_string(dir.path().join(BASELINE)).unwrap();
    for entry in [
        "forbidden-package 1 billing lodash",
        "isolation-breach 1 features: billing --> search",
        "over-budget 1 billing files",
        "stale-facade 1 store",
        "stale-package 1 search left-pad",
        "undeclared-edge 1 billing --> search",
        "upward-edge 1 billing --> store",
    ] {
        assert!(text.contains(entry), "{entry} missing from\n{text}");
    }
    let output = lint(dir.path(), &["--baseline", BASELINE, "--strict"]);
    assert_eq!(code(&output), 0, "{}", stdout(&output));
    assert!(stdout(&output).starts_with("driftwall: clean, "));
}
