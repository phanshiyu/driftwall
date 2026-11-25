//! Workspace packages and package `imports`, read from the `package.json` files of a repository.
//!
//! The IO shell is [`discover`]. Everything it returns is plain data that [`Workspaces`] turns into
//! candidate paths for a specifier; the caller probes them. See "Workspace packages" and "Package
//! `imports`" in `docs/DESIGN.md`. A `package.json` that cannot be read or parsed is skipped
//! silently, like a malformed `tsconfig.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::alias::Candidate;
use super::package::package_name;
use super::subpath::Subpaths;
use crate::code::Language;
use crate::imports::syntax::glob;
use crate::imports::syntax::paths::directories;
use crate::imports::syntax::paths::join_normalised;

const MANIFEST_FILE: &str = "package.json";

/// The fields probed, in this order, for the root of a package that has no usable `exports`.
const ENTRY_FIELDS: [&str; 3] = ["module", "main", "types"];

/// One workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    /// Repository-relative directory of the member.
    dir: PathBuf,
    /// Its `exports`, when it has a usable one.
    exports: Option<Subpaths>,
    /// The text of `module`, `main` and `types`, in that order, for those that are strings.
    fields: Vec<String>,
}

/// The workspace members of a repository by package name, and the `imports` maps of its packages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspaces {
    members: BTreeMap<String, Member>,
    imports: BTreeMap<PathBuf, Subpaths>,
}

impl Workspaces {
    /// The paths a bare `specifier` may name inside the workspace member it names, in the order to
    /// probe them. `None` when `specifier` names no member. A member whose entry cannot be found
    /// gives an empty list.
    #[must_use]
    pub fn candidates(&self, specifier: &str) -> Option<Vec<Candidate>> {
        let name = package_name(specifier);
        let member = self.members.get(name)?;
        let rest = specifier.get(name.len()..).unwrap_or_default();
        let subpath = format!(".{rest}");
        let mut found = Vec::new();
        if let Some(paths) = member
            .exports
            .as_ref()
            .and_then(|exports| exports.lookup(&subpath))
        {
            found.extend(paths.iter().filter_map(|path| inside(&member.dir, path)));
        }
        if rest.is_empty() {
            found.extend(
                member
                    .fields
                    .iter()
                    .filter_map(|field| inside(&member.dir, field)),
            );
            found.push(Candidate {
                path: member.dir.clone(),
                directory_only: true,
            });
        } else if let Some(path) = rest.strip_prefix('/') {
            found.extend(inside(&member.dir, path));
        }
        Some(found)
    }

    /// The paths a `#` `specifier` may name through the `imports` of the nearest `package.json` at
    /// or above `importer` that has one. `None` when there is none or none of its keys matches.
    #[must_use]
    pub fn imports_candidates(&self, specifier: &str, importer: &Path) -> Option<Vec<Candidate>> {
        let (dir, map) = importer
            .parent()?
            .ancestors()
            .find_map(|dir| Some((dir, self.imports.get(dir)?)))?;
        let paths = map.lookup(specifier)?;
        Some(paths.iter().filter_map(|path| inside(dir, path)).collect())
    }
}

/// `text`, a path relative to `dir`, as a candidate. `None` when it would leave `dir`.
fn inside(dir: &Path, text: &str) -> Option<Candidate> {
    let path = join_normalised(Some(dir), text)?;
    path.starts_with(dir).then(|| Candidate {
        path,
        directory_only: text.ends_with('/'),
    })
}

/// Reads the workspace members and package `imports` of the repository under `root`.
///
/// Only the directories at or above a TypeScript or JavaScript file in `files` are looked at, so a
/// member without such a file is not found and ignored trees are never consulted.
#[must_use]
pub fn discover(root: &Path, files: &BTreeMap<PathBuf, Language>) -> Workspaces {
    let sources = files
        .iter()
        .filter(|(_, language)| language.is_ecmascript())
        .map(|(path, _)| path.as_path());
    let dirs = directories(sources);
    let manifests: BTreeMap<&PathBuf, Map<String, Value>> = dirs
        .iter()
        .filter_map(|dir| Some((dir, read_manifest(&root.join(dir).join(MANIFEST_FILE))?)))
        .collect();

    let imports = manifests
        .iter()
        .filter_map(|(dir, manifest)| {
            let map = Subpaths::imports(manifest.get("imports")?)?;
            Some(((*dir).clone(), map))
        })
        .collect();

    let mut members = BTreeMap::new();
    for (base, manifest) in &manifests {
        let patterns = workspace_patterns(manifest.get("workspaces"));
        for dir in matched_dirs(base, &patterns, &dirs) {
            let Some(member_manifest) = manifests.get(&dir) else {
                continue;
            };
            let Some(name) = member_manifest
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            members
                .entry(name.to_owned())
                .or_insert_with(|| member_of(dir, member_manifest));
        }
    }
    Workspaces { members, imports }
}

fn member_of(dir: PathBuf, manifest: &Map<String, Value>) -> Member {
    Member {
        dir,
        exports: manifest.get("exports").and_then(Subpaths::exports),
        fields: ENTRY_FIELDS
            .iter()
            .filter_map(|field| manifest.get(*field)?.as_str())
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

fn read_manifest(path: &Path) -> Option<Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    match serde_json::from_str(text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// The patterns of a `workspaces` field: an array of strings, or an object whose `packages` is one.
fn workspace_patterns(value: Option<&Value>) -> Vec<&str> {
    let list = match value {
        Some(Value::Array(list)) => Some(list),
        Some(Value::Object(map)) => match map.get("packages") {
            Some(Value::Array(list)) => Some(list),
            _ => None,
        },
        _ => None,
    };
    list.into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// The directories of `dirs` strictly below `base` that `patterns` select, in order: a pattern
/// adds the directories it matches and a `!` pattern removes them.
fn matched_dirs(base: &Path, patterns: &[&str], dirs: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let mut chosen = BTreeSet::new();
    for pattern in patterns {
        let (removes, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, *pattern), |rest| (true, rest));
        for dir in dirs {
            let Ok(relative) = dir.strip_prefix(base) else {
                continue;
            };
            if relative.as_os_str().is_empty() || !glob::matches(pattern, relative) {
                continue;
            }
            if removes {
                chosen.remove(dir);
            } else {
                chosen.insert(dir.clone());
            }
        }
    }
    chosen
}
