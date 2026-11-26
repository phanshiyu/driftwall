//! Discovery of the crates a repository depends on, read from its `Cargo.toml` files.
//!
//! The IO shell next to [`super::manifest`]: for each directory at or above a Rust file it reads the
//! dependency tables of the manifest there. A manifest that cannot be read or parsed is left out,
//! like a malformed `package.json`: the directory has no manifest and the others still count.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use toml::Table;

use crate::code::Language;
use crate::reading::syntax::paths::directories;

/// The file that lists a crate's dependencies.
const MANIFEST_FILE: &str = "Cargo.toml";

/// The tables of a manifest, and of each `[target.<cfg>]` table, that list dependencies.
const TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// The dependencies one manifest lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependencies {
    /// The crate names as code imports them (`-` written `_`), renamed ones under their new name.
    pub names: BTreeSet<String>,
    /// Renamed dependency (`alpha = { package = "a-lib" }`): the name code imports to the package
    /// name, both written with `_` for `-`.
    pub renames: BTreeMap<String, String>,
}

/// Maps each directory that holds a readable `Cargo.toml` to its dependency crates, reading from
/// under `root`.
///
/// The crates are the names listed in `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]` and the same tables under `[target.<cfg>]`, as code imports them (`-`
/// written `_`). Only the directories at or above a Rust file of `files` are looked at.
#[must_use]
pub fn cargo_dependencies(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> BTreeMap<PathBuf, Dependencies> {
    let sources = files
        .iter()
        .filter(|(_, language)| **language == Language::Rust)
        .map(|(path, _)| path.as_path());
    directories(sources)
        .into_iter()
        .filter_map(|dir| {
            let text = fs::read_to_string(root.join(&dir).join(MANIFEST_FILE)).ok()?;
            let manifest: Table = toml::from_str(&text).ok()?;
            Some((dir, listed(&manifest)))
        })
        .collect()
}

/// The dependencies `manifest` lists.
fn listed(manifest: &Table) -> Dependencies {
    let targets = manifest
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|targets| targets.values().filter_map(toml::Value::as_table));
    let mut found = Dependencies::default();
    let entries = std::iter::once(manifest)
        .chain(targets)
        .flat_map(|table| TABLES.iter().filter_map(|name| table.get(*name)))
        .filter_map(toml::Value::as_table)
        .flat_map(|table| table.iter());
    for (key, value) in entries {
        let name = key.replace('-', "_");
        let package = value
            .get("package")
            .and_then(toml::Value::as_str)
            .map(|package| package.replace('-', "_"));
        if let Some(package) = package.filter(|package| *package != name) {
            found.renames.insert(name.clone(), package);
        }
        found.names.insert(name);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<String> {
        let manifest: Table = toml::from_str(text).unwrap();
        listed(&manifest).names.into_iter().collect()
    }

    #[test]
    fn every_dependency_table_counts_and_dashes_become_underscores() {
        let text = "[package]\nname = \"x\"\n\
                    [dependencies]\nserde-json = \"1\"\nrayon = { version = \"1\" }\n\
                    [dev-dependencies]\ntempfile = \"3\"\n\
                    [build-dependencies]\ncc = \"1\"\n\
                    [target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n\
                    [target.'cfg(windows)'.dev-dependencies]\nwinapi = \"0.3\"\n";
        assert_eq!(
            names(text),
            ["cc", "libc", "rayon", "serde_json", "tempfile", "winapi"]
        );
    }

    #[test]
    fn a_manifest_without_dependencies_lists_none() {
        assert_eq!(names("[package]\nname = \"x\"\n"), Vec::<String>::new());
    }

    #[test]
    fn a_renamed_dependency_maps_its_name_to_the_package() {
        let text = "[dependencies]\n\
                    alpha = { path = \"../a-lib\", package = \"a-lib\" }\n\
                    plain = \"1\"\n\
                    [dev-dependencies.beta-x]\npackage = \"b-lib\"\nversion = \"1\"\n\
                    [target.'cfg(unix)'.build-dependencies]\ngamma = { package = \"c\" }\n";
        let manifest: Table = toml::from_str(text).unwrap();
        let found = listed(&manifest);
        let renames: Vec<_> = found.renames.into_iter().collect();
        assert_eq!(
            renames,
            [
                ("alpha".to_owned(), "a_lib".to_owned()),
                ("beta_x".to_owned(), "b_lib".to_owned()),
                ("gamma".to_owned(), "c".to_owned()),
            ]
        );
        assert!(found.names.contains("alpha") && found.names.contains("plain"));
    }
}
