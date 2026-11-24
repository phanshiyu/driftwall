//! Test edges (`-->|test|`) through the CLI: `init` writes them, `lint` enforces them and the
//! read-only commands show them.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

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

    pub fn run(command: &str, dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .arg(command)
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap()
    }

    pub fn text(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

use std::fs;

use support::{repo, run, text};

const SOURCES: [(&str, &str); 6] = [
    ("a/.arch", "node: a\n"),
    ("b/.arch", "node: b\n"),
    ("c/.arch", "node: c\n"),
    ("a/x.ts", "import { y } from '../b/y';\n"),
    (
        "b/y.ts",
        "import { z } from '../c/z';\nexport const y = z;\n",
    ),
    ("b/y.test.ts", "import { z } from '../c/z';\n"),
];

fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = SOURCES.to_vec();
    files.extend_from_slice(extra);
    files.push(("c/z.ts", "export const z = 1;\n"));
    files
}

#[test]
fn init_writes_an_edge_only_tests_use_as_a_test_edge_and_the_draft_lints_strict() {
    let dir = repo(&with(&[("c/z.test.ts", "import { y } from '../b/y';\n")]));
    let init = run("init", &dir, &[]);
    assert!(init.status.success(), "{init:?}");
    let contract = fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(contract.contains("  a --> b\n"), "{contract}");
    assert!(contract.contains("  b --> c\n"), "{contract}");
    assert!(contract.contains("  c -->|test| b\n"), "{contract}");
    let lint = run("lint", &dir, &["--strict"]);
    assert!(lint.status.success(), "{lint:?}");
}

#[test]
fn lint_allows_the_test_site_and_flags_the_production_one() {
    let dir = repo(&with(&[
        ("c/z.test.ts", "import { y } from '../b/y';\n"),
        ("c/w.ts", "import { y } from '../b/y';\n"),
    ]));
    fs::write(
        dir.path().join("architecture.mmd"),
        "flowchart LR\n  a\n  b\n  c\n  a --> b\n  b --> c\n  c -->|test| b\n",
    )
    .unwrap();
    let lint = run("lint", &dir, &["--format", "json"]);
    assert_eq!(lint.status.code(), Some(1), "{lint:?}");
    let out = text(&lint);
    assert_eq!(
        out.matches("\"kind\": \"undeclared-edge\"").count(),
        1,
        "{out}"
    );
    assert!(out.contains("c/w.ts"), "{out}");
    assert!(!out.contains("c/z.test.ts"), "{out}");
}

#[test]
fn a_solid_edge_realised_only_by_tests_warns_and_strict_fails() {
    let dir = repo(&with(&[("c/z.test.ts", "import { y } from '../b/y';\n")]));
    fs::write(
        dir.path().join("architecture.mmd"),
        "flowchart LR\n  a\n  b\n  c\n  a --> b\n  b --> c\n  c --> b\n",
    )
    .unwrap();
    let plain = run("lint", &dir, &[]);
    assert!(plain.status.success(), "{plain:?}");
    assert!(text(&plain).contains("test-only-edge"), "{}", text(&plain));
    let strict = run("lint", &dir, &["--strict"]);
    assert_eq!(strict.status.code(), Some(1), "{strict:?}");
}

#[test]
fn brief_shows_the_test_kind_in_text_and_json() {
    let dir = repo(&with(&[("c/z.test.ts", "import { y } from '../b/y';\n")]));
    fs::write(
        dir.path().join("architecture.mmd"),
        "flowchart LR\n  a\n  b\n  c\n  a --> b\n  b --> c\n  c -->|test| b\n",
    )
    .unwrap();
    let brief = text(&run("brief", &dir, &[]));
    assert!(brief.contains("c -->|test| b"), "{brief}");
    assert!(
        brief.contains("(2 implemented, 0 planned, 1 test)"),
        "{brief}"
    );
    let json = text(&run("brief", &dir, &["--format", "json"]));
    assert!(json.contains("\"kind\": \"test\""), "{json}");
}
