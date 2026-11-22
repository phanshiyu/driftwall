//! Code units: the directories a manifest or the layout names as one buildable or importable
//! thing. Detection is the IO half (manifests are read here); deciding what to do with the units
//! is pure and lives in `claims`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::model::Language;

const CARGO_MANIFEST: &str = "Cargo.toml";
/// File names that make a directory a JavaScript unit.
const JAVASCRIPT_MARKERS: [&str; 3] = ["package.json", "tsconfig.json", "jsconfig.json"];
const PYTHON_INIT: &str = "__init__.py";
/// Directory that holds a Rust crate's source.
pub(super) const CRATE_SOURCE: &str = "src";

/// What makes a directory a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnitKind {
    /// A Rust crate: a `[package]` manifest next to a `src/` directory, or a workspace member.
    Crate,
    /// A JavaScript or TypeScript unit: a directory with a `package.json`, `tsconfig.json` or
    /// `jsconfig.json`.
    Package,
    /// An outermost Python package: a directory with an `__init__.py` and no package above it.
    PythonPackage,
}

/// A directory that is one code unit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unit {
    /// Repository-relative directory, empty for the repository root.
    pub dir: PathBuf,
    /// What makes it a unit.
    pub kind: UnitKind,
}

#[derive(Deserialize)]
struct CargoManifest {
    package: Option<IgnoredAny>,
    workspace: Option<CargoWorkspace>,
}

#[derive(Deserialize)]
struct CargoWorkspace {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

/// Finds the crate and JavaScript units of the repository, from the manifests that sit in a
/// directory holding or above a file of `files`, read under `root`.
///
/// A `Cargo.toml` that cannot be read or parsed is skipped: a broken manifest elsewhere in the
/// repository must not stop `init`. A directory with a `package.json`, `tsconfig.json` or
/// `jsconfig.json` is a JavaScript unit whatever those files hold. Directories named by a
/// workspace list but missing on disk are not units. The result is sorted.
#[must_use]
pub fn detect_units(root: &Path, files: &BTreeMap<PathBuf, Language>) -> Vec<Unit> {
    let dirs: BTreeSet<&Path> = files
        .keys()
        .flat_map(|file| file.ancestors().skip(1))
        .collect();
    let mut found = BTreeSet::new();
    for dir in dirs {
        found.extend(crates_at(root, dir));
        found.extend(package_at(root, dir));
    }
    found.into_iter().collect()
}

/// The Python package units among `files`: every directory with an `__init__.py` that has no
/// ancestor directory with one. Sorted.
pub(super) fn python_units<'a>(files: impl Iterator<Item = &'a PathBuf>) -> Vec<Unit> {
    let packages: BTreeSet<&Path> = files
        .filter(|file| file.file_name().is_some_and(|name| name == PYTHON_INIT))
        .filter_map(|file| file.parent())
        .collect();
    packages
        .iter()
        .filter(|dir| {
            !dir.ancestors()
                .skip(1)
                .any(|above| packages.contains(above))
        })
        .map(|dir| Unit {
            dir: dir.to_path_buf(),
            kind: UnitKind::PythonPackage,
        })
        .collect()
}

fn crates_at(root: &Path, dir: &Path) -> BTreeSet<Unit> {
    let Some(manifest) = read_manifest::<CargoManifest, _>(root, dir, CARGO_MANIFEST, |text| {
        toml::from_str(text).ok()
    }) else {
        return BTreeSet::new();
    };
    let mut crates = BTreeSet::new();
    if manifest.package.is_some() && root.join(dir).join(CRATE_SOURCE).is_dir() {
        crates.insert(crate_unit(dir));
    }
    if let Some(workspace) = manifest.workspace {
        for member in members(root, dir, &workspace.members, &workspace.exclude) {
            if root.join(&member).join(CARGO_MANIFEST).is_file() {
                crates.insert(crate_unit(&member));
            }
        }
    }
    crates
}

fn crate_unit(dir: &Path) -> Unit {
    Unit {
        dir: dir.to_path_buf(),
        kind: UnitKind::Crate,
    }
}

/// The JavaScript unit at `dir`, when a marker file sits there.
fn package_at(root: &Path, dir: &Path) -> Option<Unit> {
    JAVASCRIPT_MARKERS
        .iter()
        .any(|marker| root.join(dir).join(marker).is_file())
        .then(|| Unit {
            dir: dir.to_path_buf(),
            kind: UnitKind::Package,
        })
}

fn read_manifest<T, F: FnOnce(&str) -> Option<T>>(
    root: &Path,
    dir: &Path,
    name: &str,
    parse: F,
) -> Option<T> {
    let text = fs::read_to_string(root.join(dir).join(name)).ok()?;
    parse(&text)
}

/// The directories `include` names below `base`, minus those `exclude` names.
fn members(root: &Path, base: &Path, include: &[String], exclude: &[String]) -> BTreeSet<PathBuf> {
    let mut dirs: BTreeSet<PathBuf> = include
        .iter()
        .flat_map(|pattern| expand(root, base, pattern))
        .collect();
    for pattern in exclude {
        for dir in expand(root, base, pattern) {
            dirs.remove(&dir);
        }
    }
    dirs
}

/// The existing directories below `base` that `pattern` names. `*` and `?` match inside one path
/// segment; `..` and `**` segments match nothing.
fn expand(root: &Path, base: &Path, pattern: &str) -> BTreeSet<PathBuf> {
    let mut frontier = BTreeSet::from([base.to_path_buf()]);
    for segment in pattern
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if segment == ".." || segment == "**" {
            return BTreeSet::new();
        }
        frontier = if segment.contains(['*', '?']) {
            let wanted: Vec<char> = segment.chars().collect();
            frontier
                .iter()
                .flat_map(|dir| {
                    child_directories(root, dir)
                        .into_iter()
                        .filter(|name| !name.starts_with('.') || segment.starts_with('.'))
                        .filter(|name| matches_glob(&wanted, &name.chars().collect::<Vec<_>>()))
                        .map(|name| dir.join(name))
                        .collect::<Vec<_>>()
                })
                .collect()
        } else {
            frontier
                .iter()
                .map(|dir| dir.join(segment))
                .filter(|dir| root.join(dir).is_dir())
                .collect()
        };
    }
    frontier
        .into_iter()
        .map(|dir| dir.components().filter(is_normal).collect())
        .collect()
}

const fn is_normal(component: &Component<'_>) -> bool {
    matches!(component, Component::Normal(_))
}

fn child_directories(root: &Path, dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// Whether `text` matches `pattern`, where `*` is any run of characters and `?` any one.
fn matches_glob(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => {
            (0..=text.len()).any(|skip| matches_glob(rest, text.get(skip..).unwrap_or_default()))
        }
        Some(('?', rest)) => text
            .split_first()
            .is_some_and(|(_, tail)| matches_glob(rest, tail)),
        Some((wanted, rest)) => text
            .split_first()
            .is_some_and(|(found, tail)| found == wanted && matches_glob(rest, tail)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tree(entries: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, contents) in entries {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    fn files(paths: &[&str]) -> BTreeMap<PathBuf, Language> {
        paths
            .iter()
            .map(|path| (PathBuf::from(path), Language::Rust))
            .collect()
    }

    fn dirs_of(units: &[Unit], kind: UnitKind) -> Vec<String> {
        units
            .iter()
            .filter(|unit| unit.kind == kind)
            .map(|unit| unit.dir.display().to_string())
            .collect()
    }

    const PACKAGE: &str = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n";

    #[test]
    fn glob_matches_stars_and_question_marks_inside_one_segment() {
        let glob = |pattern: &str, text: &str| {
            matches_glob(
                &pattern.chars().collect::<Vec<_>>(),
                &text.chars().collect::<Vec<_>>(),
            )
        };
        assert!(glob("*", "core"));
        assert!(glob("app-*", "app-web"));
        assert!(glob("a?c", "abc"));
        assert!(glob("*-core", "x-core"));
        assert!(!glob("a?c", "ac"));
        assert!(!glob("app-*", "web-app"));
        assert!(!glob("core", "cores"));
    }

    #[test]
    fn a_package_with_a_source_directory_is_a_crate() {
        let dir = tree(&[("Cargo.toml", PACKAGE), ("src/main.rs", "")]);
        let units = detect_units(dir.path(), &files(&["src/main.rs"]));
        assert_eq!(dirs_of(&units, UnitKind::Crate), [""]);
    }

    #[test]
    fn a_package_without_a_source_directory_is_not_a_crate() {
        let dir = tree(&[("Cargo.toml", PACKAGE), ("main.rs", "")]);
        assert_eq!(detect_units(dir.path(), &files(&["main.rs"])), []);
    }

    #[test]
    fn workspace_members_are_expanded_and_excluded() {
        let dir = tree(&[
            (
                "engine/Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\", \"tools/cli\"]\nexclude = [\"crates/skip\"]\n",
            ),
            ("engine/crates/a/Cargo.toml", PACKAGE),
            ("engine/crates/b/Cargo.toml", PACKAGE),
            ("engine/crates/skip/Cargo.toml", PACKAGE),
            ("engine/crates/nomanifest/lib.rs", ""),
            ("engine/tools/cli/Cargo.toml", PACKAGE),
            ("engine/crates/a/src/lib.rs", ""),
        ]);
        let units = detect_units(dir.path(), &files(&["engine/crates/a/src/lib.rs"]));
        assert_eq!(
            dirs_of(&units, UnitKind::Crate),
            ["engine/crates/a", "engine/crates/b", "engine/tools/cli"]
        );
    }

    #[test]
    fn members_that_leave_the_workspace_or_use_unsupported_globs_match_nothing() {
        let dir = tree(&[
            (
                "ws/Cargo.toml",
                "[workspace]\nmembers = [\"../other\", \"**\"]\n",
            ),
            ("other/Cargo.toml", PACKAGE),
            ("ws/a/Cargo.toml", PACKAGE),
        ]);
        assert_eq!(detect_units(dir.path(), &files(&["ws/a/x.rs"])), []);
    }

    #[test]
    fn a_broken_manifest_is_skipped() {
        let dir = tree(&[("Cargo.toml", "[package\n"), ("src/main.rs", "")]);
        assert_eq!(detect_units(dir.path(), &files(&["src/main.rs"])), []);
    }

    #[test]
    fn every_directory_with_a_package_json_is_a_javascript_unit() {
        let dir = tree(&[
            ("package.json", "{\"workspaces\": [\"packages/*\"]}"),
            ("packages/web/package.json", "{}"),
            ("packages/api/package.json", "not even json"),
            ("packages/nomanifest/index.js", ""),
            ("ui/package.json", "{}"),
        ]);
        let units = detect_units(
            dir.path(),
            &files(&[
                "packages/web/index.js",
                "packages/api/x.js",
                "packages/nomanifest/index.js",
                "ui/src/a.ts",
            ]),
        );
        assert_eq!(
            dirs_of(&units, UnitKind::Package),
            ["", "packages/api", "packages/web", "ui"]
        );
    }

    #[test]
    fn a_tsconfig_or_jsconfig_makes_a_javascript_unit_too() {
        let dir = tree(&[
            ("tsconfig.json", "{}"),
            ("ui/jsconfig.json", "{}"),
            ("ui/package.json", "{}"),
            ("tools/tsconfig.base.json", "{}"),
            ("tools/x.ts", ""),
        ]);
        let units = detect_units(
            dir.path(),
            &files(&["ui/src/a.js", "tools/x.ts", "src/b.ts"]),
        );
        assert_eq!(dirs_of(&units, UnitKind::Package), ["", "ui"]);
    }

    #[test]
    fn manifests_are_only_looked_for_above_source_files() {
        let dir = tree(&[("package.json", "{}"), ("empty/package.json", "{}")]);
        let units = detect_units(dir.path(), &files(&["src/a.ts"]));
        assert_eq!(dirs_of(&units, UnitKind::Package), [""]);
    }

    #[test]
    fn a_directory_without_a_manifest_has_no_units() {
        let dir = tree(&[("index.js", "")]);
        assert_eq!(detect_units(dir.path(), &files(&["index.js"])), []);
    }

    #[test]
    fn python_units_are_the_outermost_packages() {
        let all = files(&[
            "a/__init__.py",
            "a/sub/__init__.py",
            "a/sub/deep/x.py",
            "svc/b/__init__.py",
            "svc/c/__init__.py",
            "scripts/run.py",
        ]);
        let units = python_units(all.keys());
        assert_eq!(
            dirs_of(&units, UnitKind::PythonPackage),
            ["a", "svc/b", "svc/c"]
        );
    }

    #[test]
    fn a_package_at_the_root_swallows_every_other_package() {
        let all = files(&["__init__.py", "a/__init__.py"]);
        assert_eq!(
            dirs_of(&python_units(all.keys()), UnitKind::PythonPackage),
            [""]
        );
    }
}
