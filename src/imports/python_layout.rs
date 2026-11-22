//! The shape of the Python code in a repository: which directories are packages, which hold Python
//! files, where each top-level name can be found, and the import roots the project configures.
//!
//! Pure. Built once per run from the known files and the configured roots (see
//! [`super::python_config`]), then read by the resolver for every Python import.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const INIT_FILE: &str = "__init__.py";
const SOURCE_EXTENSION: &str = "py";

/// What the resolver needs to know about the Python files of the repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PythonLayout {
    /// Directory holding configuration to the roots it configures, repository-relative and in
    /// the order they apply.
    configured: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// Directories that hold an `__init__.py`.
    packages: BTreeSet<PathBuf>,
    /// Directories with a Python file somewhere below them, at any depth.
    directories: BTreeSet<PathBuf>,
    /// Top-level name to the first repository path in path order that can supply it: a
    /// `<name>.py` file or a directory named `<name>` with a Python file below it.
    names: BTreeMap<String, PathBuf>,
}

impl PythonLayout {
    /// The layout of the Python files among `known`, with the roots `configured` by the
    /// configuration directory that names them (see [`super::python_roots`]).
    #[must_use]
    pub fn new(known: &BTreeSet<PathBuf>, configured: BTreeMap<PathBuf, Vec<PathBuf>>) -> Self {
        let mut layout = Self {
            configured,
            ..Self::default()
        };
        for file in known {
            if file.extension().is_none_or(|ext| ext != SOURCE_EXTENSION) {
                continue;
            }
            if file.file_name().is_some_and(|name| name == INIT_FILE) {
                layout.packages.extend(file.parent().map(Path::to_path_buf));
            } else if let Some(stem) = file.file_stem().and_then(|stem| stem.to_str()) {
                layout.offer(stem, file);
            }
            for dir in file.ancestors().skip(1) {
                let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                layout.directories.insert(dir.to_path_buf());
                layout.offer(name, dir);
            }
        }
        layout
    }

    /// Records `path` as a place the top-level `name` can come from, keeping the first in path
    /// order.
    fn offer(&mut self, name: &str, path: &Path) {
        match self.names.get_mut(name) {
            Some(known) if path < known.as_path() => path.clone_into(known),
            Some(_) => {}
            None => {
                self.names.insert(name.to_owned(), path.to_path_buf());
            }
        }
    }

    /// The import roots of `importer`, repository-relative, first to try first: its own directory
    /// when that is not a package, the roots of the configuration directories at or above it (the
    /// nearest first), the repository root and `src/`. A root appears once, at its first place.
    #[must_use]
    pub fn roots(&self, importer: &Path) -> Vec<PathBuf> {
        let dir = importer.parent().unwrap_or_else(|| Path::new(""));
        let own = (!self.packages.contains(dir)).then(|| dir.to_path_buf());
        let configured = dir
            .ancestors()
            .filter_map(|ancestor| self.configured.get(ancestor))
            .flatten()
            .cloned();
        let mut roots: Vec<PathBuf> = Vec::new();
        for root in own
            .into_iter()
            .chain(configured)
            .chain([PathBuf::new(), PathBuf::from("src")])
        {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        roots
    }

    /// Whether `dir` has a Python file somewhere below it: a package, or a namespace package.
    #[must_use]
    pub fn is_directory(&self, dir: &Path) -> bool {
        self.directories.contains(dir)
    }

    /// The first repository path, in path order, that a top-level `name` can come from.
    #[must_use]
    pub fn matching(&self, name: &str) -> Option<&Path> {
        self.names.get(name).map(PathBuf::as_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(files: &[&str], configured: &[(&str, &[&str])]) -> PythonLayout {
        let known = files.iter().map(PathBuf::from).collect();
        let configured = configured
            .iter()
            .map(|(dir, roots)| {
                (
                    PathBuf::from(dir),
                    roots.iter().map(PathBuf::from).collect(),
                )
            })
            .collect();
        PythonLayout::new(&known, configured)
    }

    fn roots(layout: &PythonLayout, importer: &str) -> Vec<String> {
        layout
            .roots(Path::new(importer))
            .iter()
            .map(|root| root.display().to_string())
            .collect()
    }

    #[test]
    fn a_file_in_a_plain_directory_has_its_own_directory_first() {
        let layout = layout(&["scripts/a.py"], &[]);
        assert_eq!(roots(&layout, "scripts/a.py"), ["scripts", "", "src"]);
    }

    #[test]
    fn a_file_in_a_package_directory_has_no_own_root() {
        let layout = layout(&["pkg/__init__.py", "pkg/a.py"], &[]);
        assert_eq!(roots(&layout, "pkg/a.py"), ["", "src"]);
    }

    #[test]
    fn a_file_at_the_repository_root_has_the_root_once() {
        let layout = layout(&["main.py"], &[]);
        assert_eq!(roots(&layout, "main.py"), ["", "src"]);
    }

    #[test]
    fn configured_roots_come_nearest_first_between_own_and_repository_root() {
        let layout = layout(
            &["svc/api/x/m.py"],
            &[
                ("", &["scripts", "svc/api/x"]),
                ("svc/api", &["svc/api/lib"]),
            ],
        );
        assert_eq!(
            roots(&layout, "svc/api/x/m.py"),
            ["svc/api/x", "svc/api/lib", "scripts", "", "src"]
        );
    }

    #[test]
    fn configured_roots_only_apply_below_their_directory() {
        let layout = layout(&["a/m.py", "b/m.py"], &[("a", &["a/lib"])]);
        assert_eq!(roots(&layout, "a/m.py"), ["a", "a/lib", "", "src"]);
        assert_eq!(roots(&layout, "b/m.py"), ["b", "", "src"]);
    }

    #[test]
    fn a_name_matches_a_file_or_a_directory_with_python_below_it() {
        let layout = layout(
            &["scripts/tuner.py", "deep/er/pkg/mod.py", "__init__.py"],
            &[],
        );
        assert_eq!(
            layout.matching("tuner"),
            Some(Path::new("scripts/tuner.py"))
        );
        assert_eq!(layout.matching("er"), Some(Path::new("deep/er")));
        assert_eq!(layout.matching("pkg"), Some(Path::new("deep/er/pkg")));
        assert_eq!(
            layout.matching("mod"),
            Some(Path::new("deep/er/pkg/mod.py"))
        );
        assert_eq!(layout.matching("__init__"), None);
        assert_eq!(layout.matching("nothing"), None);
    }

    #[test]
    fn the_first_path_in_path_order_wins_a_shared_name() {
        let layout = layout(&["z/util.py", "a/b/util/x.py", "m/util.py"], &[]);
        assert_eq!(layout.matching("util"), Some(Path::new("a/b/util")));
    }

    #[test]
    fn directories_are_those_with_python_at_any_depth() {
        let layout = layout(&["a/b/c.py", "d/e.ts"], &[]);
        assert!(layout.is_directory(Path::new("a")));
        assert!(layout.is_directory(Path::new("a/b")));
        assert!(!layout.is_directory(Path::new("d")));
    }
}
