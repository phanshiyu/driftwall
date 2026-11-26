//! Every Mermaid source in the repo must be drawable: no bare `%%` line before its header.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The lines of `src` before its first `flowchart` line that are exactly `%%`.
fn bare_before_header(src: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (index, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("flowchart") {
            break;
        }
        if line.trim_start() == "%%" {
            found.push(index + 1);
        }
    }
    found
}

fn files_under(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == ".git" || name == ".claude" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn no_mermaid_source_in_the_repo_has_a_bare_comment_before_its_header() {
    let mut files = Vec::new();
    files_under(root(), &mut files);
    for path in files {
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let rel = path.strip_prefix(root()).unwrap_or(&path);
        if path.extension().is_some_and(|e| e == "mmd") {
            assert!(bare_before_header(&body).is_empty(), "{}", rel.display());
        }
        let in_docs = rel.starts_with("docs") || rel.starts_with("integrations");
        let is_md = path.extension().is_some_and(|e| e == "md");
        if is_md && (in_docs || rel == Path::new("README.md")) {
            let mut block: Option<String> = None;
            for line in body.lines() {
                match (&mut block, line.trim_start()) {
                    (None, l) if l.starts_with("```mermaid") => block = Some(String::new()),
                    (Some(text), l) if l.starts_with("```") => {
                        assert!(bare_before_header(text).is_empty(), "{}", rel.display());
                        block = None;
                    }
                    (Some(text), _) => {
                        text.push_str(line);
                        text.push('\n');
                    }
                    (None, _) => {}
                }
            }
        }
    }
}

#[test]
fn the_rendered_contract_has_no_bare_comment_line() {
    let output = Command::new(env!("CARGO_BIN_EXE_driftwall"))
        .arg("render")
        .arg(root())
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{text}");
    assert!(text.lines().all(|l| l.trim_start() != "%%"), "{text}");
}
