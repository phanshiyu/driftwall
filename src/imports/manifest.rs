//! Discovery of the library crates in a repository, read from their `Cargo.toml` files.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const LIBRARY_FILE: &str = "lib.rs";
const MANIFEST_FILE: &str = "Cargo.toml";

#[derive(Debug, Deserialize)]
struct Manifest {
    package: Option<Package>,
    lib: Option<Lib>,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Lib {
    name: Option<String>,
    path: Option<PathBuf>,
}

/// Maps the name each library crate is imported by to the directory that holds its `lib.rs`.
///
/// Every known `<dir>/lib.rs` is matched with the nearest `Cargo.toml` in `<dir>` or an ancestor,
/// read from under `root`. The name is `[lib] name` when present, else `[package] name`, with `-`
/// replaced by `_`. A crate is skipped when it has no manifest, the manifest cannot be read or
/// parsed, it names no crate, or its `[lib] path` points at a different file: a broken manifest
/// elsewhere in the repository must not stop the whole run. When two crates share a name the first
/// in path order wins.
#[must_use]
pub fn library_crates(root: &Path, known: &BTreeSet<PathBuf>) -> BTreeMap<String, PathBuf> {
    let mut crates = BTreeMap::new();
    for file in known {
        if file.file_name().is_none_or(|name| name != LIBRARY_FILE) {
            continue;
        }
        let Some(dir) = file.parent() else {
            continue;
        };
        if let Some(name) = crate_name(root, dir, file) {
            crates.entry(name).or_insert_with(|| dir.to_path_buf());
        }
    }
    crates
}

fn crate_name(root: &Path, dir: &Path, lib_file: &Path) -> Option<String> {
    let manifest_dir = dir
        .ancestors()
        .find(|ancestor| root.join(ancestor).join(MANIFEST_FILE).is_file())?;
    let text = fs::read_to_string(root.join(manifest_dir).join(MANIFEST_FILE)).ok()?;
    let manifest: Manifest = toml::from_str(&text).ok()?;
    let (lib_name, lib_path) = manifest
        .lib
        .map_or((None, None), |lib| (lib.name, lib.path));
    if lib_path.is_some_and(|path| manifest_dir.join(path) != lib_file) {
        return None;
    }
    let name = lib_name.or_else(|| manifest.package.and_then(|package| package.name))?;
    Some(name.replace('-', "_"))
}
