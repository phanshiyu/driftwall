//! The IO half of `init`: conflict checks, then writes in a fixed order.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagram;

use super::{InitError, InitPlan, InitSummary, WriteOptions};

/// Name of the file that tags a directory with a node.
const ARCH_FILE: &str = ".arch";

/// Checks every conflict, then writes the contract followed by the tags in directory order.
pub fn apply(
    root: &Path,
    baseline: &InitPlan,
    existing_tags: &[PathBuf],
    write: &WriteOptions,
) -> Result<InitSummary, InitError> {
    let rendered = diagram::render(&baseline.contract);
    if diagram::parse(&rendered)? != baseline.contract {
        return Err(InitError::RoundTrip);
    }

    let tag_paths: Vec<(PathBuf, String)> = if write.write_tags {
        baseline
            .tags
            .iter()
            .map(|(dir, id)| (dir.join(ARCH_FILE), format!("node: {id}\n")))
            .collect()
    } else {
        Vec::new()
    };

    if write.write_tags {
        let mut shadowing: Vec<PathBuf> = existing_tags
            .iter()
            .filter(|tag| {
                let at = tag.parent().unwrap_or_else(|| Path::new(""));
                !baseline.tags.contains_key(at)
                    && baseline
                        .tags
                        .keys()
                        .any(|dir| at.starts_with(dir) && at != dir)
            })
            .cloned()
            .collect();
        shadowing.sort();
        if !shadowing.is_empty() {
            return Err(InitError::Shadowed { paths: shadowing });
        }
    }

    if !write.force {
        let mut conflicts: Vec<PathBuf> = std::iter::once(&write.contract)
            .chain(tag_paths.iter().map(|(path, _)| path))
            .filter(|path| fs::symlink_metadata(root.join(path)).is_ok())
            .cloned()
            .collect();
        conflicts.sort();
        if !conflicts.is_empty() {
            return Err(InitError::Conflict { paths: conflicts });
        }
    }

    write_file(root, &write.contract, &rendered)?;
    for (path, contents) in &tag_paths {
        write_file(root, path, contents)?;
    }
    Ok(InitSummary {
        nodes: baseline.contract.nodes.len(),
        edges: baseline.contract.edges.len(),
        tag_files: tag_paths.len(),
        contract: write.contract.clone(),
    })
}

fn write_file(root: &Path, relative: &Path, contents: &str) -> Result<(), InitError> {
    let full = root.join(relative);
    let io_error = |source| InitError::Io {
        path: full.clone(),
        source,
    };
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    fs::write(&full, contents).map_err(io_error)
}
