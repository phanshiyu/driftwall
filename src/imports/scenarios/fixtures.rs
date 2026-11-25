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

/// Writes `files` (relative path, contents). TypeScript, JavaScript, Python, Rust, Swift, shell and Svelte files, and HTML files
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
            Some("rs") => Some(Language::Rust),
            Some("svelte") => Some(Language::Svelte),
            Some("swift") => Some(Language::Swift),
            Some("sh" | "bash" | "zsh") => Some(Language::Shell),
            Some("html" | "htm") if crate::imports::web::has_script(content) => {
                Some(Language::Html)
            }
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

    /// The source files and their languages.
    pub fn files(&self) -> BTreeMap<PathBuf, Language> {
        self.files.clone()
    }

    /// Every asset.
    pub fn assets(&self) -> BTreeSet<PathBuf> {
        self.assets.clone()
    }

    /// The imports of every source file.
    pub fn collect(&self) -> BTreeMap<PathBuf, FileImports> {
        collect_with_assets(
            self.dir.path(),
            &self.files,
            &self.assets,
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect("collect succeeds")
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

/// The Rust part of an environment for a test of another language: no crates, modules or
/// dependencies. Leaked, so a language's test names no other language.
pub fn no_rust() -> crate::imports::rust::Env<'static> {
    crate::imports::rust::Env {
        crates: Box::leak(Box::default()),
        paths: Box::leak(Box::default()),
        dependencies: Box::leak(Box::default()),
    }
}

/// The TypeScript and JavaScript part of an environment for a test of another language: no
/// configuration at all. Leaked, so a language's test names no other language.
pub fn no_web() -> crate::imports::web::Env<'static> {
    crate::imports::web::Env {
        aliases: Box::leak(Box::default()),
        packages: Box::leak(Box::default()),
        workspaces: Box::leak(Box::default()),
        projects: Box::leak(Box::default()),
        vite_roots: Box::leak(Box::default()),
        vite_aliases: Box::leak(Box::default()),
    }
}
