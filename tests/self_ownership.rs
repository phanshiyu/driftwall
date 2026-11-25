//! This repository's own contract maps every source file, and every node it declares owns code.
//! The mapping lives in `architecture.mmd` (`%% map` lines), so the check runs the binary on it.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and this helper is not a #[test] fn.
#[cfg(test)]
mod support {
    use std::path::Path;
    use std::process::Command;

    use serde_json::Value;

    /// `driftwall brief --format json` on this repository.
    pub fn brief() -> Value {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_driftwall"));
        // why: a hook that runs the tests must not point git at the repository being committed.
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        let output = command
            .args(["brief", "--format", "json"])
            .arg(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

use support::brief;

#[test]
fn every_source_file_has_an_owner() {
    let brief = brief();
    let unmapped = &brief["blind_spots"]["unmapped_items"];
    let mut stray = Vec::new();
    for items in unmapped.as_object().unwrap().values() {
        for item in items.as_array().into_iter().flatten() {
            let file = item["file"].as_str().unwrap_or_default();
            if file.starts_with("src/") {
                stray.push(file.to_owned());
            }
        }
    }
    assert!(stray.is_empty(), "unmapped files under src/: {stray:?}");
}

#[test]
fn every_declared_node_owns_files() {
    let brief = brief();
    let empty: Vec<&str> = brief["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["files"].as_u64() == Some(0))
        .filter_map(|node| node["id"].as_str())
        .collect();
    assert!(empty.is_empty(), "nodes that own no file: {empty:?}");
}
