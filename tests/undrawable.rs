//! Ids and labels Mermaid cannot parse: the lint warns, and `init` never writes such an id.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    pub fn run(dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
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

    pub fn write(dir: &TempDir, path: &str, text: &str) {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }

    pub fn text(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }
}

use support::{repo, run, text, write};

#[test]
fn an_undrawable_id_warns_and_a_drawable_one_does_not() {
    let dir = repo("flowchart LR\n  a\n  end-user\n  End\n  a --> End\n  a --> end-user\n");
    let out = text(&run(&dir, &["lint"]));
    assert!(out.contains("warning[undrawable-id]"), "{out}");
    assert!(out.contains("`end-user`"), "{out}");
    assert!(!out.contains("`End`"), "{out}");
}

#[test]
fn an_undrawable_label_warns_with_its_line() {
    let dir = repo("flowchart LR\n  a\n  b\n  a -->|uses (x)| b\n");
    let out = text(&run(&dir, &["lint"]));
    assert!(
        out.contains("architecture.mmd:4: warning[undrawable-label]"),
        "{out}"
    );
}

#[test]
fn init_never_writes_an_id_mermaid_cannot_parse() {
    let dir = tempfile::TempDir::new().unwrap();
    for name in ["app", "class", "style", "graph", "end-user"] {
        write(
            &dir,
            &format!("src/{name}/index.ts"),
            "export const x = 1;\n",
        );
    }
    write(&dir, "src/app/main.ts", "import {x} from '../class';\n");
    assert!(run(&dir, &["init"]).status.success());
    let lint = run(&dir, &["lint", "--strict"]);
    assert!(lint.status.success(), "{}", text(&lint));
    let contract = std::fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
    assert!(contract.contains("src-class"), "{contract}");
}
