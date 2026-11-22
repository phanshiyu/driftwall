//! Repositories written to a temporary directory, for tests that go through [`collect`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;
use crate::model::Language;

/// A repository on disk and what the scan would know about it.
pub struct Repo {
    dir: TempDir,
    files: BTreeMap<PathBuf, Language>,
    assets: BTreeSet<PathBuf>,
}

/// Writes `files` (relative path, contents). TypeScript, JavaScript, Python and Svelte files, and HTML files
/// that hold a `<script` element, are source and every other file is an asset.
pub fn repo(files: &[(&str, &str)]) -> Repo {
    let dir = TempDir::new().expect("tempdir");
    let mut known = BTreeMap::new();
    let mut assets = BTreeSet::new();
    for (path, content) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(&full, content).expect("write");
        let language = match Path::new(path).extension().and_then(|e| e.to_str()) {
            Some("ts" | "tsx" | "mts" | "cts") => Some(Language::TypeScript),
            Some("js" | "jsx" | "mjs" | "cjs") => Some(Language::JavaScript),
            Some("py") => Some(Language::Python),
            Some("svelte") => Some(Language::Svelte),
            Some("html" | "htm") if crate::markup::has_script(content) => Some(Language::Html),
            _ => None,
        };
        match language {
            Some(language) => {
                known.insert(PathBuf::from(path), language);
            }
            None => {
                assets.insert(PathBuf::from(path));
            }
        }
    }
    Repo {
        dir,
        files: known,
        assets,
    }
}

impl Repo {
    /// The root directory the repository was written to.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The imports of every source file.
    pub fn collect(&self) -> BTreeMap<PathBuf, FileImports> {
        collect_with_assets(self.dir.path(), &self.files, &self.assets).expect("collect succeeds")
    }

    /// The imports of `file`.
    pub fn imports(&self, file: &str) -> FileImports {
        self.collect()
            .remove(Path::new(file))
            .expect("file is a source file of the repository")
    }

    /// The resolved imports of `file` as (line, target).
    pub fn edges(&self, file: &str) -> Vec<(u32, String)> {
        self.imports(file)
            .resolved
            .iter()
            .map(|import| (import.at.line, import.target.display().to_string()))
            .collect()
    }

    /// The unresolved specifiers of `file` as (line, specifier).
    pub fn unresolved(&self, file: &str) -> Vec<(u32, String)> {
        self.imports(file)
            .unresolved
            .iter()
            .map(|import| (import.at.line, import.specifier.clone()))
            .collect()
    }

    /// The skipped specifiers of `file` as (line, specifier).
    pub fn skipped(&self, file: &str) -> Vec<(u32, String)> {
        self.imports(file)
            .skipped
            .iter()
            .map(|skip| (skip.at.line, skip.specifier.clone()))
            .collect()
    }
}

/// `(line, text)` pairs from literals.
pub fn pairs(entries: &[(u32, &str)]) -> Vec<(u32, String)> {
    entries
        .iter()
        .map(|&(line, text)| (line, text.to_owned()))
        .collect()
}
