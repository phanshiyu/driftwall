//! The IO half of `init`: conflict checks, then writes in a fixed order.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagram;
use crate::model::{DirTag, SCOPE_HERE, Scope};

use crate::model::BlindSummary;

use super::{InitError, InitPlan, InitSummary, WriteOptions, present};

/// Name of the file that tags a directory with a node.
const ARCH_FILE: &str = ".arch";

/// Checks every conflict (a path `init` would write that already exists), then writes the contract followed by the tags in directory order.
pub fn apply(
    root: &Path,
    baseline: &InitPlan,
    write: &WriteOptions,
) -> Result<InitSummary, InitError> {
    let rendered = present::render(&baseline.contract, &baseline.test_only);
    if diagram::parse(&rendered)? != baseline.contract {
        return Err(InitError::RoundTrip);
    }

    let tag_paths: Vec<(PathBuf, String)> = if write.write_tags {
        baseline
            .tags
            .iter()
            .map(|(dir, found)| (dir.join(ARCH_FILE), arch_text(found)))
            .collect()
    } else {
        Vec::new()
    };

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
        blind_spots: BlindSummary::default(),
    })
}

/// The contents of the `.arch` file for `found`: its `node:` line, then `scope: here` when the tag
/// does not reach below its directory.
fn arch_text(found: &DirTag) -> String {
    let scope = match found.scope {
        Scope::Subtree => String::new(),
        Scope::Here => format!("scope: {SCOPE_HERE}\n"),
    };
    format!("node: {}\n{scope}", found.tag)
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
