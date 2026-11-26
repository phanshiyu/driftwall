//! A bare `%%` before the `flowchart` header: Mermaid cannot draw the contract.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    pub fn lint(dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .arg("lint")
            .args(args)
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn repo(contract: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("architecture.mmd"), contract).unwrap();
        dir
    }

    pub fn text(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }
}

use support::{lint, repo, text};

#[test]
fn a_bare_line_before_the_header_warns_and_fails_strict() {
    let dir = repo("%% top\n%%\nflowchart LR\n  a\n");
    let plain = lint(&dir, &[]);
    let out = text(&plain);
    assert!(plain.status.success(), "{out}");
    assert!(out.contains("warning[bare-comment]"), "{out}");
    assert!(out.contains("line 2 of"), "{out}");
    let strict = lint(&dir, &["--strict"]);
    assert!(!strict.status.success(), "{}", text(&strict));
}

#[test]
fn removing_the_line_makes_it_clean() {
    let dir = repo("%% top\n\nflowchart LR\n  a\n");
    let output = lint(&dir, &["--strict"]);
    let out = text(&output);
    assert!(output.status.success(), "{out}");
    assert!(!out.contains("bare-comment"), "{out}");
}
