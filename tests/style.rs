//! Style rules no linter covers. Plain `//` comments must justify themselves with `// why:`.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn plain_comments_must_start_with_why() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files.sort();
    let mut offenders = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let plain = trimmed.starts_with("//")
                && !trimmed.starts_with("///")
                && !trimmed.starts_with("//!")
                && !trimmed.starts_with("// why:");
            if plain {
                offenders.push(format!("{}:{}: {}", file.display(), index + 1, trimmed));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "comments must be `// why: <reason>`:\n{}",
        offenders.join("\n")
    );
}
