//! End-to-end tests of what `driftwall diff` reports beyond nodes and edges: files that changed
//! owner, rule directives and boundary moves (see "Diff beyond the contract" in `docs/DESIGN.md`).

use serde_json::json;

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

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

    /// Writes `files` (relative path, contents) under `root`, replacing what is there.
    pub fn write(root: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
    }

    pub fn remove(root: &Path, path: &str) {
        fs::remove_file(root.join(path)).unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
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
                // why: newer git starts detached maintenance after a commit; its lock file in
                // `.git/objects` came and went during the "diff writes nothing" snapshot.
                "-c",
                "maintenance.auto=false",
                "-c",
                "gc.auto=0",
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

    /// A repository holding `files`, committed.
    pub fn repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        write(dir.path(), files);
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-q", "-m", "base"]);
        dir
    }

    /// Runs `driftwall diff --base HEAD <extra>` in `root`.
    pub fn diff(root: &Path, extra: &[&str]) -> Output {
        clean(Command::new(env!("CARGO_BIN_EXE_driftwall")))
            .args(["diff", "--base", "HEAD"])
            .args(extra)
            .arg(root)
            .output()
            .unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    /// Every file below `dir`, sorted, with its length: a snapshot to compare before and after.
    pub fn files_under(dir: &Path) -> Vec<(std::path::PathBuf, u64)> {
        let mut found = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(next) = pending.pop() {
            for entry in fs::read_dir(next).unwrap().flatten() {
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    pending.push(entry.path());
                } else {
                    found.push((entry.path(), meta.len()));
                }
            }
        }
        found.sort();
        found
    }

    pub fn json_of(output: &Output) -> serde_json::Value {
        serde_json::from_str(&stdout(output)).unwrap()
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }
}

use support::{code, diff, json_of, remove, repo, stdout, write};

const TAGGED_CONTRACT: &str = "flowchart LR\n  alpha[\"alpha\"]\n  beta[\"beta\"]\n";

fn tagged() -> tempfile::TempDir {
    repo(&[
        ("architecture.mmd", TAGGED_CONTRACT),
        ("alpha/.arch", "node: alpha\n"),
        ("alpha/widget.ts", "export const widget = 1;\n"),
        ("beta/.arch", "node: beta\n"),
        ("beta/gadget.ts", "export const gadget = 2;\n"),
    ])
}

fn mapped_contract(alpha: &str, beta: &str) -> String {
    format!(
        "flowchart LR\n  alpha[\"alpha\"]\n  beta[\"beta\"]\n  %% map alpha: {alpha}\n  %% map beta: {beta}\n"
    )
}

fn mapped() -> tempfile::TempDir {
    repo(&[
        (
            "architecture.mmd",
            &mapped_contract("src/one.ts, src/two.ts", "other/"),
        ),
        ("src/one.ts", "export const one = 1;\n"),
        ("src/two.ts", "export const two = 2;\n"),
        ("other/three.ts", "export const three = 3;\n"),
    ])
}

#[test]
fn a_file_moved_into_another_nodes_directory_is_reported_as_renamed() {
    let dir = tagged();
    std::fs::rename(
        dir.path().join("alpha/widget.ts"),
        dir.path().join("beta/widget.ts"),
    )
    .unwrap();
    let output = diff(dir.path(), &[]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "moved between nodes:\n  beta/widget.ts: alpha -> beta (renamed from alpha/widget.ts)\n"
    );
}

#[test]
fn an_edited_map_glob_reports_mapping_changed() {
    let dir = mapped();
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            &mapped_contract("src/one.ts", "src/two.ts, other/"),
        )],
    );
    let output = diff(dir.path(), &[]);
    assert_eq!(
        stdout(&output),
        "moved between nodes:\n  src/two.ts: alpha -> beta (mapping changed)\n"
    );
}

#[test]
fn a_tag_file_edit_reports_mapping_changed() {
    let dir = tagged();
    write(dir.path(), &[("alpha/.arch", "node: beta\n")]);
    let output = diff(dir.path(), &[]);
    assert_eq!(
        stdout(&output),
        "moved between nodes:\n  alpha/widget.ts: alpha -> beta (mapping changed)\n"
    );
}

#[test]
fn a_file_that_becomes_unmapped_goes_to_none() {
    let dir = mapped();
    write(
        dir.path(),
        &[("architecture.mmd", &mapped_contract("src/one.ts", "other/"))],
    );
    let output = diff(dir.path(), &[]);
    assert_eq!(
        stdout(&output),
        "moved between nodes:\n  src/two.ts: alpha -> (none) (mapping changed)\n"
    );
}

#[test]
fn a_newly_mapped_file_comes_from_none() {
    let dir = repo(&[
        ("architecture.mmd", &mapped_contract("src/one.ts", "other/")),
        ("src/one.ts", "export const one = 1;\n"),
        ("src/four.ts", "export const four = 4;\n"),
    ]);
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            &mapped_contract("src/one.ts, src/four.ts", "other/"),
        )],
    );
    assert_eq!(
        stdout(&diff(dir.path(), &[])),
        "moved between nodes:\n  src/four.ts: (none) -> alpha (mapping changed)\n"
    );
}

#[test]
fn a_rename_and_a_mapping_change_together_name_both_causes() {
    let dir = repo(&[
        ("architecture.mmd", &mapped_contract("src/", "other/")),
        ("src/one.ts", "export const one = 1;\n"),
        ("other/three.ts", "export const three = 3;\n"),
    ]);
    std::fs::create_dir_all(dir.path().join("lib")).unwrap();
    std::fs::rename(dir.path().join("src/one.ts"), dir.path().join("lib/one.ts")).unwrap();
    write(
        dir.path(),
        &[("architecture.mmd", &mapped_contract("src/", "other/, lib/"))],
    );
    assert_eq!(
        stdout(&diff(dir.path(), &[])),
        "moved between nodes:\n  lib/one.ts: alpha -> beta (renamed from src/one.ts, mapping changed)\n"
    );
}

#[test]
fn an_added_or_deleted_file_alone_is_not_a_move() {
    let dir = tagged();
    write(
        dir.path(),
        &[("beta/extra.ts", "export const extra = 3;\n")],
    );
    remove(dir.path(), "alpha/widget.ts");
    let output = diff(dir.path(), &["--exit-code"]);
    assert_eq!(stdout(&output), "driftwall: no contract changes\n");
    assert_eq!(code(&output), 0);
    assert_eq!(
        json_of(&diff(dir.path(), &["--format", "json"]))["moved"],
        json!([])
    );
}

#[test]
fn a_rename_inside_one_node_is_not_a_move() {
    let dir = tagged();
    std::fs::rename(
        dir.path().join("alpha/widget.ts"),
        dir.path().join("alpha/renamed.ts"),
    )
    .unwrap();
    assert_eq!(
        stdout(&diff(dir.path(), &[])),
        "driftwall: no contract changes\n"
    );
}

#[test]
fn diff_writes_nothing_into_the_repository() {
    let dir = tagged();
    write(
        dir.path(),
        &[("beta/fresh.ts", "export const fresh = 5;\n")],
    );
    std::fs::rename(
        dir.path().join("alpha/widget.ts"),
        dir.path().join("beta/widget.ts"),
    )
    .unwrap();
    let before = support::files_under(&dir.path().join(".git"));
    let output = diff(dir.path(), &[]);
    assert!(stdout(&output).contains("renamed from alpha/widget.ts"));
    assert_eq!(support::files_under(&dir.path().join(".git")), before);
}

#[test]
fn exit_code_fails_on_a_move_and_plain_diff_does_not() {
    let dir = tagged();
    std::fs::rename(
        dir.path().join("alpha/widget.ts"),
        dir.path().join("beta/widget.ts"),
    )
    .unwrap();
    assert_eq!(code(&diff(dir.path(), &[])), 0);
    assert_eq!(code(&diff(dir.path(), &["--exit-code"])), 1);
}

const BOUNDED: &str = "flowchart LR\n  subgraph app[\"app\"]\n    web[\"web\"]\n  end\n  subgraph lib[\"lib\"]\n    core[\"core\"]\n    util[\"util\"]\n  end\n  subgraph features[\"features\"]\n    subgraph billing[\"billing\"]\n      billing-controller\n      billing-service\n    end\n    subgraph search[\"search\"]\n      search-controller\n      search-service\n    end\n  end\n";

const DIRECTIVES: [&str; 11] = [
    "%% order: app > lib",
    "%% order open: app > lib",
    "%% isolated: features",
    "%% each features: controller --> service",
    "%% budget core: files 3",
    "%% tests: isolated",
    "%% packages core: left-pad",
    "%% forbid-packages core: left-pad",
    "%% facade util",
    "%% no-cycles",
    "%% no-cycles: boundaries",
];

fn with_directive(directive: &str) -> String {
    format!("{BOUNDED}  {directive}\n")
}

#[test]
fn every_rule_directive_added_is_reported() {
    for directive in DIRECTIVES {
        let dir = repo(&[("architecture.mmd", BOUNDED)]);
        write(
            dir.path(),
            &[("architecture.mmd", &with_directive(directive))],
        );
        let output = diff(dir.path(), &["--exit-code"]);
        assert_eq!(
            stdout(&output),
            format!("rules changed:\n  + {directive}\n"),
            "{directive}"
        );
        assert_eq!(code(&output), 1, "{directive}");
    }
}

#[test]
fn every_rule_directive_removed_is_reported() {
    for directive in DIRECTIVES {
        let dir = repo(&[("architecture.mmd", &with_directive(directive))]);
        write(dir.path(), &[("architecture.mmd", BOUNDED)]);
        let output = diff(dir.path(), &["--exit-code"]);
        assert_eq!(
            stdout(&output),
            format!("rules changed:\n  - {directive}\n"),
            "{directive}"
        );
        assert_eq!(code(&output), 1, "{directive}");
    }
}

#[test]
fn a_directive_that_changes_is_a_removal_and_an_addition() {
    let dir = repo(&[("architecture.mmd", &with_directive("%% order: app > lib"))]);
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            &with_directive("%% order open: app > lib"),
        )],
    );
    assert_eq!(
        stdout(&diff(dir.path(), &[])),
        "rules changed:\n  + %% order open: app > lib\n  - %% order: app > lib\n"
    );
}

#[test]
fn a_reordered_or_respaced_directive_is_not_a_change() {
    let dir = repo(&[("architecture.mmd", &with_directive("%% facade util, core"))]);
    write(
        dir.path(),
        &[(
            "architecture.mmd",
            &with_directive("%%   facade   core,   util"),
        )],
    );
    assert_eq!(
        stdout(&diff(dir.path(), &[])),
        "driftwall: no contract changes\n"
    );
}

#[test]
fn a_node_moving_between_boundaries_is_reported() {
    let dir = repo(&[("architecture.mmd", BOUNDED)]);
    let moved = BOUNDED
        .replace("    util[\"util\"]\n  end\n", "  end\n")
        .replace(
            "    web[\"web\"]\n",
            "    web[\"web\"]\n    util[\"util\"]\n",
        );
    write(dir.path(), &[("architecture.mmd", &moved)]);
    let output = diff(dir.path(), &["--exit-code"]);
    assert_eq!(
        stdout(&output),
        "rules changed:\n  node util moved from boundary lib to app\n"
    );
    assert_eq!(code(&output), 1);
}

#[test]
fn a_change_of_nesting_moves_every_node_inside() {
    let dir = repo(&[("architecture.mmd", BOUNDED)]);
    let nested = BOUNDED
        .replace(
            "  subgraph lib[\"lib\"]\n    core[\"core\"]\n    util[\"util\"]\n  end\n",
            "",
        )
        .replace(
            "  subgraph features[\"features\"]\n",
            "  subgraph features[\"features\"]\n    subgraph lib[\"lib\"]\n      core[\"core\"]\n      util[\"util\"]\n    end\n",
        );
    write(dir.path(), &[("architecture.mmd", &nested)]);
    assert_eq!(
        stdout(&diff(dir.path(), &[])),
        "rules changed:\n  node core moved from boundary lib to features/lib\n  node util moved from boundary lib to features/lib\n"
    );
}

#[test]
fn a_node_leaving_every_boundary_goes_to_none() {
    let dir = repo(&[("architecture.mmd", BOUNDED)]);
    let loose = BOUNDED
        .replace("    web[\"web\"]\n", "")
        .replace("flowchart LR\n", "flowchart LR\n  web[\"web\"]\n");
    write(dir.path(), &[("architecture.mmd", &loose)]);
    assert_eq!(
        stdout(&diff(dir.path(), &[])),
        "rules changed:\n  node web moved from boundary app to (none)\n"
    );
}

#[test]
fn json_has_the_documented_shape_sorted_by_file() {
    let dir = tagged();
    write(dir.path(), &[("beta/zeta.ts", "export const zeta = 9;\n")]);
    std::fs::rename(
        dir.path().join("alpha/widget.ts"),
        dir.path().join("beta/widget.ts"),
    )
    .unwrap();
    write(
        dir.path(),
        &[
            ("beta/.arch", "node: beta\n"),
            ("alpha/.arch", "node: beta\n"),
        ],
    );
    std::fs::write(
        dir.path().join("architecture.mmd"),
        format!("{TAGGED_CONTRACT}  %% no-cycles\n"),
    )
    .unwrap();
    let value = json_of(&diff(dir.path(), &["--format", "json"]));
    assert_eq!(value["rules_added"], json!(["%% no-cycles"]));
    assert_eq!(value["rules_removed"], json!([]));
    assert_eq!(value["boundary_moves"], json!([]));
    assert_eq!(
        value["moved"],
        json!([
            {
                "file": "beta/widget.ts",
                "from_node": "alpha",
                "to_node": "beta",
                "renamed_from": "alpha/widget.ts",
                "mapping_changed": false,
                "cause": "renamed"
            }
        ])
    );
}

#[test]
fn json_marks_a_mapping_change_and_an_unowned_side_with_null() {
    let dir = mapped();
    write(
        dir.path(),
        &[("architecture.mmd", &mapped_contract("src/one.ts", "other/"))],
    );
    let value = json_of(&diff(dir.path(), &["--format", "json"]));
    assert_eq!(
        value["moved"],
        json!([
            {
                "file": "src/two.ts",
                "from_node": "alpha",
                "to_node": null,
                "mapping_changed": true,
                "cause": "mapping-changed"
            }
        ])
    );
}

#[test]
fn a_contract_absent_at_the_ref_does_not_fail() {
    let dir = repo(&[
        ("alpha/.arch", "node: alpha\n"),
        ("alpha/widget.ts", "export const widget = 1;\n"),
    ]);
    write(dir.path(), &[("architecture.mmd", TAGGED_CONTRACT)]);
    assert_eq!(code(&diff(dir.path(), &[])), 0);
}

#[test]
fn a_root_below_the_repository_top_reports_paths_relative_to_it() {
    let dir = repo(&[
        ("pkg/architecture.mmd", TAGGED_CONTRACT),
        ("pkg/alpha/.arch", "node: alpha\n"),
        ("pkg/alpha/widget.ts", "export const widget = 1;\n"),
        ("pkg/alpha/anchor.ts", "export const anchor = 4;\n"),
        ("pkg/beta/.arch", "node: beta\n"),
        ("pkg/beta/gadget.ts", "export const gadget = 2;\n"),
        ("outside.ts", "export const outside = 3;\n"),
    ]);
    let (from, to) = (dir.path().join("pkg/alpha"), dir.path().join("pkg/beta"));
    std::fs::rename(from.join("widget.ts"), to.join("widget.ts")).unwrap();
    std::fs::rename(from.join("anchor.ts"), to.join("anchor.ts")).unwrap();
    let output = diff(&dir.path().join("pkg"), &[]);
    assert_eq!(
        stdout(&output),
        "moved between nodes:\n  beta/anchor.ts: alpha -> beta (renamed from alpha/anchor.ts)\n  beta/widget.ts: alpha -> beta (renamed from alpha/widget.ts)\n"
    );
}
