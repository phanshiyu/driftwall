//! End-to-end tests of how `driftwall init` groups files: JavaScript units that are layers (split
//! by directory), the primary unit and root files, test files that join a node, assets owned by
//! declared nodes, and Rust crates and Python packages that stay one node per module. Every
//! baseline must lint exactly clean.

use std::process::Output;

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
}

use support::{contract, read, repo, run_on, text, where_is};

type Files = Vec<(&'static str, &'static str)>;

/// Runs init and asserts it succeeded.
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

/// What `driftwall where <file>` prints.
fn answer(dir: &TempDir, file: &str) -> String {
    text(&where_is(dir.path(), file).stdout)
}

/// A TypeScript app whose layers each have a `tsconfig.json`, beside tooling at the root.
fn layered_app() -> Files {
    vec![
        ("package.json", "{\"name\": \"app\"}\n"),
        ("tsconfig.json", "{}\n"),
        ("src/tsconfig.json", "{}\n"),
        ("src/domain/tsconfig.json", "{}\n"),
        (
            "src/main.ts",
            "import { ui } from './ui/app';\nimport { a } from './domain/a';\n\nexport const main = [ui, a];\n",
        ),
        (
            "src/ui/app.ts",
            "import { b } from '../domain/b';\n\nexport const ui = b;\n",
        ),
        (
            "src/domain/a.ts",
            "import { b } from './b';\n\nexport const a = b;\n",
        ),
        (
            "src/domain/b.ts",
            "import { c } from './c';\n\nexport const b = c;\n",
        ),
        ("src/domain/c.ts", "export const c = 1;\n"),
        ("vite.config.ts", "export default {};\n"),
        ("eslint.config.js", "export default [];\n"),
        (
            "scripts/s.ts",
            "import { main } from '../src/main';\n\nexport const s = main;\n",
        ),
    ]
}

#[test]
fn layers_with_their_own_tsconfig_are_split_by_directory_not_by_file() {
    let dir = repo(&layered_app());
    init_ok(&dir);
    assert_eq!(
        node_ids(&dir),
        ["root", "scripts", "src", "src-domain", "src-ui"]
    );
    assert_eq!(
        edge_pairs(&dir),
        [
            "scripts src",
            "src src-domain",
            "src src-ui",
            "src-ui src-domain"
        ]
    );
    assert_eq!(read(&dir, ".arch"), "node: root\nscope: here\n");
    assert_eq!(read(&dir, "src/.arch"), "node: src\nscope: here\n");
    assert_eq!(
        read(&dir, "src/domain/.arch"),
        "node: src-domain\nscope: here\n"
    );
    assert_lints_clean(&dir);
}

#[test]
fn a_file_added_to_a_layer_stays_in_its_node() {
    let dir = repo(&layered_app());
    init_ok(&dir);
    std::fs::write(
        dir.path().join("src/domain/d.ts"),
        "import { a } from './a';\n\nexport const d = a;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/ui/panel.ts"),
        "export const panel = 1;\n",
    )
    .unwrap();
    assert_lints_clean(&dir);
}

#[test]
fn root_files_are_one_node_when_src_is_a_unit_of_its_own() {
    let dir = repo(&[
        ("package.json", "{\"name\": \"app\"}\n"),
        ("tsconfig.json", "{}\n"),
        ("src/tsconfig.json", "{}\n"),
        (
            "src/a.ts",
            "import { b } from './b';\nexport const a = b;\n",
        ),
        ("src/b.ts", "export const b = 1;\n"),
        ("vite.config.ts", "export default {};\n"),
        ("eslint.config.js", "export default [];\n"),
        ("reset.js", "export const reset = 1;\n"),
        (
            "index.html",
            "<html><script type=\"module\" src=\"/src/a.ts\"></script></html>\n",
        ),
    ]);
    init_ok(&dir);
    assert_eq!(node_ids(&dir), ["root", "src"]);
    assert_eq!(read(&dir, ".arch"), "node: root\nscope: here\n");
    assert_lints_clean(&dir);
}

/// A root package with a flat `src/`, and a sub-project `ui/` shaped like a real Vite app.
fn root_with_ui() -> Files {
    vec![
        ("package.json", "{\"name\": \"root\"}\n"),
        (
            "src/a.ts",
            "import { b } from './b';\nexport const a = b;\n",
        ),
        ("src/b.ts", "export const b = 1;\n"),
        ("src/c.ts", "export const c = 2;\n"),
        ("ui/package.json", "{\"name\": \"ui\"}\n"),
        ("ui/tsconfig.json", "{}\n"),
        ("ui/vite.config.ts", "export default {};\n"),
        ("ui/dev/serve.mjs", "export const serve = 1;\n"),
        (
            "ui/src/main.tsx",
            "import { App } from './App';\nimport './index.css';\nimport './components/button.css';\n\nexport const main = App;\n",
        ),
        (
            "ui/src/App.tsx",
            "import { Page } from './page';\nimport { Button } from './components/Button';\n\nexport const App = [Page, Button];\n",
        ),
        (
            "ui/src/page.tsx",
            "import { store } from './store';\n\nexport const Page = store;\n",
        ),
        ("ui/src/store.tsx", "export const store = 1;\n"),
        ("ui/src/vendor.d.ts", "declare const v: number;\n"),
        ("ui/src/index.css", "body { margin: 0; }\n"),
        ("ui/src/components/button.css", ".b { color: red; }\n"),
        (
            "ui/src/components/Button.tsx",
            "import { util } from '../lib/util';\n\nexport const Button = util;\n",
        ),
        ("ui/src/lib/util.ts", "export const util = 1;\n"),
        (
            "ui/src/test/app.test.tsx",
            "import { App } from '../App';\n\nexport const t = App;\n",
        ),
    ]
}

#[test]
fn a_sub_project_is_split_by_directory_while_the_root_src_keeps_its_modules() {
    let dir = repo(&root_with_ui());
    init_ok(&dir);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(
        node_ids(&dir),
        [
            "a",
            "b",
            "c",
            "ui",
            "ui-components",
            "ui-dev",
            "ui-lib",
            "ui-src",
            "ui-test"
        ]
    );
    assert_eq!(
        edge_pairs(&dir),
        [
            "a b",
            "ui-components ui-lib",
            "ui-src ui-components",
            "ui-test ui-src"
        ]
    );
    assert_lints_clean(&dir);
}

#[test]
fn an_asset_a_declared_edge_points_at_is_owned_by_a_declared_node() {
    let dir = repo(&root_with_ui());
    init_ok(&dir);
    let declared = node_ids(&dir);
    for (file, owner) in [
        ("ui/src/index.css", "ui-src"),
        ("ui/src/components/button.css", "ui-components"),
    ] {
        assert!(declared.iter().any(|id| id == owner), "{owner}");
        let said = answer(&dir, file);
        assert!(
            said.starts_with(&format!("{file}: asset of node {owner} ")),
            "{said}"
        );
    }
    assert_lints_clean(&dir);
}

#[test]
fn an_asset_import_in_a_single_sub_project_lints_clean() {
    let dir = repo(&[
        ("package.json", "{\"name\": \"root\"}\n"),
        ("lib/x.ts", "export const x = 1;\n"),
        ("lib/y.ts", "export const y = 1;\n"),
        ("ui/package.json", "{\"name\": \"ui\"}\n"),
        (
            "ui/src/main.tsx",
            "import './index.css';\n\nexport const main = 1;\n",
        ),
        ("ui/src/index.css", "body { margin: 0; }\n"),
    ]);
    init_ok(&dir);
    assert!(node_ids(&dir).iter().any(|id| id == "ui-src"));
    assert!(
        answer(&dir, "ui/src/index.css").starts_with("ui/src/index.css: asset of node ui-src ")
    );
    assert_lints_clean(&dir);
}

#[test]
fn typescript_tests_join_their_module_or_the_node_of_their_directory() {
    let dir = repo(&[
        ("src/a.ts", "export const a = 1;\n"),
        (
            "src/b.ts",
            "import { a } from './a';\nexport const b = a;\n",
        ),
        (
            "src/a.test.ts",
            "import { a } from './a';\nexport const t = a;\n",
        ),
        (
            "src/rows.test.ts",
            "import { b } from './b';\nexport const r = b;\n",
        ),
        (
            "src/cols.spec.ts",
            "import { a } from './a';\nexport const c = a;\n",
        ),
    ]);
    init_ok(&dir);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(node_ids(&dir), ["a", "b", "src"]);
    assert_eq!(edge_pairs(&dir), ["b a", "src a", "src b"]);
    assert!(answer(&dir, "src/a.test.ts").starts_with("src/a.test.ts: a "));
    assert!(answer(&dir, "src/rows.test.ts").starts_with("src/rows.test.ts: src "));
    assert_lints_clean(&dir);
}

#[test]
fn a_directory_of_only_tests_is_not_cut_into_nodes() {
    let dir = repo(&[
        ("lib/x.ts", "export const x = 1;\n"),
        ("lib/y.ts", "export const y = 1;\n"),
        ("e2e/a.test.ts", "export const a = 1;\n"),
        ("e2e/b.test.ts", "export const b = 1;\n"),
        ("e2e/c.spec.ts", "export const c = 1;\n"),
    ]);
    init_ok(&dir);
    assert_eq!(node_ids(&dir), ["e2e", "lib"]);
    assert_lints_clean(&dir);
}

#[test]
fn python_tests_join_the_node_of_their_directory() {
    let dir = repo(&[
        ("app/a.py", "VALUE = 1\n"),
        ("app/b.py", "from a import VALUE\n\nOTHER = VALUE\n"),
        ("app/test_a.py", "from a import VALUE\n\nCHECK = VALUE\n"),
        ("app/b_test.py", "from b import OTHER\n\nCHECK = OTHER\n"),
    ]);
    init_ok(&dir);
    assert_eq!(read(&dir, "app/.arch"), "node: *\n");
    assert_eq!(node_ids(&dir), ["a", "app", "b"]);
    assert!(answer(&dir, "app/test_a.py").starts_with("app/test_a.py: app "));
    assert!(answer(&dir, "app/b_test.py").starts_with("app/b_test.py: app "));
    assert_lints_clean(&dir);
}

#[test]
fn rust_tests_join_the_node_of_their_directory() {
    let dir = repo(&[
        (
            "Cargo.toml",
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
        ),
        ("src/main.rs", "mod a;\nmod b;\n\nfn main() {}\n"),
        ("src/a.rs", "pub fn a() {}\n"),
        (
            "src/b.rs",
            "use crate::a::a;\n\npub fn b() {\n    a();\n}\n",
        ),
        (
            "src/a_test.rs",
            "use crate::a::a;\n\nfn t() {\n    a();\n}\n",
        ),
        (
            "src/b_tests.rs",
            "use crate::b::b;\n\nfn t() {\n    b();\n}\n",
        ),
    ]);
    init_ok(&dir);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(node_ids(&dir), ["a", "b", "main", "src"]);
    assert!(answer(&dir, "src/a_test.rs").starts_with("src/a_test.rs: src "));
    assert_lints_clean(&dir);
}

#[test]
fn a_crate_and_a_python_package_are_still_split_per_module() {
    let dir = repo(&[
        ("package.json", "{\"name\": \"root\"}\n"),
        ("src/a.ts", "export const a = 1;\n"),
        (
            "src/b.ts",
            "import { a } from './a';\nexport const b = a;\n",
        ),
        (
            "engine/Cargo.toml",
            "[package]\nname = \"engine\"\nversion = \"0.1.0\"\n",
        ),
        ("engine/src/main.rs", "mod x;\nmod y;\n\nfn main() {}\n"),
        ("engine/src/x.rs", "pub fn x() {}\n"),
        (
            "engine/src/y.rs",
            "use crate::x::x;\n\npub fn y() {\n    x();\n}\n",
        ),
        ("tools/pkg/__init__.py", ""),
        ("tools/pkg/one.py", "VALUE = 1\n"),
        (
            "tools/pkg/two.py",
            "from .one import VALUE\n\nOTHER = VALUE\n",
        ),
    ]);
    init_ok(&dir);
    assert_eq!(read(&dir, "engine/src/.arch"), "node: engine-*\n");
    assert_eq!(read(&dir, "tools/pkg/.arch"), "node: tools-pkg-*\n");
    assert_eq!(
        node_ids(&dir),
        [
            "a",
            "b",
            "engine-main",
            "engine-x",
            "engine-y",
            "tools-pkg-one",
            "tools-pkg-two"
        ]
    );
    assert_lints_clean(&dir);
}

#[test]
fn where_derives_the_same_node_as_the_scan_for_a_test_file() {
    let dir = repo(&[
        ("src/.arch", "node: *\n"),
        ("src/a.ts", ""),
        ("src/b.ts", ""),
        ("src/a.test.ts", ""),
        ("src/c.test.ts", ""),
    ]);
    assert!(answer(&dir, "src/a.test.ts").starts_with("src/a.test.ts: a "));
    assert!(answer(&dir, "src/c.test.ts").starts_with("src/c.test.ts: src "));
}
