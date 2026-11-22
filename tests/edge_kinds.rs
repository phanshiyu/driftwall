//! End-to-end tests of how `brief` and `neighbors` show import kinds, module uses and sites.

use serde_json::Value;

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
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

    /// Runs `driftwall <args...> <root>`.
    pub fn driftwall(dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(args)
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn code(output: &Output) -> i32 {
        output.status.code().unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }
}

use support::{code, driftwall, repo, stdout};

const TWO_NODES: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";

/// `a` and `b` as tagged directories, with `a_files` added.
fn ab(a_files: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = vec![
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        ("b/.arch", "node: b\n"),
        ("b/lib.ts", "export const b = 1;\nexport type T = number;\n"),
    ];
    files.extend_from_slice(a_files);
    files
}

/// The first line of `brief` output that starts with `  a --> b`.
fn edge_line(dir: &tempfile::TempDir) -> String {
    let output = driftwall(dir, &["brief"]);
    assert_eq!(code(&output), 0);
    stdout(&output)
        .lines()
        .find(|line| line.starts_with("  a --> b"))
        .unwrap_or("(no edge line)")
        .to_owned()
}

#[test]
fn an_edge_of_only_type_imports_is_annotated_type_only() {
    let dir = repo(&ab(&[(
        "a/main.ts",
        "import type { T } from '../b/lib';\nimport { type T as U } from '../b/lib';\n",
    )]));
    assert_eq!(edge_line(&dir), "  a --> b (2 imports) (type-only) uses: T");
}

#[test]
fn an_edge_of_only_test_imports_is_annotated_test_only() {
    let dir = repo(&ab(&[(
        "a/main.test.ts",
        "import { b } from '../b/lib';\nexport const t = b;\n",
    )]));
    assert_eq!(edge_line(&dir), "  a --> b (1 import) (test-only) uses: b");
}

#[test]
fn an_edge_of_only_assets_is_annotated_asset_and_uses_no_module() {
    let mut files = ab(&[("a/main.ts", "import data from '../b/data.json';\n")]);
    files.push(("b/data.json", "{}\n"));
    let dir = repo(&files);
    assert_eq!(edge_line(&dir), "  a --> b (1 import) (asset)");
}

#[test]
fn an_edge_of_only_embeds_is_annotated_embed() {
    let dir = repo(&[
        ("architecture.mmd", TWO_NODES),
        (
            "Cargo.toml",
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        ),
        ("a/.arch", "node: a\n"),
        (
            "a/lib.rs",
            "pub const SRC: &str = include_str!(\"../b/lib.rs\");\n",
        ),
        ("b/.arch", "node: b\n"),
        ("b/lib.rs", "pub fn b() {}\n"),
    ]);
    assert_eq!(edge_line(&dir), "  a --> b (1 import) (embed)");
}

#[test]
fn an_edge_with_mixed_imports_shows_counts_per_property() {
    let dir = repo(&ab(&[
        (
            "a/main.ts",
            "import { b } from '../b/lib';\nimport type { T } from '../b/lib';\nexport const a = b;\n",
        ),
        (
            "a/main.test.ts",
            "import { b } from '../b/lib';\nimport type { T } from '../b/lib';\n",
        ),
    ]));
    assert_eq!(
        edge_line(&dir),
        "  a --> b (4 imports, 2 type-only, 2 test-only) uses: T, b"
    );
}

#[test]
fn a_plain_runtime_edge_keeps_its_plain_count() {
    let dir = repo(&ab(&[("a/main.ts", "import { b } from '../b/lib';\n")]));
    assert_eq!(edge_line(&dir), "  a --> b (1 import) uses: b");
}

#[test]
fn a_rust_module_binding_is_displayed_as_a_module_use() {
    let dir = repo(&[
        (
            "architecture.mmd",
            "flowchart LR\n  main[\"main\"]\n  gitlab[\"gitlab\"]\n  main --> gitlab\n",
        ),
        (
            "Cargo.toml",
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        ),
        ("src/.arch", "node: *\n"),
        (
            "src/main.rs",
            "mod gitlab;\nuse crate::gitlab;\nfn main() {}\n",
        ),
        ("src/gitlab.rs", "pub fn fetch() {}\n"),
    ]);
    let output = driftwall(&dir, &["brief"]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    assert!(
        out.contains("  main --> gitlab (1 import) uses: module\n"),
        "{out}"
    );
    assert!(!out.contains("uses: *"), "{out}");
}

#[test]
fn a_python_module_import_is_displayed_as_a_module_use() {
    let dir = repo(&[
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        ("a/main.py", "from b import lib\n"),
        ("b/.arch", "node: b\n"),
        ("b/lib.py", "X = 1\n"),
    ]);
    assert_eq!(edge_line(&dir), "  a --> b (1 import) uses: module");
}

#[test]
fn a_module_use_and_a_symbol_use_are_both_listed_module_first() {
    let dir = repo(&ab(&[(
        "a/main.ts",
        "import { b } from '../b/lib';\nimport '../b/lib';\n",
    )]));
    assert_eq!(edge_line(&dir), "  a --> b (2 imports) uses: module, b");
}

/// `a` imports `b` from `n` different lines.
fn many_sites(n: usize) -> Vec<(&'static str, String)> {
    let lines: Vec<String> = (1..=n)
        .map(|i| format!("import {{ b as b{i} }} from '../b/lib';\n"))
        .collect();
    let main = lines.concat();
    vec![("a/main.ts", main)]
}

fn repo_with_sites(n: usize) -> tempfile::TempDir {
    let mut files: Vec<(String, String)> = ab(&[])
        .into_iter()
        .map(|(path, text)| (path.to_owned(), text.to_owned()))
        .collect();
    files.extend(
        many_sites(n)
            .into_iter()
            .map(|(path, text)| (path.to_owned(), text)),
    );
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    repo(&borrowed)
}

#[test]
fn neighbors_text_stops_at_three_sites_unless_all_sites_is_given() {
    let dir = repo_with_sites(5);
    let capped = stdout(&driftwall(&dir, &["neighbors", "a"]));
    assert!(
        capped.contains("    a/main.ts:3\n    ... 2 more\n"),
        "{capped}"
    );
    let every = stdout(&driftwall(&dir, &["neighbors", "a", "--all-sites"]));
    assert!(every.contains("    a/main.ts:5\n"), "{every}");
    assert!(!every.contains("more"), "{every}");
}

#[test]
fn json_lists_every_site_with_its_kind_and_test_flag() {
    let dir = repo_with_sites(5);
    for args in [["brief"], ["neighbors"]] {
        let mut full: Vec<&str> = args.to_vec();
        if args == ["neighbors"] {
            full.push("a");
        }
        full.extend(["--format", "json"]);
        let output = driftwall(&dir, &full);
        assert_eq!(code(&output), 0);
        let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
        let edge = if args == ["brief"] {
            &value["edges"][0]
        } else {
            &value["outgoing"][0]
        };
        let sites = edge["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 5, "{args:?}");
        assert_eq!(edge["realised"], 5);
        assert_eq!(
            sites[4],
            serde_json::json!({"file": "a/main.ts", "line": 5, "kind": "runtime", "test_only": false})
        );
    }
}

#[test]
fn json_sites_carry_a_type_only_kind_and_a_test_flag() {
    let dir = repo(&ab(&[(
        "a/main.test.ts",
        "import type { T } from '../b/lib';\n",
    )]));
    let output = driftwall(&dir, &["brief", "--format", "json"]);
    let value: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        value["edges"][0]["sites"][0],
        serde_json::json!({"file": "a/main.test.ts", "line": 1, "kind": "type-only", "test_only": true})
    );
}
