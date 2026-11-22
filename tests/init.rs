//! End-to-end tests of `driftwall init`: the baseline it writes must lint clean.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

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

    /// Copies the source tree of this repository, without any `.arch` file.
    pub fn copy_sources(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let path = entry.unwrap().path();
            let target = to.join(path.file_name().unwrap());
            if path.is_dir() {
                copy_sources(&path, &target);
            } else if path.file_name().is_some_and(|name| name != ".arch") {
                fs::copy(&path, &target).unwrap();
            }
        }
    }

    /// The edges of the written `architecture.mmd`, as `from to` pairs.
    pub fn edge_pairs(dir: &TempDir) -> Vec<String> {
        let contract = driftwall::diagram::parse(&read(dir, "architecture.mmd")).unwrap();
        contract
            .edges
            .keys()
            .map(|edge| format!("{} {}", edge.from, edge.to))
            .collect()
    }

    /// The node ids of the written `architecture.mmd`.
    pub fn node_ids(dir: &TempDir) -> Vec<String> {
        let contract = driftwall::diagram::parse(&read(dir, "architecture.mmd")).unwrap();
        contract.nodes.keys().map(ToString::to_string).collect()
    }
}

use support::{
    arch_files, code, copy_sources, edge_pairs, node_ids, read, repo, run_on, snapshot, stderr,
    stdout,
};

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

/// One root-level file next to directories.
fn ts_flat() -> Files {
    vec![
        (
            "main.ts",
            "import { util } from './lib/util';\nimport { helper } from './lib/helpers/h';\nexport const m = [util, helper];\n",
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

/// A flat Rust binary crate: three modules and two directories, with `use crate::` edges.
fn rust_flat() -> Files {
    vec![
        (
            "Cargo.toml",
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
        ),
        (
            "src/main.rs",
            "mod gitlab;\nmod store;\nmod routes;\nmod ui;\n\nuse crate::gitlab::fetch;\nuse crate::store::save;\nuse crate::ui::b::render;\n\nfn main() {\n    save(fetch());\n    render();\n}\n",
        ),
        (
            "src/gitlab.rs",
            "use crate::store::Cache;\n\npub fn fetch() -> Cache {\n    Cache\n}\n",
        ),
        (
            "src/store.rs",
            "pub struct Cache;\n\npub fn save(_c: Cache) {}\n",
        ),
        (
            "src/routes/a.rs",
            "use crate::gitlab::fetch;\n\npub fn handle() {\n    fetch();\n}\n",
        ),
        (
            "src/ui/b.rs",
            "use crate::routes::a::handle;\n\npub fn render() {\n    handle();\n}\n",
        ),
    ]
}

/// A flat TypeScript `src/` with a test file beside its module and one directory.
fn ts_modules() -> Files {
    vec![
        (
            "src/app.ts",
            "import { store } from './store';\nimport { view } from './ui/view';\nexport const app = [store, view];\n",
        ),
        ("src/store.ts", "export const store = 1;\n"),
        (
            "src/store.test.ts",
            "import { store } from './store';\nexport const t = store;\n",
        ),
        ("src/util.ts", "export const util = 1;\n"),
        (
            "src/ui/view.ts",
            "import { util } from '../util';\nexport const view = util;\n",
        ),
    ]
}

/// A flat Python package under the repository root, with a subpackage.
fn py_package() -> Files {
    vec![
        ("pkg/__init__.py", ""),
        (
            "pkg/api.py",
            "from .models import USER\nfrom pkg.util import HELP\n\nAPI = [USER, HELP]\n",
        ),
        ("pkg/models.py", "from .util import HELP\n\nUSER = HELP\n"),
        ("pkg/util.py", "HELP = 1\n"),
        ("pkg/sub/c.py", "from pkg.api import API\n\nC = API\n"),
    ]
}

/// One module next to several directories: the source root keeps directory nodes.
fn ts_one_module() -> Files {
    vec![
        (
            "src/index.ts",
            "import { login } from './auth/login';\nexport const main = login;\n",
        ),
        (
            "src/auth/login.ts",
            "import { connect } from '../db/conn';\nexport const login = connect;\n",
        ),
        ("src/db/conn.ts", "export const connect = 1;\n"),
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
        ("rust flat", rust_flat()),
        ("ts modules", ts_modules()),
        ("py package", py_package()),
        ("ts one module", ts_one_module()),
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
    assert_eq!(read(&dir, ".arch"), "node: root\nscope: here\n");
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
    assert!(
        !err.contains(".arch"),
        "existing tags are decisions, not conflicts: {err}"
    );
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn an_existing_tag_is_honored_not_a_conflict() {
    let mut files = ts_src_root();
    files.push(("src/db/.arch", "node: mine\n"));
    let dir = repo(&files);
    let output = init(&dir, &[]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(read(&dir, "src/db/.arch"), "node: mine\n");
    assert_eq!(
        arch_files(&dir),
        ["src/auth/.arch", "src/billing/.arch", "src/db/.arch"]
    );
    assert_eq!(node_ids(&dir), ["auth", "billing", "mine"]);
    assert_eq!(
        edge_pairs(&dir),
        ["auth mine", "billing auth", "billing mine"]
    );
    let lint = run_on("lint", dir.path(), &["--strict"]);
    assert_eq!(stdout(&lint), "driftwall: clean\n");
}

#[test]
fn force_overwrites_the_contract_but_never_an_existing_tag() {
    let mut files = ts_src_root();
    files.push(("architecture.mmd", "stale\n"));
    files.push(("src/db/.arch", "node: mine\n"));
    let dir = repo(&files);
    let output = init(&dir, &["--force"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(read(&dir, "src/db/.arch"), "node: mine\n");
    let lint = run_on("lint", dir.path(), &[]);
    assert_eq!(stdout(&lint), "driftwall: clean\n");
}

#[test]
fn running_init_again_with_force_rewrites_the_same_contract() {
    let dir = repo(&rust_flat());
    init(&dir, &[]);
    let before = snapshot(dir.path());
    let output = init(&dir, &["--force"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(snapshot(dir.path()), before);
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

#[test]
fn a_flat_rust_crate_gets_one_node_per_module_and_directory() {
    let dir = repo(&rust_flat());
    let output = init(&dir, &[]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(arch_files(&dir), ["src/.arch"]);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(node_ids(&dir), ["gitlab", "main", "routes", "store", "ui"]);
    assert_eq!(
        edge_pairs(&dir),
        [
            "gitlab store",
            "main gitlab",
            "main store",
            "main ui",
            "routes gitlab",
            "ui routes"
        ]
    );
    let lint = run_on("lint", dir.path(), &["--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, "driftwall: clean\n".to_owned())
    );
}

#[test]
fn a_flat_typescript_src_joins_test_files_to_their_module() {
    let dir = repo(&ts_modules());
    init(&dir, &[]);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(node_ids(&dir), ["app", "store", "ui", "util"]);
    assert_eq!(edge_pairs(&dir), ["app store", "app ui", "ui util"]);
}

#[test]
fn a_flat_python_package_names_its_init_after_the_package() {
    let dir = repo(&py_package());
    init(&dir, &[]);
    assert_eq!(arch_files(&dir), ["pkg/.arch"]);
    assert_eq!(read(&dir, "pkg/.arch"), "node: *\n");
    assert_eq!(node_ids(&dir), ["api", "models", "pkg", "sub", "util"]);
    assert_eq!(
        edge_pairs(&dir),
        ["api models", "api util", "models util", "sub api"]
    );
}

#[test]
fn one_module_next_to_directories_keeps_directory_nodes() {
    let dir = repo(&ts_one_module());
    init(&dir, &[]);
    assert_eq!(
        arch_files(&dir),
        ["src/.arch", "src/auth/.arch", "src/db/.arch"]
    );
    assert_eq!(read(&dir, "src/.arch"), "node: src\n");
    assert_eq!(node_ids(&dir), ["auth", "db", "src"]);
}

#[test]
fn an_explicit_depth_keeps_directory_nodes_on_a_flat_crate() {
    let dir = repo(&rust_flat());
    init(&dir, &["--depth", "2"]);
    assert_eq!(read(&dir, "src/.arch"), "node: src\n");
    assert_eq!(node_ids(&dir), ["routes", "src", "ui"]);
}

#[test]
fn a_source_file_with_an_unusable_module_name_falls_back_to_directory_nodes() {
    let mut files = rust_flat();
    files.push(("src/_.rs", "pub fn h() {}\n"));
    let dir = repo(&files);
    init(&dir, &[]);
    assert_eq!(read(&dir, "src/.arch"), "node: src\n");
    let lint = run_on("lint", dir.path(), &["--strict"]);
    assert_eq!(stdout(&lint), "driftwall: clean\n");
}

#[test]
fn lint_reports_a_new_module_the_contract_does_not_declare() {
    let dir = repo(&rust_flat());
    init(&dir, &[]);
    fs::write(dir.path().join("src/extra.rs"), "pub fn e() {}\n").unwrap();
    let lint = run_on("lint", dir.path(), &[]);
    assert_eq!(code(&lint), 1);
    let out = stdout(&lint);
    assert!(out.contains("unknown-node"), "{out}");
    assert!(out.contains("src/.arch:1"), "{out}");
    assert!(out.contains("extra"), "{out}");
}

#[test]
fn lint_reports_an_invalid_derived_name_as_a_tool_error() {
    let dir = repo(&rust_flat());
    init(&dir, &[]);
    fs::write(dir.path().join("src/__.rs"), "pub fn e() {}\n").unwrap();
    let lint = run_on("lint", dir.path(), &[]);
    assert_eq!(code(&lint), 2);
    assert!(stderr(&lint).contains("src/.arch:1"), "{}", stderr(&lint));
}

#[test]
fn where_reports_the_derived_node_and_its_arch_file() {
    let dir = repo(&rust_flat());
    init(&dir, &[]);
    // why: `where` needs the file to exist, and this one is added after `init` ran.
    fs::create_dir_all(dir.path().join("src/gitlab")).unwrap();
    fs::write(dir.path().join("src/gitlab/new.rs"), "").unwrap();
    for (file, expected) in [
        ("src/gitlab.rs", "src/gitlab.rs: gitlab (via src/.arch)\n"),
        (
            "src/routes/a.rs",
            "src/routes/a.rs: routes (via src/.arch)\n",
        ),
        (
            "src/gitlab/new.rs",
            "src/gitlab/new.rs: gitlab (via src/.arch)\n",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(["where", file, "--root"])
            .arg(dir.path())
            .output()
            .unwrap();
        assert_eq!(stdout(&output), expected);
    }
}

#[test]
fn this_repository_baselines_clean_without_its_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = TempDir::new().unwrap();
    copy_sources(&root.join("src"), &dir.path().join("src"));
    fs::copy(root.join("Cargo.toml"), dir.path().join("Cargo.toml")).unwrap();

    let output = init(&dir, &[]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let lint = run_on("lint", dir.path(), &["--strict"]);
    assert_eq!(
        (code(&lint), stdout(&lint)),
        (0, "driftwall: clean\n".to_owned()),
        "{}",
        read(&dir, "architecture.mmd")
    );
    let declared =
        driftwall::diagram::parse(&fs::read_to_string(root.join("architecture.mmd")).unwrap())
            .unwrap();
    let mut expected: Vec<String> = declared
        .nodes
        .keys()
        .map(|id| id.as_str().to_owned())
        .chain(["lib".to_owned()])
        .collect();
    expected.sort();
    assert_eq!(
        node_ids(&dir),
        expected,
        "init must rediscover every node of the hand-written contract, plus lib for src/lib.rs"
    );
}
