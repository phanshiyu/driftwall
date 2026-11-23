//! Every node label `init` writes names a path that exists in the repository.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    /// Whether `label` names something in `root`: the repository (`.`), a file or a directory, or a
    /// module, whose label is the path of its file without the extensions (`src/cli` for `cli.ts`).
    pub fn exists(root: &Path, label: &str) -> bool {
        if label == "." || root.join(label).exists() {
            return true;
        }
        let path = Path::new(label);
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return false;
        };
        fs::read_dir(root.join(dir)).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                let file = entry.file_name().to_string_lossy().into_owned();
                file.split('.').next() == name.to_str()
            })
        })
    }

    pub fn init_labels(files: &[(&str, &str)]) -> Vec<(String, String)> {
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .arg("init")
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let text = fs::read_to_string(dir.path().join("architecture.mmd")).unwrap();
        let contract = driftwall::diagram::parse(&text).unwrap();
        let labels: Vec<(String, String)> = contract
            .nodes
            .iter()
            .map(|(id, info)| (id.to_string(), info.label.clone().unwrap_or_default()))
            .collect();
        for (id, label) in &labels {
            assert!(
                exists(dir.path(), label),
                "node {id}: label {label:?}\n{text}"
            );
        }
        labels
    }
}

use support::init_labels;

#[test]
fn a_node_of_orphan_tests_is_labelled_with_its_directory() {
    let labels = init_labels(&[
        ("src/a.ts", "export const a = 1;\n"),
        ("src/b.ts", "export const b = 1;\n"),
        ("src/rows.test.ts", "import './a';\n"),
    ]);
    assert!(
        labels.contains(&("src".to_owned(), "src".to_owned())),
        "{labels:?}"
    );
}

#[test]
fn a_node_that_only_an_asset_names_is_labelled_with_the_asset() {
    let labels = init_labels(&[
        (
            "src/a.ts",
            "import './index.css';\nimport './fonts/f.woff2';\n",
        ),
        ("src/b.ts", "export const b = 1;\n"),
        ("src/index.css", "body {}\n"),
        ("src/fonts/f.woff2", ""),
    ]);
    assert!(
        labels.contains(&("src".to_owned(), "src/index.css".to_owned())),
        "{labels:?}"
    );
}
