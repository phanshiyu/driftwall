//! `init` drafts read as an architecture: boundaries, responsibilities, layers and cycles, and
//! the draft still lints clean.

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

    /// Runs `init`, then `lint --strict`, which must be clean. Returns the contract text.
    pub fn draft(files: &[(&str, &str)]) -> String {
        let dir = repo(files);
        let init = run("init", &dir, &[]);
        assert!(init.status.success(), "{init:?}");
        let lint = run("lint", &dir, &["--strict"]);
        assert!(lint.status.success(), "{lint:?}");
        fs::read_to_string(dir.path().join("architecture.mmd")).unwrap()
    }
}

use std::collections::BTreeSet;

use support::draft;

fn two_packages() -> Vec<(&'static str, &'static str)> {
    vec![
        ("package.json", "{\"name\": \"root\"}\n"),
        (
            "src/app.ts",
            "import { ui } from '../ui/src/kit';\nexport const app = ui;\n",
        ),
        (
            "src/store.ts",
            "/**\n * @file Keeps the app state in one place.\n */\nexport const s = 1;\n",
        ),
        (
            "src/view.ts",
            "import { s } from './store';\nexport const v = s;\n",
        ),
        (
            "ui/package.json",
            "{\"name\": \"ui\", \"description\": \"Shared widgets, no state.\"}\n",
        ),
        ("ui/src/kit.ts", "export const ui = 1;\n"),
        ("ui/src/icons.ts", "export const icon = 1;\n"),
        (
            "ui/src/theme.ts",
            "import { ui } from './kit';\nexport const t = ui;\n",
        ),
        ("tools/package.json", "{\"name\": \"tools\"}\n"),
        ("tools/a.ts", "export const a = 1;\n"),
        ("tools/b.ts", "export const b = 1;\n"),
    ]
}

#[test]
fn nodes_of_a_unit_share_a_boundary_and_stay_lint_clean() {
    let text = draft(&[
        ("alpha/__init__.py", ""),
        ("alpha/one.py", "from . import two\nX = two.Y\n"),
        ("alpha/two.py", "Y = 1\n"),
        ("beta/__init__.py", ""),
        ("beta/three.py", "from alpha import one\nZ = one.X\n"),
        ("beta/four.py", "W = 1\n"),
        ("tool.py", "import beta.three\n"),
    ]);
    let contract = driftwall::diagram::parse(&text).unwrap();
    let boundaries: BTreeSet<&str> = contract
        .nodes
        .values()
        .filter_map(|info| info.boundary.as_deref())
        .collect();
    assert_eq!(boundaries, BTreeSet::from(["alpha", "beta"]), "{text}");
    assert!(
        contract
            .nodes
            .keys()
            .all(|id| !boundaries.contains(id.to_string().as_str())),
        "{text}"
    );
}

#[test]
fn a_file_tag_gives_the_responsibility_and_a_manifest_the_unit_node() {
    let text = draft(&two_packages());
    assert!(
        text.contains("store: Keeps the app state in one place"),
        "{text}"
    );
}

#[test]
fn a_rust_inner_doc_and_a_python_docstring_label_their_nodes() {
    let rust = draft(&[
        (
            "Cargo.toml",
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        ),
        ("src/lib.rs", "pub mod a;\npub mod b;\n"),
        (
            "src/a.rs",
            "//! Does the first thing. And more.\npub fn a() {}\n",
        ),
        ("src/b.rs", "use crate::a::a;\npub fn b() { a() }\n"),
    ]);
    assert!(rust.contains("a: Does the first thing\""), "{rust}");
    assert!(rust.contains("b[\"src/b\"]"), "{rust}");
    let python = draft(&[
        ("app/__init__.py", ""),
        (
            "app/db.py",
            "\"\"\"Opens the database.\n\nMore.\"\"\"\nX = 1\n",
        ),
        ("app/api.py", "from . import db\nY = db.X\n"),
    ]);
    assert!(python.contains("db: Opens the database\""), "{python}");
}

#[test]
fn a_node_that_is_a_whole_unit_takes_the_manifest_description() {
    let text = draft(&[
        ("pkg/__init__.py", ""),
        (
            "pkg/pyproject.toml",
            "[project]\nname = \"pkg\"\ndescription = \"Sends the nightly report.\"\n",
        ),
        ("pkg/only.py", "X = 1\n"),
        ("app/main.py", "import pkg.only\n"),
    ]);
    assert!(text.contains("pkg: Sends the nightly report\""), "{text}");
}

#[test]
fn nodes_are_written_from_entry_points_to_foundations() {
    let text = draft(&[
        ("src/a_low.ts", "export const low = 1;\n"),
        (
            "src/mid.ts",
            "import { low } from './a_low';\nexport const m = low;\n",
        ),
        (
            "src/zz_top.ts",
            "import { m } from './mid';\nexport const t = m;\n",
        ),
    ]);
    let position = |id: &str| text.find(&format!("  {id}[")).unwrap();
    assert!(position("zz_top") < position("mid"), "{text}");
    assert!(position("mid") < position("a_low"), "{text}");
}

#[test]
fn cycles_are_listed_after_the_edges_and_counted() {
    let text = draft(&[
        ("src/a.ts", "import './b';\nexport const a = 1;\n"),
        ("src/b.ts", "import './a';\nexport const b = 1;\n"),
        ("src/c.ts", "import './a';\nexport const c = 1;\n"),
    ]);
    assert!(text.contains("%% 3 nodes, 3 edges, 1 cycle; generated by driftwall init"));
    let edges = text.rfind(" --> ").unwrap();
    let cycle = text.find("  %% cycles: a <-> b").unwrap();
    assert!(edges < cycle, "{text}");
}

#[test]
fn a_cycle_only_tests_make_is_named_apart() {
    let text = draft(&[
        ("src/a.ts", "export const a = 1;\n"),
        ("src/b.ts", "import './a';\nexport const b = 1;\n"),
        ("src/a.test.ts", "import './b';\n"),
    ]);
    assert!(text.contains("%% cycles (tests only): a <-> b"), "{text}");
}

#[test]
fn the_draft_is_the_same_on_every_run() {
    assert_eq!(draft(&two_packages()), draft(&two_packages()));
}
