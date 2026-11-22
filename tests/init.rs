//! End-to-end tests of `driftwall init`: the baseline it writes must lint clean.

use std::process::Output;

use tempfile::TempDir;

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::collections::BTreeMap;
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

    /// Runs `driftwall <subcommand> <root> <extra...>`.
    pub fn run_on(subcommand: &str, root: &Path, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .arg(subcommand)
            .arg(root)
            .args(extra)
            .output()
            .unwrap()
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

    /// Every file under `root` (relative path with `/`) and its bytes.
    pub fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    let relative = path.strip_prefix(root).unwrap();
                    out.insert(
                        relative.to_string_lossy().replace('\\', "/"),
                        fs::read(&path).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }
    pub fn read(dir: &TempDir, path: &str) -> String {
        fs::read_to_string(dir.path().join(path)).unwrap()
    }

    /// Every `.arch` file in the repo, as sorted `/` paths.
    pub fn arch_files(dir: &TempDir) -> Vec<String> {
        snapshot(dir.path())
            .into_keys()
            .filter(|path| path == ".arch" || path.ends_with("/.arch"))
            .collect()
    }

    /// The node ids of the written `architecture.mmd`.
    pub fn node_ids(dir: &TempDir) -> Vec<String> {
        let contract = driftwall::diagram::parse(&read(dir, "architecture.mmd")).unwrap();
        contract.nodes.keys().map(ToString::to_string).collect()
    }
}

use support::{arch_files, code, node_ids, read, repo, run_on, snapshot, stderr, stdout};

type Files = Vec<(&'static str, &'static str)>;

/// A single `src/` root with several modules and cross imports.
fn ts_src_root() -> Files {
    vec![
        (
            "src/auth/login.ts",
            "import { connect } from '../db/conn';\nexport const login = connect;\n",
        ),
        (
            "src/billing/invoice.ts",
            "import { login } from '../auth/login';\nimport { connect } from '../db/conn';\nexport const invoice = [login, connect];\n",
        ),
        ("src/db/conn.ts", "export const connect = 1;\n"),
        ("src/db/pool/pool.ts", "export const pool = 1;\n"),
        ("README.md", "not source\n"),
    ]
}

/// Root-level files next to directories.
fn ts_flat() -> Files {
    vec![
        (
            "main.ts",
            "import { util } from './lib/util';\nimport { helper } from './lib/helpers/h';\nexport const m = [util, helper];\n",
        ),
        (
            "utils.ts",
            "import { util } from './lib/util';\nexport const u = util;\n",
        ),
        ("lib/util.ts", "export const util = 1;\n"),
        ("lib/helpers/h.ts", "export const helper = 1;\n"),
    ]
}

/// More levels than the automatic depth cuts at.
fn ts_nested() -> Files {
    vec![
        (
            "src/app/ui/button.ts",
            "import { core } from '../core/core';\nexport const button = core;\n",
        ),
        (
            "src/app/core/core.ts",
            "import { shared } from '../../lib/shared/shared';\nexport const core = shared;\n",
        ),
        ("src/lib/shared/shared.ts", "export const shared = 1;\n"),
        ("src/lib/other/other.ts", "export const other = 1;\n"),
    ]
}

/// An ignored directory and a `.driftwallignore`d one, both with imports nobody should see.
fn ts_ignored() -> Files {
    vec![
        (".gitignore", "dist/\n"),
        (".driftwallignore", "legacy/\n"),
        (
            "dist/bundle.ts",
            "import { a } from '../a/a';\nexport const d = a;\n",
        ),
        (
            "legacy/old.ts",
            "import { b } from '../b/b';\nexport const o = b;\n",
        ),
        (
            "a/a.ts",
            "import { b } from '../b/b';\nexport const a = b;\n",
        ),
        ("b/b.ts", "export const b = 1;\n"),
    ]
}

/// Two directories importing each other.
fn ts_cyclic() -> Files {
    vec![
        (
            "a/x.ts",
            "import { y } from '../b/y';\nexport const x = y;\n",
        ),
        (
            "b/y.ts",
            "import { x } from '../a/x';\nexport const y = x;\n",
        ),
    ]
}

/// Python under `src/`, absolute imports rooted there.
fn py_src_root() -> Files {
    vec![
        (
            "src/shop/models/user.py",
            "from shop.db import conn\n\nUSER = conn.CONN\n",
        ),
        ("src/shop/db/conn.py", "CONN = 1\n"),
        (
            "src/shop/services/auth.py",
            "from shop.models.user import USER\n\nAUTH = USER\n",
        ),
    ]
}

/// Python with root-level modules and relative imports.
fn py_flat() -> Files {
    vec![
        ("main.py", "from lib import util\n\nMAIN = util.UTIL\n"),
        ("lib/__init__.py", ""),
        (
            "lib/util.py",
            "from . import helper\n\nUTIL = helper.HELP\n",
        ),
        ("lib/helper.py", "HELP = 1\n"),
        ("tools/run.py", "from lib import util\n\nRUN = util.UTIL\n"),
    ]
}

fn fixtures() -> Vec<(&'static str, Files)> {
    vec![
        ("ts src root", ts_src_root()),
        ("ts flat", ts_flat()),
        ("ts nested", ts_nested()),
        ("ts ignored", ts_ignored()),
        ("ts cyclic", ts_cyclic()),
        ("py src root", py_src_root()),
        ("py flat", py_flat()),
    ]
}

fn init(dir: &TempDir, extra: &[&str]) -> Output {
    run_on("init", dir.path(), extra)
}

#[test]
fn baseline_lints_clean_on_every_fixture() {
    for (name, files) in fixtures() {
        let dir = repo(&files);
        let output = init(&dir, &[]);
        assert_eq!(code(&output), 0, "{name}: {}", stderr(&output));
        assert_eq!(stderr(&output), "", "{name}");

        let lint = run_on("lint", dir.path(), &[]);
        assert_eq!(
            (code(&lint), stdout(&lint)),
            (0, "driftwall: clean\n".to_owned()),
            "{name}\n{}",
            read(&dir, "architecture.mmd")
        );
    }
}

#[test]
fn baseline_lints_clean_under_strict_at_every_depth() {
    for (name, files) in fixtures() {
        for depth in ["0", "1", "2", "3", "5"] {
            let dir = repo(&files);
            let output = init(&dir, &["--depth", depth]);
            assert_eq!(
                code(&output),
                0,
                "{name} depth {depth}: {}",
                stderr(&output)
            );
            let lint = run_on("lint", dir.path(), &["--strict"]);
            assert_eq!(
                stdout(&lint),
                "driftwall: clean\n",
                "{name} depth {depth}\n{}",
                read(&dir, "architecture.mmd")
            );
        }
    }
}

#[test]
fn summary_line_counts_nodes_edges_and_tags() {
    let dir = repo(&ts_src_root());
    let output = init(&dir, &[]);
    assert_eq!(
        stdout(&output),
        "driftwall: wrote architecture.mmd (3 nodes, 3 edges) and 3 .arch files\n"
    );
    let mmd = read(&dir, "architecture.mmd");
    for edge in ["auth --> db", "billing --> auth", "billing --> db"] {
        assert!(mmd.contains(edge), "{edge}\n{mmd}");
    }
    assert_eq!(
        arch_files(&dir),
        ["src/auth/.arch", "src/billing/.arch", "src/db/.arch"]
    );
    assert_eq!(read(&dir, "src/db/.arch"), "node: db\n");
}

#[test]
fn flat_layout_gets_a_root_node() {
    let dir = repo(&ts_flat());
    let output = init(&dir, &[]);
    assert_eq!(code(&output), 0);
    assert_eq!(read(&dir, ".arch"), "node: root\n");
    assert_eq!(arch_files(&dir), [".arch", "lib/.arch"]);
    assert!(read(&dir, "architecture.mmd").contains("root --> lib"));
}

#[test]
fn ignored_directories_are_not_baselined() {
    let dir = repo(&ts_ignored());
    init(&dir, &[]);
    assert_eq!(arch_files(&dir), ["a/.arch", "b/.arch"]);
    let mmd = read(&dir, "architecture.mmd");
    assert!(mmd.contains("a --> b"), "{mmd}");
    assert!(!mmd.contains("dist") && !mmd.contains("legacy"), "{mmd}");
}

#[test]
fn cyclic_directories_get_both_edges() {
    let dir = repo(&ts_cyclic());
    init(&dir, &[]);
    let mmd = read(&dir, "architecture.mmd");
    assert!(mmd.contains("a --> b") && mmd.contains("b --> a"), "{mmd}");
}

#[test]
fn written_contract_parses_and_round_trips() {
    for (name, files) in fixtures() {
        let dir = repo(&files);
        init(&dir, &[]);
        let text = read(&dir, "architecture.mmd");
        let contract = driftwall::diagram::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!contract.nodes.is_empty(), "{name}");
        assert_eq!(driftwall::diagram::render(&contract), text, "{name}");
    }
}

#[test]
fn second_run_without_force_exits_2_and_changes_nothing() {
    let dir = repo(&ts_src_root());
    init(&dir, &[]);
    let before = snapshot(dir.path());
    let output = init(&dir, &[]);
    assert_eq!(code(&output), 2);
    assert_eq!(stdout(&output), "");
    let err = stderr(&output);
    assert!(err.contains("architecture.mmd"), "{err}");
    assert!(err.contains("src/auth/.arch"), "{err}");
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_conflict_leaves_the_repo_untouched_even_when_only_a_tag_exists() {
    let mut files = ts_src_root();
    files.push(("src/db/.arch", "node: mine\n"));
    let dir = repo(&files);
    let before = snapshot(dir.path());
    let output = init(&dir, &[]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("src/db/.arch"));
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn force_overwrites() {
    let mut files = ts_src_root();
    files.push(("architecture.mmd", "stale\n"));
    files.push(("src/db/.arch", "node: mine\n"));
    let dir = repo(&files);
    let output = init(&dir, &["--force"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(read(&dir, "src/db/.arch"), "node: db\n");
    let lint = run_on("lint", dir.path(), &[]);
    assert_eq!(stdout(&lint), "driftwall: clean\n");
}

#[test]
fn no_tags_writes_no_arch_files() {
    let dir = repo(&ts_src_root());
    let output = init(&dir, &["--no-tags"]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "driftwall: wrote architecture.mmd (3 nodes, 3 edges) and no .arch files\n"
    );
    assert_eq!(arch_files(&dir), Vec::<String>::new());
}

#[test]
fn custom_contract_path_is_written_and_lintable() {
    let dir = repo(&ts_src_root());
    let output = init(&dir, &["--contract", "docs/arch.mmd"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(!dir.path().join("architecture.mmd").exists());
    let lint = run_on("lint", dir.path(), &["--contract", "docs/arch.mmd"]);
    assert_eq!(stdout(&lint), "driftwall: clean\n");
}

#[test]
fn depth_changes_the_node_set() {
    let shallow = repo(&ts_nested());
    init(&shallow, &["--depth", "1"]);
    assert_eq!(node_ids(&shallow), ["src"]);

    let auto = repo(&ts_nested());
    init(&auto, &[]);
    assert_eq!(node_ids(&auto), ["app", "lib"]);

    let deep = repo(&ts_nested());
    init(&deep, &["--depth", "3"]);
    assert_eq!(
        node_ids(&deep),
        ["app-core", "app-ui", "lib-other", "lib-shared"]
    );
}

#[test]
fn a_repo_without_sources_exits_2() {
    let dir = repo(&[("README.md", "hello\n"), ("notes/todo.txt", "x\n")]);
    let before = snapshot(dir.path());
    let output = init(&dir, &[]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("no supported source files"));
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_missing_root_exits_2() {
    let dir = TempDir::new().unwrap();
    let output = run_on("init", &dir.path().join("nope"), &[]);
    assert_eq!(code(&output), 2);
}

#[test]
fn output_is_byte_identical_across_fresh_copies() {
    for (name, files) in fixtures() {
        let first = repo(&files);
        let second = repo(&files);
        init(&first, &[]);
        init(&second, &[]);
        assert_eq!(snapshot(first.path()), snapshot(second.path()), "{name}");
    }
}
