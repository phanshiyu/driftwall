//! End-to-end tests of how `driftwall init` cuts repositories into nodes: code units, root files,
//! existing tags, id collisions and excluded files. Every baseline must lint clean.

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

    /// Output bytes as text.
    pub fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// The parsed `architecture.mmd`.
    pub fn contract(dir: &TempDir) -> driftwall::model::Contract {
        driftwall::diagram::parse(&read(dir, "architecture.mmd")).unwrap()
    }

    /// The text of a repository file.
    pub fn read(dir: &TempDir, path: &str) -> String {
        fs::read_to_string(dir.path().join(path)).unwrap()
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

use support::{arch_files, contract, read, repo, run_on, text};

type Files = Vec<(&'static str, &'static str)>;

fn init(dir: &TempDir, extra: &[&str]) -> Output {
    run_on("init", dir.path(), extra)
}

/// Runs init and asserts it succeeded.
fn init_ok(dir: &TempDir, extra: &[&str]) {
    let output = init(dir, extra);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
}

/// Asserts that `lint` on `dir` is exactly clean, under `--strict`.
fn assert_lints_clean(dir: &TempDir) {
    let lint = run_on("lint", dir.path(), &["--strict"]);
    assert_eq!(
        (lint.status.code(), text(&lint.stdout)),
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

const CRATE: &str = "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n";

/// A single crate at the root: flat modules and a directory in `src/`, then `scripts/`,
/// `static/`, `tests/` and `ui/` next to it, with minified and vendored files among them.
fn single_crate() -> Files {
    vec![
        ("Cargo.toml", CRATE),
        (
            "src/main.rs",
            "mod gitlab;\nmod store;\nmod routes;\n\nuse crate::gitlab::fetch;\nuse crate::store::save;\n\nfn main() {\n    save(fetch());\n}\n",
        ),
        (
            "src/gitlab.rs",
            "use crate::store::Cache;\n\npub fn fetch() -> Cache {\n    Cache\n}\n",
        ),
        (
            "src/store.rs",
            "pub struct Cache;\n\npub fn save(_c: Cache) {}\n",
        ),
        ("src/auth.rs", "pub fn login() {}\n"),
        (
            "src/routes/a.rs",
            "use crate::gitlab::fetch;\n\npub fn handle() {\n    fetch();\n}\n",
        ),
        ("scripts/build.py", "print('build')\n"),
        ("static/app.js", "export const app = 1;\n"),
        ("static/htmx.min.js", "export const htmx = 1;\n"),
        ("tests/it.rs", "fn t() {}\n"),
        ("ui/app.js", "export const ui = 1;\n"),
        (
            "ui/vendor/basecoat/basecoat.min.js",
            "export const basecoat = 1;\n",
        ),
    ]
}

#[test]
fn a_single_crate_gets_module_nodes_in_src_and_directory_nodes_elsewhere() {
    let dir = repo(&single_crate());
    init_ok(&dir, &[]);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(
        arch_files(dir.path()),
        [
            "scripts/.arch",
            "src/.arch",
            "static/.arch",
            "tests/.arch",
            "ui/.arch"
        ]
    );
    assert_eq!(
        node_ids(&dir),
        [
            "auth", "gitlab", "main", "routes", "scripts", "static", "store", "tests", "ui"
        ]
    );
    assert_eq!(
        edge_pairs(&dir),
        ["gitlab store", "main gitlab", "main store", "routes gitlab"]
    );
    assert_lints_clean(&dir);
}

#[test]
fn minified_and_vendored_files_are_excluded_and_counted() {
    let dir = repo(&single_crate());
    let ownership = driftwall::ownership::scan(dir.path()).unwrap();
    let listed = |reason| -> Vec<String> {
        ownership
            .excluded
            .get(&reason)
            .into_iter()
            .flatten()
            .map(|path| path.display().to_string())
            .collect()
    };
    assert_eq!(listed(Exclusion::Minified), ["static/htmx.min.js"]);
    assert_eq!(
        listed(Exclusion::Vendored),
        ["ui/vendor/basecoat/basecoat.min.js"]
    );
    assert!(
        !ownership
            .unmapped
            .iter()
            .any(|path| { path.to_string_lossy().contains("min.js") })
    );
}

#[test]
fn a_directory_that_only_holds_excluded_files_makes_no_node() {
    let dir = repo(&[
        ("src/app/a.ts", "export const a = 1;\n"),
        (
            "src/core/b.ts",
            "import { a } from '../app/a';\nexport const b = a;\n",
        ),
        ("static/htmx.min.js", "export const h = 1;\n"),
        (
            "ui/vendor/basecoat/basecoat.min.js",
            "export const b = 1;\n",
        ),
        ("third_party/x.py", "X = 1\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(node_ids(&dir), ["app", "core"]);
    assert_lints_clean(&dir);
}

#[test]
fn a_negated_driftwallignore_pattern_brings_a_vendored_file_back() {
    let dir = repo(&[
        (".driftwallignore", "!static/htmx.min.js\n"),
        ("src/app/a.ts", "export const a = 1;\n"),
        ("src/core/b.ts", "export const b = 1;\n"),
        ("static/htmx.min.js", "export const h = 1;\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(node_ids(&dir), ["src", "static"]);
    assert_lints_clean(&dir);
}

/// A JavaScript app whose root also holds `vite.config.ts` and an `engine/` Cargo workspace.
fn js_with_engine() -> Files {
    vec![
        ("package.json", "{\"name\": \"app\"}\n"),
        ("vite.config.ts", "export default {};\n"),
        (
            "src/main.ts",
            "import { x } from './util';\nexport const m = x;\n",
        ),
        ("src/util.ts", "export const x = 1;\n"),
        (
            "engine/Cargo.toml",
            "[workspace]\nmembers = [\"core\", \"cli\", \"wasm\"]\n",
        ),
        (
            "engine/core/Cargo.toml",
            "[package]\nname = \"dz_core\"\nversion = \"0.1.0\"\n",
        ),
        (
            "engine/core/src/lib.rs",
            "pub mod helper;\n\npub fn run() {}\n",
        ),
        ("engine/core/src/helper.rs", "pub fn help() {}\n"),
        ("engine/core/tests/it.rs", "fn t() {}\n"),
        (
            "engine/cli/Cargo.toml",
            "[package]\nname = \"dz_cli\"\nversion = \"0.1.0\"\n",
        ),
        (
            "engine/cli/src/main.rs",
            "use dz_core::run;\n\nfn main() {\n    run();\n}\n",
        ),
        (
            "engine/wasm/Cargo.toml",
            "[package]\nname = \"dz_wasm\"\nversion = \"0.1.0\"\n",
        ),
        ("engine/wasm/src/lib.rs", "pub fn w() {}\n"),
    ]
}

#[test]
fn each_workspace_member_crate_is_a_node_with_the_edges_between_them() {
    let dir = repo(&js_with_engine());
    init_ok(&dir, &[]);
    assert_eq!(
        node_ids(&dir),
        [
            "engine-cli",
            "engine-core-helper",
            "engine-core-lib",
            "engine-core-tests",
            "engine-wasm",
            "main",
            "root",
            "util"
        ]
    );
    assert_eq!(
        edge_pairs(&dir),
        ["engine-cli engine-core-lib", "main util"]
    );
    assert_lints_clean(&dir);
}

#[test]
fn root_config_files_are_grouped_into_one_root_node() {
    let dir = repo(&js_with_engine());
    init_ok(&dir, &[]);
    assert_eq!(read(&dir, ".arch"), "node: root\nscope: here\n");
    assert_eq!(read(&dir, "engine/core/src/.arch"), "node: engine-core-*\n");
    assert_eq!(
        read(&dir, "engine/core/tests/.arch"),
        "node: engine-core-tests\n"
    );
    assert_eq!(read(&dir, "engine/cli/.arch"), "node: engine-cli\n");
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_lints_clean(&dir);
}

#[test]
fn several_root_config_files_become_one_node_not_one_each() {
    let dir = repo(&[
        ("vite.config.ts", "export default {};\n"),
        ("eslint.config.js", "export default [];\n"),
        ("reset.ts", "export const reset = 1;\n"),
        ("src/app.ts", "export const app = 1;\n"),
        ("src/util.ts", "export const util = 1;\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(node_ids(&dir), ["root", "src"]);
    assert_lints_clean(&dir);
}

#[test]
fn a_root_that_holds_only_files_keeps_module_nodes() {
    let dir = repo(&[
        ("app.py", "from util import U\n\nA = U\n"),
        ("util.py", "U = 1\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(read(&dir, ".arch"), "node: *\n");
    assert_eq!(node_ids(&dir), ["app", "util"]);
    assert_lints_clean(&dir);
}

/// Python services and a shared library, each a package, plus a root script and a plain directory.
fn python_services() -> Files {
    vec![
        (
            "manage.py",
            "from services.auth.login import LOGIN\n\nM = LOGIN\n",
        ),
        ("scripts/run.py", "RUN = 1\n"),
        ("services/auth/__init__.py", ""),
        (
            "services/auth/login.py",
            "from libs.common.util import HELP\n\nLOGIN = HELP\n",
        ),
        ("services/billing/__init__.py", ""),
        (
            "services/billing/invoice.py",
            "from libs.common.util import HELP\nfrom services.auth.login import LOGIN\n\nINVOICE = [HELP, LOGIN]\n",
        ),
        ("libs/common/__init__.py", ""),
        ("libs/common/util.py", "HELP = 1\n"),
    ]
}

#[test]
fn outermost_python_packages_are_units_wherever_they_sit() {
    let dir = repo(&python_services());
    init_ok(&dir, &[]);
    assert_eq!(
        node_ids(&dir),
        [
            "libs-common-common",
            "libs-common-util",
            "root",
            "scripts",
            "services-auth-auth",
            "services-auth-login",
            "services-billing-billing",
            "services-billing-invoice"
        ]
    );
    assert_eq!(
        edge_pairs(&dir),
        [
            "root services-auth-login",
            "services-auth-login libs-common-util",
            "services-billing-invoice libs-common-util",
            "services-billing-invoice services-auth-login"
        ]
    );
    assert_lints_clean(&dir);
}

/// A JavaScript monorepo: `workspaces` names the packages.
fn js_workspaces() -> Files {
    vec![
        (
            "package.json",
            "{\"name\": \"mono\", \"workspaces\": [\"packages/*\"]}\n",
        ),
        ("packages/web/package.json", "{\"name\": \"@m/web\"}\n"),
        (
            "packages/web/src/index.ts",
            "import { api } from '../../api/src/index';\nexport const web = api;\n",
        ),
        ("packages/web/src/page.ts", "export const page = 1;\n"),
        ("packages/api/package.json", "{\"name\": \"@m/api\"}\n"),
        ("packages/api/src/index.ts", "export const api = 1;\n"),
        ("tools/gen.ts", "export const gen = 1;\n"),
    ]
}

#[test]
fn javascript_workspace_packages_are_nodes() {
    let dir = repo(&js_workspaces());
    init_ok(&dir, &[]);
    assert_eq!(
        node_ids(&dir),
        [
            "packages-api",
            "packages-web-page",
            "packages-web-src",
            "tools"
        ]
    );
    assert_eq!(edge_pairs(&dir), ["packages-web-src packages-api"]);
    assert_lints_clean(&dir);
}

#[test]
fn an_explicit_depth_ignores_units() {
    let dir = repo(&js_with_engine());
    init_ok(&dir, &["--depth", "1"]);
    assert_eq!(node_ids(&dir), ["engine", "root", "src"]);
    assert_lints_clean(&dir);
}

/// Hand-written tags that steer init away from what it would choose.
fn tagged_by_hand() -> Files {
    vec![
        ("app/.arch", "node: *\n"),
        (
            "app/ui/button.ts",
            "import { core } from '../core/core';\nexport const button = core;\n",
        ),
        ("app/core/core.ts", "export const core = 1;\n"),
        ("lib/.arch", "node: lib\n"),
        ("lib/x.ts", "export const x = 1;\n"),
    ]
}

#[test]
fn existing_tags_are_honored_by_no_tags() {
    let dir = repo(&tagged_by_hand());
    init_ok(&dir, &["--no-tags"]);
    assert_eq!(node_ids(&dir), ["core", "lib", "ui"]);
    assert_eq!(edge_pairs(&dir), ["ui core"]);
    assert_eq!(arch_files(dir.path()), ["app/.arch", "lib/.arch"]);
    assert_lints_clean(&dir);
}

#[test]
fn an_existing_star_tag_steers_the_rest_of_the_plan() {
    let mut files = single_crate();
    files.retain(|(path, _)| !path.contains("min.js"));
    files.push(("src/.arch", "node: *\n"));
    let dir = repo(&files);
    init_ok(&dir, &[]);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(
        arch_files(dir.path()),
        [
            "scripts/.arch",
            "src/.arch",
            "static/.arch",
            "tests/.arch",
            "ui/.arch"
        ]
    );
    assert_lints_clean(&dir);
}

#[test]
fn an_existing_tag_declares_its_node_even_when_its_directory_has_no_source() {
    let dir = repo(&[
        ("empty/.arch", "node: reserved\n"),
        ("a/x.ts", "export const x = 1;\n"),
        ("b/y.ts", "export const y = 1;\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(node_ids(&dir), ["a", "b", "reserved"]);
    assert_lints_clean(&dir);
}

#[test]
fn a_module_and_a_directory_with_the_same_id_are_never_merged() {
    let dir = repo(&[
        ("Cargo.toml", CRATE),
        (
            "src/main.rs",
            "mod ui;\nmod store;\n\nfn main() {\n    store::save();\n    ui::draw();\n}\n",
        ),
        ("src/store.rs", "pub fn save() {}\n"),
        ("src/ui.rs", "pub fn draw() {}\n"),
        ("ui/app.js", "export const app = 1;\n"),
        ("scripts/x.py", "X = 1\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(read(&dir, "src/.arch"), "node: *\n");
    assert_eq!(read(&dir, "ui/.arch"), "node: ui-dir\n");
    assert_eq!(read(&dir, "scripts/.arch"), "node: scripts\n");
    assert_eq!(node_ids(&dir), ["main", "scripts", "store", "ui", "ui-dir"]);
    assert_lints_clean(&dir);
}

#[test]
fn a_directory_that_collides_with_an_existing_tag_gets_a_dir_suffix() {
    let dir = repo(&[
        ("legacy/.arch", "node: core\n"),
        ("legacy/x.ts", "export const x = 1;\n"),
        ("core/y.ts", "export const y = 1;\n"),
        ("other/z.ts", "export const z = 1;\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(read(&dir, "core/.arch"), "node: core-dir\n");
    assert_eq!(node_ids(&dir), ["core", "core-dir", "other"]);
    assert_lints_clean(&dir);
}

#[test]
fn two_existing_tags_may_name_the_same_node_on_purpose() {
    let dir = repo(&[
        ("a/.arch", "node: shared\n"),
        ("a/x.ts", "export const x = 1;\n"),
        ("b/.arch", "node: shared\n"),
        ("b/y.ts", "export const y = 1;\n"),
    ]);
    init_ok(&dir, &[]);
    assert_eq!(node_ids(&dir), ["shared"]);
    assert_lints_clean(&dir);
}

#[test]
fn the_same_repository_always_gets_the_same_baseline() {
    for files in [single_crate(), js_with_engine(), python_services()] {
        let first = repo(&files);
        let second = repo(&files);
        init_ok(&first, &[]);
        init_ok(&second, &[]);
        assert_eq!(
            read(&first, "architecture.mmd"),
            read(&second, "architecture.mmd")
        );
        assert_eq!(arch_files(first.path()), arch_files(second.path()));
    }
}
