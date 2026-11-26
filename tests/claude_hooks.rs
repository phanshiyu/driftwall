//! The Claude Code hooks in `integrations/claude-code/hooks/`, run as Claude Code runs them: they
//! find the contract at the root, in `docs/` or by `DRIFTWALL_CONTRACT`, find `driftwall` on PATH
//! or in `node_modules/.bin`, and say on stderr when they skip instead of staying silent.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Output, Stdio};

    use tempfile::TempDir;

    /// A repository with a contract at `contract` that allows `ui --> api`, and code where `ui`
    /// imports `api` and, when `undeclared`, `db` too.
    pub fn repo(contract: &str, undeclared: bool) -> TempDir {
        let dir = TempDir::new().unwrap();
        let write = |path: &str, body: &str| {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, body).unwrap();
        };
        write(
            contract,
            "flowchart LR\n  ui\n  api\n  db\n  ui --> api\n  api --> db\n  %% map ui: src/ui/\n  %% map api: src/api/\n  %% map db: src/db/\n",
        );
        let screen = if undeclared {
            "import \"../api/handler\";\nimport \"../db/store\";\n"
        } else {
            "import \"../api/handler\";\n"
        };
        write("src/ui/screen.ts", screen);
        write("src/api/handler.ts", "import \"../db/store\";\n");
        write("src/db/store.ts", "");
        dir
    }

    /// Links the driftwall binary under test into `node_modules/.bin` of `root`.
    pub fn link_binary(root: &Path) {
        let bin = root.join("node_modules/.bin");
        fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_driftwall"), bin.join("driftwall")).unwrap();
    }

    /// Runs the hook `name` with `root` as the project, a PATH with no driftwall on it, `stdin`
    /// as its input and `extra` environment variables.
    pub fn hook(name: &str, root: &Path, stdin: &str, extra: &[(&str, &str)]) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("integrations/claude-code/hooks")
            .join(name);
        let mut command = Command::new("bash");
        command
            .arg(script)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("CLAUDE_PROJECT_DIR", root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        let written = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        // why: a hook that exits before reading its input closes the pipe; that is not a failure.
        if let Err(error) = written {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }

    pub fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

use support::{hook, link_binary, repo, text};

#[test]
fn stop_lint_finds_a_docs_contract_and_a_node_modules_binary_and_blocks_on_a_violation() {
    let dir = repo("docs/architecture.mmd", true);
    link_binary(dir.path());
    let output = hook("stop-lint.sh", dir.path(), "{}", &[]);
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    assert!(
        text(&output.stderr).contains("undeclared-edge"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn stop_lint_passes_a_clean_repository() {
    let dir = repo("docs/architecture.mmd", false);
    link_binary(dir.path());
    let output = hook("stop-lint.sh", dir.path(), "{}", &[]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

#[test]
fn the_contract_variable_names_any_path() {
    let dir = repo("design/arch.mmd", true);
    link_binary(dir.path());
    let output = hook(
        "stop-lint.sh",
        dir.path(),
        "{}",
        &[("DRIFTWALL_CONTRACT", "design/arch.mmd")],
    );
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    let missing = hook(
        "stop-lint.sh",
        dir.path(),
        "{}",
        &[("DRIFTWALL_CONTRACT", "nowhere.mmd")],
    );
    assert_eq!(missing.status.code(), Some(0));
    assert!(text(&missing.stderr).contains("does not exist"));
}

#[test]
fn a_repository_without_a_contract_is_left_alone() {
    let dir = tempfile::TempDir::new().unwrap();
    for name in ["stop-lint.sh", "session-start.sh"] {
        let output = hook(name, dir.path(), "{}", &[]);
        assert_eq!(output.status.code(), Some(0), "{name}");
        assert!(
            output.stdout.is_empty() && output.stderr.is_empty(),
            "{name}"
        );
    }
}

#[test]
fn a_contract_without_a_binary_says_so_on_stderr() {
    let dir = repo("architecture.mmd", true);
    let stop = hook("stop-lint.sh", dir.path(), "{}", &[]);
    assert_eq!(stop.status.code(), Some(0));
    assert!(text(&stop.stderr).contains("no driftwall binary"));
    let start = hook("session-start.sh", dir.path(), "", &[]);
    assert!(text(&start.stdout).contains("ui --> api"));
    assert!(text(&start.stderr).contains("no driftwall binary"));
}

#[test]
fn session_start_shows_the_contract_and_the_brief() {
    let dir = repo("docs/architecture.mmd", false);
    link_binary(dir.path());
    let output = hook("session-start.sh", dir.path(), "", &[]);
    let out = text(&output.stdout);
    assert!(
        out.contains("Architecture contract (docs/architecture.mmd)"),
        "{out}"
    );
    assert!(out.contains("driftwall brief"), "{out}");
    assert!(out.contains("nodes:"), "{out}");
}
