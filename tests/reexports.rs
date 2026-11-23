//! A single file inside a directory that re-exports it can be its own node (see "Re-exports" in
//! `docs/DESIGN.md`): callers write `crate::ui::Tone`, the edge lands on `src/ui/tone.rs`.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    pub fn repo(contract: &str, app: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let files = [
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nversion = \"0.0.0\"\n",
            ),
            ("src/lib.rs", "mod ui;\nmod app;\n"),
            (
                "src/ui/mod.rs",
                "mod tone;\nmod other;\npub use tone::{Hidden, Tone};\npub use other::Other;\n",
            ),
            ("src/ui/tone.rs", "pub struct Tone;\npub struct Hidden;\n"),
            ("src/ui/other.rs", "pub struct Other;\n"),
            ("src/app.rs", app),
            ("docs/architecture.mmd", contract),
        ];
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    pub fn lint(root: &Path) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_driftwall"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
            .args(["lint", "--contract", "docs/architecture.mmd"])
            .current_dir(root)
            .output()
            .unwrap()
    }
}

use support::{lint, repo};

const CONTRACT: &str = "flowchart LR
  app
  ui
  tokens
  app --> tokens
  app --> ui
  ui --> tokens
%% map app: src/app.rs, src/lib.rs
%% map tokens: src/ui/tone.rs
%% map ui: src/ui/mod.rs, src/ui/other.rs
";

#[test]
fn a_file_in_a_reexporting_directory_is_its_own_component() {
    let dir = repo(CONTRACT, "use crate::ui::{Tone, Other};\n");
    let output = lint(dir.path());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(!text.contains("stale-edge"), "{text}");
}

#[test]
fn callers_need_no_edge_to_the_facade_they_never_import_from() {
    let contract = CONTRACT.replace("  app --> ui\n", "");
    let dir = repo(&contract, "use crate::ui::Tone;\n");
    let output = lint(dir.path());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
}

#[test]
fn the_surface_is_judged_at_the_definition() {
    let contract = format!("{CONTRACT}%% exports tokens: Tone\n");
    let dir = repo(&contract, "use crate::ui::Tone;\nuse crate::ui::Hidden;\n");
    let output = lint(dir.path());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("private-symbol"), "{text}");
    assert!(text.contains("Hidden"), "{text}");
    assert!(!text.contains("`Tone`"), "{text}");
}
