//! `driftwall where`: which node owns a file, and which `.arch` file decided it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use driftwall::ownership::{self, Explanation};
use serde::Serialize;

use crate::args::{WhereArgs, WhereFormat};
use crate::commands::{Run, require_directory};
use crate::outcome::Outcome;

/// Explains the ownership of `args.file` under `args.root`.
///
/// # Errors
/// A root that is not a directory, a path outside the root, or an unreadable or invalid `.arch`.
pub fn run(args: &WhereArgs) -> Result<Run> {
    require_directory(&args.root)?;
    let file = inside_root(&args.root, &args.file);
    let explanation = ownership::explain(&args.root, &file)?;
    let shown = slash_path(&args.file);
    let stdout = match args.format {
        WhereFormat::Text => render_text(&shown, &explanation),
        WhereFormat::Json => render_json(&shown, &explanation)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::of_where(&explanation),
    })
}

/// Turns an absolute `file` below `root` into the root-relative path `explain` accepts.
///
/// Both are compared as given first, then with symlinks resolved (a temp dir like `/var/..` may
/// really be `/private/var/..`). Anything that still does not fit is returned unchanged so
/// `explain` reports it as outside the root.
fn inside_root(root: &Path, file: &Path) -> PathBuf {
    if !file.is_absolute() {
        return file.to_path_buf();
    }
    if let Ok(relative) = file.strip_prefix(root) {
        return relative.to_path_buf();
    }
    let resolved = fs::canonicalize(root).ok().zip(resolve(file));
    match resolved {
        Some((real_root, real_file)) => real_file
            .strip_prefix(&real_root)
            .map_or_else(|_| file.to_path_buf(), Path::to_path_buf),
        None => file.to_path_buf(),
    }
}

/// Resolves symlinks in `file`, or in its directory when the file itself does not exist.
fn resolve(file: &Path) -> Option<PathBuf> {
    fs::canonicalize(file).ok().or_else(|| {
        let parent = fs::canonicalize(file.parent()?).ok()?;
        Some(parent.join(file.file_name()?))
    })
}

/// `<file>: <node> (via <arch file>)`, or `<file>: unmapped`.
fn render_text(file: &str, explanation: &Explanation) -> String {
    match (&explanation.node, &explanation.arch_file) {
        (Some(node), Some(arch_file)) => {
            format!("{file}: {node} (via {})\n", slash_path(arch_file))
        }
        _ => format!("{file}: unmapped\n"),
    }
}

/// The JSON document of `where --format json`.
#[derive(Debug, Serialize)]
struct Answer<'a> {
    file: &'a str,
    node: Option<&'a str>,
    arch_file: Option<String>,
}

fn render_json(file: &str, explanation: &Explanation) -> Result<String> {
    let answer = Answer {
        file,
        node: explanation
            .node
            .as_ref()
            .map(driftwall::model::NodeId::as_str),
        arch_file: explanation.arch_file.as_deref().map(slash_path),
    };
    let mut out = serde_json::to_string_pretty(&answer)?;
    out.push('\n');
    Ok(out)
}

/// A path with `/` separators regardless of platform.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use driftwall::model::NodeId;

    use super::*;

    fn mapped() -> Explanation {
        Explanation {
            node: Some(NodeId::new("lint").unwrap()),
            arch_file: Some(PathBuf::from("src/lint/.arch")),
        }
    }

    fn unmapped() -> Explanation {
        Explanation {
            node: None,
            arch_file: None,
        }
    }

    #[test]
    fn text_names_node_and_arch_file() {
        assert_eq!(
            render_text("src/lint/mod.rs", &mapped()),
            "src/lint/mod.rs: lint (via src/lint/.arch)\n"
        );
    }

    #[test]
    fn text_says_unmapped() {
        assert_eq!(render_text("x.ts", &unmapped()), "x.ts: unmapped\n");
    }

    #[test]
    fn json_mapped_has_all_three_keys_pretty_printed() {
        assert_eq!(
            render_json("a.ts", &mapped()).unwrap(),
            "{\n  \"file\": \"a.ts\",\n  \"node\": \"lint\",\n  \"arch_file\": \"src/lint/.arch\"\n}\n"
        );
    }

    #[test]
    fn json_unmapped_has_nulls() {
        assert_eq!(
            render_json("a.ts", &unmapped()).unwrap(),
            "{\n  \"file\": \"a.ts\",\n  \"node\": null,\n  \"arch_file\": null\n}\n"
        );
    }

    #[test]
    fn relative_files_pass_through() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("a/b.ts")),
            Path::new("a/b.ts")
        );
    }

    #[test]
    fn absolute_files_below_the_root_become_relative() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("/r/a/b.ts")),
            Path::new("a/b.ts")
        );
    }

    #[test]
    fn absolute_files_elsewhere_are_left_for_explain_to_reject() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("/other/b.ts")),
            Path::new("/other/b.ts")
        );
    }
}
