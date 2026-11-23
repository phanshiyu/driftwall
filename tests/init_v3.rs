//! End-to-end tests of init v3 on fixtures modelled on real repositories: the source directory of
//! the primary unit is split into module nodes, other JavaScript units are split by directory and
//! get prefixed ids, the root group is limited to its own files, and generated output is left out.
//! Every baseline must lint exactly clean.

use std::process::Output;

use driftwall::model::Exclusion;
use tempfile::TempDir;

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
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

    /// Runs `driftwall where <file> --root <root>`.
    pub fn where_is(root: &Path, file: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(["where", file, "--root"])
            .arg(root)
            .output()
            .unwrap()
    }

    /// Output bytes as text.
    pub fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// The text of a repository file.
    pub fn read(dir: &TempDir, path: &str) -> String {
        fs::read_to_string(dir.path().join(path)).unwrap()
    }

    /// The parsed `architecture.mmd`.
    pub fn contract(dir: &TempDir) -> driftwall::model::Contract {
        driftwall::diagram::parse(&read(dir, "architecture.mmd")).unwrap()
    }

    /// Every `.arch` file in the repo, as sorted `/` paths.
    pub fn arch_files(dir: &Path) -> Vec<String> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else if path.file_name().is_some_and(|name| name == ".arch") {
                    let relative = path.strip_prefix(root).unwrap();
                    out.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }
}

use support::{arch_files, contract, read, repo, run_on, text, where_is};

type Files = Vec<(&'static str, &'static str)>;

fn init_ok(dir: &TempDir) {
    let output: Output = run_on("init", dir.path(), &[]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

/// Asserts that `lint` on `dir` is exactly clean, under `--strict`. The blind spots note is not a
/// finding, so its line is left out of the comparison.
fn assert_lints_clean(dir: &TempDir) {
    let lint = run_on("lint", dir.path(), &["--strict"]);
    let findings: String = text(&lint.stdout)
        .lines()
        .filter(|line| !line.starts_with("note: "))
        .fold(String::new(), |out, line| out + line + "\n");
    assert_eq!(
        (lint.status.code(), findings),
        (Some(0), "driftwall: clean\n".to_owned()),
        "{}",
        read(dir, "architecture.mmd")
    );
}

fn node_ids(dir: &TempDir) -> Vec<String> {
    contract(dir)
        .nodes
        .keys()
        .map(ToString::to_string)
        .collect()
}

fn edge_pairs(dir: &TempDir) -> Vec<String> {
    contract(dir)
        .edges
        .keys()
        .map(|edge| format!("{} {}", edge.from, edge.to))
        .collect()
}

/// A root JavaScript app: layered `src/`, an entry file next to the layers, `server/` and
/// `scripts/` beside it, and tooling files in the root.
fn root_app() -> Files {
    vec![
        ("package.json", "{\"name\": \"app\"}\n"),
        ("tsconfig.json", "{}\n"),
        ("vite.config.ts", "export default {};\n"),
        (
            "src/main.tsx",
            "import { page } from './ui/page';\nimport './sw/register';\n\nexport const main = page;\n",
        ),
        ("src/domain/rules.ts", "export const rules = 1;\n"),
        ("src/domain/types.ts", "export type T = number;\n"),
        (
            "src/content/posts.ts",
            "import { rules } from '../domain/rules';\n\nexport const posts = rules;\n",
        ),
        ("src/contract/api.ts", "export const api = 1;\n"),
        ("src/platform/env.ts", "export const env = 1;\n"),
        (
            "src/ui/page.ts",
            "import { posts } from '../content/posts';\nimport { env } from '../platform/env';\n\nexport const page = [posts, env];\n",
        ),
        (
            "src/sw/register.ts",
            "import { api } from '../contract/api';\n\nexport const register = api;\n",
        ),
        (
            "server/index.js",
            "import { helper } from './helper.js';\n\nexport const server = helper;\n",
        ),
        ("server/helper.js", "export const helper = 1;\n"),
        ("scripts/build.mjs", "export const build = 1;\n"),
    ]
}

#[test]
fn a_root_app_splits_src_into_layers_and_keeps_server_and_scripts_as_directories() {
    let dir = repo(&root_app());
    init_ok(&dir);
    assert_eq!(
        node_ids(&dir),
        [
            "content", "contract", "domain", "main", "platform", "root", "scripts", "server", "sw",
            "ui"
        ]
    );
    assert_eq!(
        edge_pairs(&dir),
        [
            "content domain",
            "main sw",
            "main ui",
            "sw contract",
            "ui content",
            "ui platform"
        ]
    );
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(read(&dir, ".arch"), "node: root\nscope: here\n");
    assert_eq!(read(&dir, "server/.arch"), "node: server\n");
    assert_eq!(read(&dir, "scripts/.arch"), "node: scripts\n");
    assert_eq!(
        arch_files(dir.path()),
        [".arch", "scripts/.arch", "server/.arch", "src/.arch"]
    );
    assert_lints_clean(&dir);
}

#[test]
fn a_directory_added_to_the_root_group_later_is_unmapped() {
    let dir = repo(&root_app());
    init_ok(&dir);
    std::fs::create_dir_all(dir.path().join("extras")).unwrap();
    std::fs::write(dir.path().join("extras/new.ts"), "export const n = 1;\n").unwrap();
    let lint = run_on("lint", dir.path(), &[]);
    assert_eq!(lint.status.code(), Some(1), "{}", text(&lint.stdout));
    let out = text(&lint.stdout);
    assert!(out.contains("unmapped-file"), "{out}");
    assert!(out.contains("extras/new.ts"), "{out}");
}

#[test]
fn where_agrees_with_the_scan_for_the_root_group_scope() {
    let dir = repo(&root_app());
    init_ok(&dir);
    std::fs::create_dir_all(dir.path().join("extras")).unwrap();
    // why: `where` needs every file it is asked about to exist, and these are added after `init`.
    for added in [
        "extras/new.ts",
        "src/domain/new.ts",
        "scripts/deep/x.ts",
        "tooling.ts",
    ] {
        let full = dir.path().join(added);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        if !full.exists() {
            std::fs::write(full, "").unwrap();
        }
    }
    for (file, expected, code) in [
        ("vite.config.ts", "vite.config.ts: root (via .arch)\n", 0),
        ("tooling.ts", "tooling.ts: root (via .arch)\n", 0),
        ("extras/new.ts", "extras/new.ts: unmapped\n", 1),
        (
            "src/domain/new.ts",
            "src/domain/new.ts: domain (via src/.arch)\n",
            0,
        ),
        (
            "scripts/deep/x.ts",
            "scripts/deep/x.ts: scripts (via scripts/.arch)\n",
            0,
        ),
    ] {
        let output = where_is(dir.path(), file);
        assert_eq!(text(&output.stdout), expected, "{file}");
        assert_eq!(output.status.code(), Some(code), "{file}");
    }
}

/// A sole Python package under `src/`, with an entry module, a private module and a sub-package,
/// next to `tests/` and `scripts/`.
fn python_package() -> Files {
    vec![
        ("pyproject.toml", "[project]\nname = \"mybot\"\n"),
        ("src/mybot/__init__.py", ""),
        ("src/mybot/__main__.py", "from .bot import run\n\nrun()\n"),
        (
            "src/mybot/bot.py",
            "from ._private import helper\n\n\ndef run():\n    return helper()\n",
        ),
        ("src/mybot/_private.py", "def helper():\n    return 1\n"),
        ("src/mybot/sub/__init__.py", ""),
        ("src/mybot/sub/x.py", "from ..bot import run\n\nX = run\n"),
        (
            "tests/test_bot.py",
            "from mybot.bot import run\n\n\ndef test_run():\n    run()\n",
        ),
        ("scripts/run.py", "print('run')\n"),
    ]
}

#[test]
fn a_sole_python_package_is_decomposed_with_names_that_stay_valid() {
    let dir = repo(&python_package());
    init_ok(&dir);
    assert_eq!(read(&dir, "src/mybot/.arch"), "node: *\n");
    assert_eq!(
        node_ids(&dir),
        ["bot", "mybot", "private", "scripts", "sub", "tests"]
    );
    assert_eq!(
        edge_pairs(&dir),
        ["bot private", "mybot bot", "sub bot", "tests bot"]
    );
    assert_eq!(
        arch_files(dir.path()),
        ["scripts/.arch", "src/mybot/.arch", "tests/.arch"]
    );
    assert_lints_clean(&dir);
}

/// A root package next to a sub-project `ui/` that has its own `package.json` and `tsconfig.json`.
fn root_and_ui() -> Files {
    vec![
        ("package.json", "{\"name\": \"root\"}\n"),
        ("src/a.ts", "export const a = 1;\n"),
        (
            "src/b.ts",
            "import { a } from './a';\n\nexport const b = a;\n",
        ),
        ("ui/package.json", "{\"name\": \"ui\"}\n"),
        ("ui/tsconfig.json", "{}\n"),
        ("ui/vite.config.ts", "export default {};\n"),
        (
            "ui/src/App.tsx",
            "import { Button } from './components/Button';\nimport { store } from './store';\n\nexport const App = [Button, store];\n",
        ),
        ("ui/src/store.tsx", "export const store = 1;\n"),
        (
            "ui/src/components/Button.tsx",
            "import { util } from '../lib/util';\n\nexport const Button = util;\n",
        ),
        ("ui/src/lib/util.ts", "export const util = 1;\n"),
        (
            "ui/src/test/app.test.ts",
            "import { App } from '../App';\n\nexport const t = App;\n",
        ),
    ]
}

#[test]
fn a_sub_project_with_its_own_manifest_is_split_by_directory() {
    let dir = repo(&root_and_ui());
    init_ok(&dir);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(read(&dir, "ui/src/.arch"), "node: ui-src\nscope: here\n");
    assert_eq!(read(&dir, "ui/.arch"), "node: ui\nscope: here\n");
    assert_eq!(
        node_ids(&dir),
        [
            "a",
            "b",
            "ui",
            "ui-components",
            "ui-lib",
            "ui-src",
            "ui-test"
        ]
    );
    assert_eq!(
        edge_pairs(&dir),
        [
            "b a",
            "ui-components ui-lib",
            "ui-src ui-components",
            "ui-test ui-src"
        ]
    );
    assert_lints_clean(&dir);
}

/// A dashboard sub-project: manifest, flat `src/`, and `test/`, `scripts/`, `dev/` beside it.
fn dashboard() -> Files {
    vec![
        (
            "apps/dashboard/package.json",
            "{\"name\": \"dashboard\"}\n",
        ),
        (
            "apps/dashboard/vite.config.ts",
            "export default {};\n",
        ),
        (
            "apps/dashboard/src/app.ts",
            "import { cards } from './cards';\nimport { api } from './api';\n\nexport const app = [cards, api];\n",
        ),
        (
            "apps/dashboard/src/cards.ts",
            "export const cards = 1;\n",
        ),
        (
            "apps/dashboard/src/api.ts",
            "export const api = 1;\n",
        ),
        (
            "apps/dashboard/test/app.test.ts",
            "import { app } from '../src/app';\n\nexport const t = app;\n",
        ),
        (
            "apps/dashboard/scripts/build.mjs",
            "export const build = 1;\n",
        ),
        (
            "apps/dashboard/dev/serve.mjs",
            "export const serve = 1;\n",
        ),
    ]
}

#[test]
fn a_dashboard_unit_splits_src_and_keeps_test_scripts_and_dev_as_directories() {
    let dir = repo(&dashboard());
    init_ok(&dir);
    assert_eq!(
        read(&dir, "apps/dashboard/src/.arch"),
        "node: *\n"
    );
    assert_eq!(
        read(&dir, "apps/dashboard/.arch"),
        "node: dashboard\nscope: here\n"
    );
    assert_eq!(
        node_ids(&dir),
        ["api", "app", "cards", "dashboard", "dev", "scripts", "test"]
    );
    assert_eq!(edge_pairs(&dir), ["app api", "app cards", "test app"]);
    assert_lints_clean(&dir);
}

#[test]
fn a_new_directory_in_a_unit_group_is_unmapped_too() {
    let dir = repo(&dashboard());
    init_ok(&dir);
    std::fs::create_dir_all(dir.path().join("apps/dashboard/e2e")).unwrap();
    std::fs::write(
        dir.path().join("apps/dashboard/e2e/x.ts"),
        "export const x = 1;\n",
    )
    .unwrap();
    let lint = run_on("lint", dir.path(), &[]);
    assert_eq!(lint.status.code(), Some(1));
    assert!(text(&lint.stdout).contains("unmapped-file"));
}

#[test]
fn generated_files_are_excluded_counted_and_do_not_break_the_baseline() {
    let bundle = "var a=1;".repeat(1000);
    let dir = TempDir::new().unwrap();
    for (path, contents) in [
        ("src/a/x.ts", "export const x = 1;\n"),
        (
            "src/b/y.ts",
            "import { x } from '../a/x';\n\nexport const y = x;\n",
        ),
        ("ui/dist/assets/index-4f9a1c.js", bundle.as_str()),
        (
            "apps/mac/Resources/audio-page.js",
            "// Generated from audio.html.\n// Do not edit.\nexport const page = 1;\n",
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    let ownership = driftwall::ownership::scan(dir.path()).unwrap();
    let generated: Vec<String> = ownership
        .excluded
        .get(&Exclusion::Generated)
        .into_iter()
        .flatten()
        .map(|path| path.display().to_string())
        .collect();
    assert_eq!(
        generated,
        [
            "apps/mac/Resources/audio-page.js",
            "ui/dist/assets/index-4f9a1c.js"
        ]
    );
    init_ok(&dir);
    assert_eq!(node_ids(&dir), ["a", "b"]);
    assert_eq!(edge_pairs(&dir), ["b a"]);
    assert_lints_clean(&dir);
}

#[test]
fn a_negated_driftwallignore_pattern_brings_generated_files_back() {
    let dir = repo(&[
        (".driftwallignore", "!/dist/\n"),
        ("src/a/x.ts", "export const x = 1;\n"),
        ("src/b/y.ts", "export const y = 1;\n"),
        ("dist/out/z.ts", "export const z = 1;\n"),
    ]);
    init_ok(&dir);
    assert!(
        node_ids(&dir).iter().any(|id| id.contains("dist")),
        "{}",
        read(&dir, "architecture.mmd")
    );
    assert_lints_clean(&dir);
}

#[test]
fn an_imported_json_file_across_two_nodes_is_an_edge_and_the_baseline_lints_clean() {
    let dir = repo(&[
        (
            "src/a/a.ts",
            "import data from '../b/data.json';\n\nexport const a = data;\n",
        ),
        ("src/b/b.ts", "export const b = 1;\n"),
        ("src/b/data.json", "{\"k\": 1}\n"),
    ]);
    init_ok(&dir);
    assert_eq!(node_ids(&dir), ["a", "b"]);
    assert_eq!(edge_pairs(&dir), ["a b"]);
    assert_lints_clean(&dir);
}

#[test]
fn an_asset_in_a_directory_no_tag_covers_makes_no_edge() {
    let dir = repo(&[
        (
            "src/a/a.ts",
            "import data from '../../data/data.json';\n\nexport const a = data;\n",
        ),
        ("src/b/b.ts", "export const b = 1;\n"),
        ("data/data.json", "{}\n"),
    ]);
    init_ok(&dir);
    assert_eq!(node_ids(&dir), ["a", "b"]);
    assert_eq!(edge_pairs(&dir), Vec::<String>::new());
    assert_lints_clean(&dir);
}

#[test]
fn the_same_repository_always_gets_the_same_baseline() {
    for files in [root_app(), root_and_ui(), python_package(), dashboard()] {
        let first = repo(&files);
        let second = repo(&files);
        init_ok(&first);
        init_ok(&second);
        assert_eq!(
            read(&first, "architecture.mmd"),
            read(&second, "architecture.mmd")
        );
        assert_eq!(arch_files(first.path()), arch_files(second.path()));
    }
}

#[test]
fn an_import_of_an_excluded_file_is_an_edge_init_declares() {
    let dir = repo(&[
        (
            "src/a/x.mjs",
            "import {g} from './g.mjs'; import {h} from '../b/h.mjs';\n",
        ),
        ("src/a/g.mjs", "// do not edit\nexport const g = 1;\n"),
        ("src/b/h.mjs", "// @generated\nexport const h = 1;\n"),
        ("src/b/y.mjs", "export const y = 1;\n"),
    ]);
    init_ok(&dir);
    assert_eq!(edge_pairs(&dir), ["a b"]);
    assert_lints_clean(&dir);
}
