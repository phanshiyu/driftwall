//! End to end: the `dirs` of a node are where its code lives, not where its `.arch` file lies.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    const CONTRACT: &str = "flowchart LR
  root[\"root\"]
  src-ui[\"src-ui\"]
  src-gitlab[\"src-gitlab\"]
  tools[\"tools\"]
  x[\"x\"]
  y[\"y\"]
";

    /// A repository with a root group, a prefixed `node: *` directory, a plain directory tag and a
    /// `node: *` directory that holds a module directory and a file module.
    pub fn repo() -> TempDir {
        let files = [
            ("architecture.mmd", CONTRACT),
            (".arch", "node: root\nscope: here\n"),
            ("main.ts", "export {};\n"),
            ("src/.arch", "node: src-*\n"),
            ("src/ui/a.ts", "export {};\n"),
            ("src/ui/deep/b.ts", "export {};\n"),
            ("src/gitlab.ts", "export {};\n"),
            ("src/gitlab.test.ts", "export {};\n"),
            ("scripts/.arch", "node: tools\n"),
            ("scripts/run.ts", "export {};\n"),
            ("scripts/deep/more.ts", "export {};\n"),
            ("lib/.arch", "node: *\n"),
            ("lib/x/z.ts", "export {};\n"),
            ("lib/y.ts", "export {};\n"),
        ];
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
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

use serde_json::json;
use support::{driftwall, repo, stdout};

#[test]
fn brief_shows_the_module_paths_of_derived_nodes_and_the_tagged_directory_of_the_others() {
    let dir = repo();
    let output = driftwall(&dir, &["brief"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    let nodes: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "nodes:")
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .collect();
    assert_eq!(
        nodes,
        [
            "  root: . (1 file)",
            "  src-gitlab: src/gitlab.test.ts, src/gitlab.ts (2 files)",
            "  src-ui: src/ui (2 files)",
            "  tools: scripts (2 files)",
            "  x: lib/x (1 file)",
            "  y: lib/y.ts (1 file)",
        ]
    );
}

#[test]
fn neighbors_and_json_show_the_same_dirs() {
    let dir = repo();
    let text = stdout(&driftwall(&dir, &["neighbors", "src-gitlab"]));
    assert!(
        text.contains("\ndirs: src/gitlab.test.ts, src/gitlab.ts\n"),
        "{text}"
    );
    let text = stdout(&driftwall(&dir, &["neighbors", "src-ui"]));
    assert!(text.contains("\ndirs: src/ui\n"), "{text}");

    let brief = driftwall(&dir, &["brief", "--format", "json"]);
    let value: serde_json::Value = serde_json::from_str(&stdout(&brief)).unwrap();
    let dirs: Vec<(String, serde_json::Value)> = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["id"].as_str().unwrap().to_owned(),
                node["dirs"].clone(),
            )
        })
        .collect();
    assert_eq!(
        dirs,
        [
            ("root".to_owned(), json!(["."])),
            (
                "src-gitlab".to_owned(),
                json!(["src/gitlab.test.ts", "src/gitlab.ts"])
            ),
            ("src-ui".to_owned(), json!(["src/ui"])),
            ("tools".to_owned(), json!(["scripts"])),
            ("x".to_owned(), json!(["lib/x"])),
            ("y".to_owned(), json!(["lib/y.ts"])),
        ]
    );
}
