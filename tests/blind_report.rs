//! End-to-end tests of the blind spots in `brief`, `neighbors`, `lint` and `init`: what the
//! analysis cannot see is counted, attributed to nodes and never changes an exit code.

use serde_json::json;

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

    pub fn json_of(output: &Output) -> serde_json::Value {
        serde_json::from_str(&stdout(output)).unwrap()
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8(output.stderr.clone()).unwrap()
    }
}

use support::{code, driftwall, json_of, repo, stderr, stdout};

const TWO_NODES: &str = "flowchart LR\n  a[\"a\"]\n  b[\"b\"]\n  a --> b\n";

/// A repository that is blind in every way: unread Swift and shell files (some in no node), a
/// vendored and a minified file, a file only `.driftwallignore` hides, a skipped package, a
/// computed import and an import of a file that is not there. `.gitignore`d files of every sort
/// are present too and must be counted nowhere.
fn blind_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", TWO_NODES),
        (".gitignore", "a/secret.ts\na/Secret.go\n"),
        (".driftwallignore", "a/hidden.ts\n"),
        ("a/.arch", "node: a\n"),
        ("b/.arch", "node: b\n"),
        (
            "a/main.ts",
            "import { b } from '../b/lib';\nimport 'left-pad';\nimport './gone';\nvoid import(modName);\n",
        ),
        ("a/View.go", "import UIKit\nlet x = 1\n"),
        ("a/App.go", "x\n"),
        ("a/Secret.go", "gitignored\n"),
        ("a/secret.ts", "export const s = 1;\n"),
        ("a/run.pl", "#!/bin/sh\necho\n"),
        ("a/vendor/lib.js", "export const v = 1;\n"),
        ("a/hidden.ts", "export const h = 1;\n"),
        ("b/lib.ts", "export const b = 1;\n"),
        ("b/util.go", "a\nb\nc"),
        ("loose/tool.pl", "echo\n"),
        ("loose/app.min.js", "x\n"),
    ]
}

/// A repository with nothing to be blind to.
fn clean_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("architecture.mmd", TWO_NODES),
        ("a/.arch", "node: a\n"),
        ("b/.arch", "node: b\n"),
        ("a/main.ts", "import { b } from '../b/lib';\n"),
        ("b/lib.ts", "export const b = 1;\n"),
    ]
}

/// The files of `clean_files` that `init` writes itself left out: no contract and no tags.
fn untagged() -> Vec<(&'static str, &'static str)> {
    let written = ["architecture.mmd", "a/.arch", "b/.arch"];
    let mut files = clean_files();
    files.retain(|(path, _)| !written.contains(path));
    files
}

#[test]
fn brief_ends_with_the_blind_spots_block_and_every_node_line_counts_its_own() {
    let dir = repo(&blind_files());
    let output = driftwall(&dir, &["brief"]);
    assert_eq!(code(&output), 0);
    let expected = "\
driftwall: 2 nodes, 1 edges (1 implemented, 0 planned), 0 errors, 1 warnings
nodes:
  a: a (1 file) blind: 3 not read, 1 excluded, 1 ignored, 2 skipped, 1 unresolved
  b: b (1 file) blind: 1 not read
edges:
  a --> b (1 import) uses: b
blind spots:
  not read: go 3 files (6 lines), pl 2 files (3 lines)
  excluded: vendored 1, minified 1
  ignored by .driftwallignore: 1
  skipped references: undeclared-package 1, dynamic-specifier 1
    undeclared-package:
      a/main.ts:2 left-pad
    dynamic-specifier:
      a/main.ts:4 modName
  unresolved imports: 1
  unmapped: 2
    not read: loose/tool.pl (pl, 1 line)
    excluded: loose/app.min.js (minified)
";
    assert_eq!(stdout(&output), expected);
    assert_eq!(stderr(&output), "");
}

#[test]
fn brief_says_none_when_nothing_is_blind_and_node_lines_stay_plain() {
    let dir = repo(&clean_files());
    let output = driftwall(&dir, &["brief"]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "\
driftwall: 2 nodes, 1 edges (1 implemented, 0 planned), 0 errors, 0 warnings
nodes:
  a: a (1 file)
  b: b (1 file)
edges:
  a --> b (1 import) uses: b
blind spots: none
"
    );
}

#[test]
fn brief_json_has_the_summary_the_unmapped_tally_and_a_tally_per_node() {
    let dir = repo(&blind_files());
    let output = driftwall(&dir, &["brief", "--format", "json"]);
    assert_eq!(code(&output), 0);
    let value = json_of(&output);
    assert_eq!(
        value["blind_spots"],
        json!({
            "unread": {"pl": {"count": 2, "lines": 3}, "go": {"count": 3, "lines": 6}},
            "excluded": {"vendored": 1, "minified": 1},
            "ignored": 1,
            "skipped": {"undeclared-package": 1, "dynamic-specifier": 1},
            "unresolved": 1,
            "skipped_references": {
                "undeclared-package": [
                    {"file": "a/main.ts", "line": 2, "specifier": "left-pad", "node": "a"},
                ],
                "dynamic-specifier": [
                    {"file": "a/main.ts", "line": 4, "specifier": "modName", "node": "a"},
                ],
            },
            "unmapped": {"unread": 1, "excluded": 1, "ignored": 0, "skipped": 0, "unresolved": 0},
            "unmapped_items": {
                "unread": {
                    "pl": {
                        "count": 1,
                        "lines": 1,
                        "files": [{"file": "loose/tool.pl", "node": null, "lines": 1}],
                    },
                },
                "excluded": {"minified": [{"file": "loose/app.min.js", "node": null}]},
                "ignored": [],
                "skipped": {},
                "unresolved": [],
            },
        })
    );
    assert_eq!(
        value["nodes"][0]["blind_spots"],
        json!({"unread": 3, "excluded": 1, "ignored": 1, "skipped": 2, "unresolved": 1})
    );
    assert_eq!(
        value["nodes"][1]["blind_spots"],
        json!({"unread": 1, "excluded": 0, "ignored": 0, "skipped": 0, "unresolved": 0})
    );
}

#[test]
fn brief_json_of_a_clean_repo_has_empty_blind_spots() {
    let dir = repo(&clean_files());
    let value = json_of(&driftwall(&dir, &["brief", "--format", "json"]));
    assert_eq!(
        value["blind_spots"],
        json!({
            "unread": {}, "excluded": {}, "ignored": 0, "skipped": {}, "unresolved": 0,
            "skipped_references": {},
            "unmapped": {"unread": 0, "excluded": 0, "ignored": 0, "skipped": 0, "unresolved": 0},
            "unmapped_items": {
                "unread": {}, "excluded": {}, "ignored": [], "skipped": {}, "unresolved": [],
            },
        })
    );
}

/// `clean_files` plus `count` unread Go files in a directory no node owns.
fn with_loose_files(count: usize) -> Vec<(String, &'static str)> {
    let mut files: Vec<(String, &'static str)> = clean_files()
        .into_iter()
        .map(|(path, text)| (path.to_owned(), text))
        .collect();
    files.extend((0..count).map(|n| (format!("loose/f{n:02}.go"), "x\n")));
    files
}

#[test]
fn brief_names_the_first_twenty_unmapped_items_and_counts_the_rest() {
    let owned = with_loose_files(25);
    let files: Vec<(&str, &str)> = owned
        .iter()
        .map(|(path, text)| (path.as_str(), *text))
        .collect();
    let dir = repo(&files);
    let out = stdout(&driftwall(&dir, &["brief"]));
    let tail = out.split_once("  unmapped: 25\n").unwrap().1;
    let mut expected: Vec<String> = (0..20)
        .map(|n| format!("    not read: loose/f{n:02}.go (go, 1 line)"))
        .collect();
    expected.push("    +5 more".to_owned());
    assert_eq!(tail, format!("{}\n", expected.join("\n")));
}

#[test]
fn brief_names_exactly_twenty_unmapped_items_without_a_more_line() {
    let owned = with_loose_files(20);
    let files: Vec<(&str, &str)> = owned
        .iter()
        .map(|(path, text)| (path.as_str(), *text))
        .collect();
    let dir = repo(&files);
    let out = stdout(&driftwall(&dir, &["brief"]));
    assert!(out.contains("  unmapped: 20\n"), "{out}");
    assert!(
        out.ends_with("    not read: loose/f19.go (go, 1 line)\n"),
        "{out}"
    );
}

#[test]
fn brief_json_lists_every_unmapped_item_uncapped() {
    let owned = with_loose_files(25);
    let files: Vec<(&str, &str)> = owned
        .iter()
        .map(|(path, text)| (path.as_str(), *text))
        .collect();
    let dir = repo(&files);
    let value = json_of(&driftwall(&dir, &["brief", "--format", "json"]));
    let listed = &value["blind_spots"]["unmapped_items"]["unread"]["go"];
    assert_eq!(listed["count"], 25);
    assert_eq!(listed["files"].as_array().unwrap().len(), 25);
    assert_eq!(listed["files"][24]["file"], "loose/f24.go");
    assert_eq!(value["blind_spots"]["unmapped"]["unread"], 25);
}

#[test]
fn gitignored_files_are_counted_nowhere() {
    let dir = repo(&blind_files());
    let all = stdout(&driftwall(&dir, &["neighbors", "a", "--files"]));
    assert!(!all.contains("Secret.go"), "{all}");
    assert!(!all.contains("secret.ts"), "{all}");
}

#[test]
fn neighbors_lists_every_blind_item_of_the_node_and_files_only_on_request() {
    let dir = repo(&blind_files());
    let plain = driftwall(&dir, &["neighbors", "a"]);
    assert_eq!(code(&plain), 0);
    let blind = "\
blind spots:
  not read: a/App.go (go, 1 line)
  not read: a/View.go (go, 2 lines)
  not read: a/run.pl (pl, 2 lines)
  excluded: a/vendor/lib.js (vendored)
  ignored: a/hidden.ts
  skipped: a/main.ts:2 left-pad (undeclared-package)
  skipped: a/main.ts:4 modName (dynamic-specifier)
  unresolved: a/main.ts:3 ./gone
";
    let expected = format!(
        "\
node: a
label: a
dirs: a
files: 1
assets: 4
outgoing:
  a --> b (1 import) uses: b
    a/main.ts:1
incoming:
  (none)
{blind}"
    );
    assert_eq!(stdout(&plain), expected);

    let with_files = driftwall(&dir, &["neighbors", "a", "--files"]);
    assert_eq!(code(&with_files), 0);
    assert_eq!(
        stdout(&with_files),
        expected
            .replace("files: 1\n", "files: 1\n  a/main.ts\n")
            .replace(
                "assets: 4\n",
                "assets: 4\n  a/App.go\n  a/View.go\n  a/run.pl\n  a/vendor/lib.js\n"
            )
    );
}

#[test]
fn neighbors_of_a_node_with_nothing_to_hide_says_none() {
    let dir = repo(&clean_files());
    let output = driftwall(&dir, &["neighbors", "b", "--files"]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "\
node: b
label: b
dirs: b
files: 1
  b/lib.ts
outgoing:
  (none)
incoming:
  a --> b (1 import) uses: b
    a/main.ts:1
blind spots: none
"
    );
}

#[test]
fn neighbors_json_always_has_the_files_and_every_blind_item_with_its_node() {
    let dir = repo(&blind_files());
    let value = json_of(&driftwall(&dir, &["neighbors", "a", "--format", "json"]));
    assert_eq!(value["files"], json!(["a/main.ts"]));
    let blind = &value["blind_spots"];
    assert_eq!(
        blind["unread"]["go"],
        json!({
            "count": 2,
            "lines": 3,
            "files": [
                {"file": "a/App.go", "node": "a", "lines": 1},
                {"file": "a/View.go", "node": "a", "lines": 2},
            ],
        })
    );
    assert_eq!(
        blind["unread"]["pl"]["files"],
        json!([{"file": "a/run.pl", "node": "a", "lines": 2}])
    );
    assert_eq!(
        blind["excluded"],
        json!({"vendored": [{"file": "a/vendor/lib.js", "node": "a"}]})
    );
    assert_eq!(
        blind["ignored"],
        json!([{"file": "a/hidden.ts", "node": "a"}])
    );
    assert_eq!(
        blind["skipped"],
        json!({
            "undeclared-package": [
                {"file": "a/main.ts", "line": 2, "specifier": "left-pad", "node": "a"}
            ],
            "dynamic-specifier": [
                {"file": "a/main.ts", "line": 4, "specifier": "modName", "node": "a"}
            ],
        })
    );
    assert_eq!(
        blind["unresolved"],
        json!([{"file": "a/main.ts", "line": 3, "specifier": "./gone", "node": "a"}])
    );
    assert_eq!(value["node"]["blind_spots"]["unread"], json!(3));
}

#[test]
fn neighbors_json_lists_every_file_of_a_large_node_uncapped() {
    let mut files = clean_files();
    let names: Vec<String> = (0..12).map(|n| format!("b/m{n:02}.ts")).collect();
    files.extend(names.iter().map(|name| (name.as_str(), "export {};\n")));
    let dir = repo(&files);
    let value = json_of(&driftwall(&dir, &["neighbors", "b", "--format", "json"]));
    assert_eq!(value["files"].as_array().unwrap().len(), 13);
}

#[test]
fn lint_appends_one_note_line_and_changes_no_exit_code() {
    let dir = repo(&blind_files());
    let output = driftwall(&dir, &["lint"]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        stdout(&output),
        "\
a/main.ts:3: warning[unresolved-import]: import `./gone` resolves to no file
  hint: fix the import path or add the missing file
driftwall: 0 errors, 1 warning
note: 5 files not read (go 3, pl 2), 2 files excluded (vendored 1, minified 1), 1 file ignored by .driftwallignore, 2 skipped references, 1 unresolved import; see driftwall brief
"
    );
    // why: the census files in no node (`loose/`) must never turn into unmapped-file errors.
    assert!(!stdout(&output).contains("unmapped-file"));
    let strict = driftwall(&dir, &["lint", "--strict"]);
    assert_eq!(code(&strict), 1);
    assert!(stdout(&strict).ends_with("; see driftwall brief\n"));
}

#[test]
fn lint_of_a_repo_with_a_finding_keeps_exit_1_and_still_notes() {
    let mut files = blind_files();
    files.push(("loose/free.ts", "export const c = 1;\n"));
    let dir = repo(&files);
    let output = driftwall(&dir, &["lint"]);
    assert_eq!(code(&output), 1);
    let out = stdout(&output);
    assert!(out.contains("error[unmapped-file]"), "{out}");
    assert!(out.contains("\nnote: 5 files not read"), "{out}");
}

#[test]
fn lint_of_a_clean_repo_prints_no_note() {
    let dir = repo(&clean_files());
    let output = driftwall(&dir, &["lint"]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output), "driftwall: clean\n");
}

#[test]
fn a_lone_unread_extension_is_singular_in_the_note() {
    let mut files = clean_files();
    files.push(("a/View.go", "x\n"));
    let dir = repo(&files);
    assert_eq!(
        stdout(&driftwall(&dir, &["lint"])),
        "driftwall: clean\nnote: 1 file not read (go 1); see driftwall brief\n"
    );
}

#[test]
fn lint_json_ends_with_the_blind_spots_summary() {
    let dir = repo(&blind_files());
    let output = driftwall(&dir, &["lint", "--format", "json"]);
    assert_eq!(code(&output), 0);
    let text = stdout(&output);
    assert!(text.rfind("\"blind_spots\"") > text.rfind("\"violations\""));
    let value = json_of(&output);
    assert_eq!(
        value["blind_spots"],
        json!({
            "unread": {"pl": {"count": 2, "lines": 3}, "go": {"count": 3, "lines": 6}},
            "excluded": {"vendored": 1, "minified": 1},
            "ignored": 1,
            "skipped": {"undeclared-package": 1, "dynamic-specifier": 1},
            "unresolved": 1,
        })
    );
    assert_eq!(value["summary"], json!({"errors": 0, "warnings": 1}));
}

#[test]
fn lint_sarif_is_unchanged_by_blind_spots() {
    let dir = repo(&blind_files());
    let output = driftwall(&dir, &["lint", "--format", "sarif"]);
    assert_eq!(code(&output), 0);
    let text = stdout(&output);
    assert!(!text.contains("blind"), "{text}");
    assert!(!text.contains("not read"), "{text}");
}

#[test]
fn init_prints_the_same_note_after_what_it_wrote() {
    let mut files = untagged();
    files.push(("ios/View.go", "x\n"));
    files.push(("tools/run.pl", "#!/bin/sh\n"));
    files.push(("tools/more.pl", "echo\n"));
    let dir = repo(&files);
    let output = driftwall(&dir, &["init"]);
    assert_eq!(code(&output), 0);
    let out = stdout(&output);
    let mut lines = out.lines();
    assert!(
        lines
            .next()
            .unwrap()
            .starts_with("driftwall: wrote architecture.mmd"),
        "{out}"
    );
    assert_eq!(
        lines.next(),
        Some("note: 3 files not read (pl 2, go 1); see driftwall brief")
    );
    assert_eq!(lines.next(), None);
    // why: the init note and the lint note come from the same census.
    let lint = stdout(&driftwall(&dir, &["lint"]));
    assert!(lint.ends_with("note: 3 files not read (pl 2, go 1); see driftwall brief\n"));
}

#[test]
fn init_of_a_repo_with_nothing_blind_prints_no_note() {
    let files = untagged();
    let dir = repo(&files);
    let output = driftwall(&dir, &["init"]);
    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output).lines().count(), 1, "{}", stdout(&output));
}
