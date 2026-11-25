//! `%% facade <node>`: callers of a deliberate facade depend on it, not on what it re-exports (see
//! "Re-exports" in `docs/DESIGN.md`).

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    /// A page that reaches `types` through `lib`, whose `wire.ts` re-exports it.
    pub fn repo(extra: &str, page: &str) -> TempDir {
        let contract = format!(
            "flowchart LR\n  page\n  lib\n  types\n  page --> lib\n  lib --> types\n{extra}"
        );
        let dir = TempDir::new().unwrap();
        for (path, contents) in [
            ("architecture.mmd", contract.as_str()),
            ("page/.arch", "node: page\n"),
            ("lib/.arch", "node: lib\n"),
            ("types/.arch", "node: types\n"),
            ("page/main.ts", page),
            (
                "lib/wire.ts",
                "export type { Comment, Secret } from '../types/types';\nexport const own = 1;\n",
            ),
            (
                "types/types.ts",
                "export interface Comment {}\nexport interface Secret {}\n",
            ),
        ] {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    pub fn driftwall(dir: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(args)
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }
}

use support::{driftwall, repo, stdout};

const PAGE: &str = "import type { Comment } from '../lib/wire';\nexport const x: Comment | undefined = undefined;\n";

#[test]
fn without_a_facade_the_caller_depends_on_the_definition() {
    let dir = repo("", PAGE);
    let text = stdout(&driftwall(&dir, &["lint"]));
    assert!(
        text.contains("`page` imports `types` (types/types.ts)"),
        "{text}"
    );
}

#[test]
fn with_a_facade_the_caller_depends_on_the_facade_and_the_facade_keeps_its_edge() {
    let dir = repo("  %% facade lib\n  %% exports lib: Comment\n", PAGE);
    let output = driftwall(&dir, &["lint", "--strict"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("clean"), "{text}");
}

#[test]
fn a_surface_is_judged_at_the_facade_node() {
    let dir = repo(
        "  %% facade lib\n  %% exports lib: own\n",
        "import type { Comment } from '../lib/wire';\nexport const x = 1;\n",
    );
    let text = stdout(&driftwall(&dir, &["lint"]));
    assert!(text.contains("private-symbol"), "{text}");
    assert!(
        text.contains("`page` imports `Comment` from `lib`"),
        "{text}"
    );
}

#[test]
fn a_facade_that_re_exports_nothing_is_stale() {
    let dir = repo("  %% facade page\n", PAGE);
    let text = stdout(&driftwall(&dir, &["lint"]));
    assert!(text.contains("warning[stale-facade]"), "{text}");
    assert!(text.contains("%% facade page"), "{text}");
}

#[test]
fn a_facade_without_an_exports_line_is_warned_about_with_its_line() {
    let dir = repo("  %% facade lib\n", PAGE);
    let output = driftwall(&dir, &["lint"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains(
            "warning[facade-without-exports]: `%% facade lib` on line 7 of architecture.mmd makes `lib` a facade, but no `%% exports lib` line states its surface, so every name it re-exports is public"
        ),
        "{text}"
    );
    assert!(
        text.contains("hint: list the names other nodes may use with `%% exports <node>: ...`, or ask the user to approve dropping the facade"),
        "{text}"
    );
    // why: the finding has no source location, so the line starts with the severity.
    assert!(
        text.starts_with("warning[facade-without-exports]"),
        "{text}"
    );
    let strict = driftwall(&dir, &["lint", "--strict"]);
    assert_eq!(strict.status.code(), Some(1), "{}", stdout(&strict));
}

#[test]
fn a_facade_with_an_exports_line_is_silent() {
    let dir = repo("  %% facade lib\n  %% exports lib: Comment\n", PAGE);
    let text = stdout(&driftwall(&dir, &["lint"]));
    assert!(!text.contains("facade-without-exports"), "{text}");
}

#[test]
fn each_facade_node_without_exports_is_one_finding() {
    let dir = repo("  %% facade lib\n  %% facade types\n", PAGE);
    let text = stdout(&driftwall(&dir, &["lint"]));
    assert_eq!(
        text.matches("[facade-without-exports]").count(),
        2,
        "{text}"
    );
    let one = repo(
        "  %% facade lib, types\n  %% exports types: Comment\n",
        PAGE,
    );
    let text = stdout(&driftwall(&one, &["lint"]));
    assert_eq!(
        text.matches("[facade-without-exports]").count(),
        1,
        "{text}"
    );
    assert!(text.contains("`%% facade lib`"), "{text}");
}

#[test]
fn the_json_and_the_baseline_key_the_finding_by_its_node() {
    let dir = repo("  %% facade lib\n", PAGE);
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&driftwall(&dir, &["lint", "--format", "json"]))).unwrap();
    let found = json["violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "facade-without-exports")
        .unwrap();
    assert_eq!(found["key"], "facade-without-exports lib");
    assert_eq!(found["severity"], "warning");
    let baseline = dir.path().join("debt.txt");
    let written = driftwall(
        &dir,
        &["lint", "--write-baseline", baseline.to_str().unwrap()],
    );
    assert!(written.status.success(), "{}", stdout(&written));
    let text = std::fs::read_to_string(&baseline).unwrap();
    assert!(text.contains("facade-without-exports 1 lib\n"), "{text}");
    let strict = driftwall(
        &dir,
        &["lint", "--strict", "--baseline", baseline.to_str().unwrap()],
    );
    assert!(strict.status.success(), "{}", stdout(&strict));
}

#[test]
fn sarif_lists_the_rule() {
    let dir = repo("  %% facade lib\n", PAGE);
    let sarif = stdout(&driftwall(&dir, &["lint", "--format", "sarif"]));
    assert!(sarif.contains("\"facade-without-exports\""), "{sarif}");
}

#[test]
fn an_unknown_facade_node_is_a_contract_error_with_its_line() {
    let dir = repo("  %% facade nope\n", PAGE);
    let output = driftwall(&dir, &["lint"]);
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        error.contains("nope") && error.contains("line 7"),
        "{error}"
    );
}

#[test]
fn brief_shows_the_facades() {
    let dir = repo("  %% facade lib\n  %% facade types\n", PAGE);
    let text = stdout(&driftwall(&dir, &["brief"]));
    assert!(text.contains("rules:\n  facade: lib, types\n"), "{text}");
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&driftwall(&dir, &["brief", "--format", "json"]))).unwrap();
    assert_eq!(
        json["rules"]["facades"],
        serde_json::json!(["lib", "types"])
    );
}
