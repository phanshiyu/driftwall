//! End-to-end tests of the grouped text output: one block per edge, JSON still per site.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use super::*;

    pub fn write(root: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
    }

    pub fn lint(root: &Path, extra: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_driftwall"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
            .args(["lint", root.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// `billing` imports `search` from `sites` files, with no edge declared.
    pub fn repo(sites: &[&str]) -> TempDir {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            &[
                (
                    "architecture.mmd",
                    "flowchart LR\n  billing[\"billing\"]\n  search[\"search\"]\n",
                ),
                ("billing/.arch", "node: billing\n"),
                ("search/.arch", "node: search\n"),
                ("search/lib.ts", "export const find = 1;\n"),
                ("search/index.ts", "export const index = 1;\n"),
            ],
        );
        for site in sites {
            write(
                dir.path(),
                &[(
                    &format!("billing/{site}"),
                    "import { find } from '../search/lib';\nexport const use = find;\n",
                )],
            );
        }
        dir
    }
}

use support::{lint, repo, stdout, write};

#[test]
fn three_sites_of_one_edge_are_one_group_with_one_hint() {
    let dir = repo(&["a.ts", "b.ts", "c.ts"]);
    let out = lint(dir.path(), &[]);
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0]
            .contains("a.ts:1: error[undeclared-edge]: `billing` imports `search` in 3 places but"),
        "{text}"
    );
    assert!(
        lines[0].ends_with("declares no edge billing --> search"),
        "{text}"
    );
    assert_eq!(
        lines[1..4]
            .iter()
            .filter(|l| l.starts_with("  at "))
            .count(),
        3
    );
    assert!(lines[1].contains("a.ts:1 (") && lines[1].contains("search/lib.ts)"));
    assert!(lines[2].contains("b.ts:1") && lines[3].contains("c.ts:1"));
    assert_eq!(text.matches("hint:").count(), 1, "{text}");
    assert_eq!(lines[5], "driftwall: 3 errors, 0 warnings (1 group)");
}

#[test]
fn a_test_only_site_is_a_separate_group() {
    let dir = repo(&["a.ts", "b.ts", "a.test.ts", "b.test.ts"]);
    let text = stdout(&lint(dir.path(), &[]));
    assert_eq!(
        text.matches("error[undeclared-edge]: ").count(),
        1,
        "{text}"
    );
    assert_eq!(
        text.matches("error[undeclared-edge] (test code)").count(),
        1,
        "{text}"
    );
    assert_eq!(text.matches("in 2 places").count(), 2, "{text}");
    assert_eq!(text.matches("hint:").count(), 2, "{text}");
    assert!(
        text.ends_with("driftwall: 4 errors, 0 warnings (2 groups)\n"),
        "{text}"
    );
}

#[test]
fn a_single_site_prints_exactly_as_before() {
    let dir = repo(&["a.ts"]);
    let text = stdout(&lint(dir.path(), &[]));
    let expected = "\
billing/a.ts:1: error[undeclared-edge]: `billing` imports `search` (search/lib.ts) but architecture.mmd declares no edge billing --> search
  hint: remove the import, or ask the user to approve adding the edge to the contract
driftwall: 1 error, 0 warnings
";
    assert_eq!(text, expected);
}

#[test]
fn json_stays_per_site_and_gains_a_key() {
    let dir = repo(&["a.ts", "b.ts", "c.ts"]);
    let out = lint(dir.path(), &["--format", "json"]);
    let value: Value = serde_json::from_str(&stdout(&out)).unwrap();
    let found = value["violations"].as_array().unwrap();
    assert_eq!(found.len(), 3);
    for one in found {
        assert_eq!(one["key"], "undeclared-edge billing --> search");
        assert!(one["message"].as_str().unwrap().contains("(search/lib.ts)"));
    }
    assert_eq!(value["summary"]["errors"], 3);
}

#[test]
fn a_baseline_that_allows_less_names_the_counts_once() {
    let dir = repo(&["a.ts"]);
    let written = lint(dir.path(), &["--write-baseline", "debt.txt"]);
    assert_eq!(written.status.code(), Some(0));
    write(
        dir.path(),
        &[
            (
                "billing/b.ts",
                "import { find } from '../search/lib';\nexport const b = find;\n",
            ),
            (
                "billing/c.ts",
                "import { find } from '../search/lib';\nexport const c = find;\n",
            ),
        ],
    );
    let out = lint(dir.path(), &["--baseline", "debt.txt"]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(
        text.matches("the baseline allows 1, found 3").count(),
        1,
        "{text}"
    );
    assert!(
        text.lines()
            .next()
            .unwrap()
            .ends_with("billing --> search; the baseline allows 1, found 3")
    );
    assert_eq!(text.matches("  at ").count(), 3, "{text}");
    assert!(
        text.ends_with("driftwall: 3 errors, 0 warnings (1 group), 0 baselined\n"),
        "{text}"
    );
}

/// Lints a repository of `files` and returns the line of the text that is the header of the group
/// of `kind`, after checking the kind groups once and did not fall back to the generic header.
// why: cfg(test) so clippy's allow-*-in-tests covers it, as for the support module.
#[cfg(test)]
fn header(files: &[(&str, &str)], kind: &str) -> String {
    let dir = TempDir::new().unwrap();
    write(dir.path(), files);
    let text = stdout(&lint(dir.path(), &[]));
    let line = text
        .lines()
        .find(|line| line.contains(&format!("[{kind}]")))
        .unwrap_or_else(|| panic!("no {kind} in:\n{text}"))
        .to_owned();
    assert!(
        !line.contains("(2 places)"),
        "fell back to the generic header: {line}"
    );
    assert_eq!(text.matches(&format!("[{kind}]")).count(), 1, "{text}");
    assert!(text.contains("  at "), "{text}");
    line
}

const IMPORT_SEARCH: &str = "import { find } from '../search/lib';\nexport const x = find;\n";

#[test]
fn private_symbol_header_is_specific() {
    let contract = "flowchart LR\n  billing[\"billing\"]\n  search[\"search\"]\n  billing --> search\n  %% exports search: other\n";
    let files = [
        ("architecture.mmd", contract),
        ("billing/.arch", "node: billing\n"),
        ("search/.arch", "node: search\n"),
        ("search/lib.ts", "export const find = 1;\n"),
        ("billing/a.ts", IMPORT_SEARCH),
        ("billing/b.ts", IMPORT_SEARCH),
    ];
    assert_eq!(
        header(&files, "private-symbol"),
        "billing/a.ts:1: error[private-symbol]: `billing` imports `find` from `search` in 2 places but architecture.mmd lets `search` export only: other"
    );
}

#[test]
fn forbidden_package_header_is_specific() {
    let contract = "flowchart LR\n  billing[\"billing\"]\n  %% forbid-packages billing: lodash\n";
    let files = [
        ("architecture.mmd", contract),
        ("billing/.arch", "node: billing\n"),
        (
            "billing/a.ts",
            "import _ from 'lodash';\nexport const a = _;\n",
        ),
        (
            "billing/b.ts",
            "import _ from 'lodash';\nexport const b = _;\n",
        ),
    ];
    assert_eq!(
        header(&files, "forbidden-package"),
        "billing/a.ts:1: error[forbidden-package]: `billing` imports `lodash` in 2 places (third-party) but architecture.mmd:3 `%% forbid-packages billing` forbids it"
    );
}

#[test]
fn production_imports_test_header_is_specific() {
    let files = [
        (
            "architecture.mmd",
            "flowchart LR\n  billing[\"billing\"]\n  %% tests: isolated\n",
        ),
        ("billing/.arch", "node: billing\n"),
        ("billing/helper.test.ts", "export const h = 1;\n"),
        (
            "billing/a.ts",
            "import { h } from './helper.test';\nimport { h as g } from './helper.test';\nexport const a = [h, g];\n",
        ),
    ];
    assert_eq!(
        header(&files, "production-imports-test"),
        "billing/a.ts:1: error[production-imports-test]: production file `billing/a.ts` imports test file `billing/helper.test.ts` in 2 places, but `%% tests: isolated` on line 3 keeps test code out of production"
    );
}

#[test]
fn upward_edge_header_is_specific() {
    let contract = "flowchart LR\n  subgraph features[\"Features\"]\n    billing[\"billing\"]\n  end\n  subgraph core[\"Core\"]\n    search[\"search\"]\n  end\n  %% order: core > features\n";
    let files = [
        ("architecture.mmd", contract),
        ("billing/.arch", "node: billing\n"),
        ("search/.arch", "node: search\n"),
        ("search/lib.ts", "export const find = 1;\n"),
        ("billing/a.ts", IMPORT_SEARCH),
        ("billing/b.ts", IMPORT_SEARCH),
    ];
    assert_eq!(
        header(&files, "upward-edge"),
        "billing/a.ts:1: error[upward-edge]: `billing` (boundary `features`) imports `search` (boundary `core`) in 2 places but the `%% order` on line 8 puts `core` above `features`"
    );
}

#[test]
fn isolation_breach_header_is_specific() {
    let contract = "flowchart LR\n  subgraph features[\"Features\"]\n    billing[\"billing\"]\n    search[\"search\"]\n  end\n  %% isolated: features\n";
    let files = [
        ("architecture.mmd", contract),
        ("billing/.arch", "node: billing\n"),
        ("search/.arch", "node: search\n"),
        ("search/lib.ts", "export const find = 1;\n"),
        ("billing/a.ts", IMPORT_SEARCH),
        ("billing/b.ts", IMPORT_SEARCH),
    ];
    assert_eq!(
        header(&files, "isolation-breach"),
        "billing/a.ts:1: error[isolation-breach]: boundary `features` is isolated (`%% isolated` on line 6): `billing` (in `billing`) imports `search` (in `search`) in 2 places, and `billing` and `search` may not depend on each other"
    );
}

#[test]
fn denied_edge_header_is_specific() {
    let contract = "flowchart LR\n  subgraph features[\"Features\"]\n    billing[\"billing\"]\n  end\n  subgraph core[\"Core\"]\n    search[\"search\"]\n  end\n  %% order open: features > core\n  %% deny billing: search\n";
    let files = [
        ("architecture.mmd", contract),
        ("billing/.arch", "node: billing\n"),
        ("search/.arch", "node: search\n"),
        ("search/lib.ts", "export const find = 1;\n"),
        ("billing/a.ts", IMPORT_SEARCH),
        ("billing/b.ts", IMPORT_SEARCH),
    ];
    assert_eq!(
        header(&files, "denied-edge"),
        "billing/a.ts:1: error[denied-edge]: `billing` imports `search` in 2 places but architecture.mmd:9 `%% deny billing: search` forbids it"
    );
}
