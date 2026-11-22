//! Discovery of the `tsconfig.json` and `jsconfig.json` files that define import aliases.
//!
//! The IO shell of [`super::alias`]: it finds the config files, reads them, follows `extends` and
//! hands back one [`AliasConfig`] per directory that holds a config.
//!
//! Which config governs a file is the one in its own directory or, failing that, in the nearest
//! ancestor directory, up to the repository root (see [`super::nearest`]). In one directory
//! `tsconfig.json` wins over `jsconfig.json`; the second is only read when the first is absent.
//!
//! Merging follows TypeScript. A child that sets `paths` replaces the parent's `paths` entirely.
//! `baseUrl` is resolved against the directory of the config file that defines it. When `baseUrl`
//! is set, `paths` targets are relative to it; otherwise they are relative to the directory of
//! the config file that defines `paths`.
//!
//! Choices where TypeScript is not directly applicable:
//!
//! - Only `extends` written as a relative file path (`./base`, `../tsconfig.base.json`, with or
//!   without `.json`) is followed. A package (`@tsconfig/node18`), an array, a path that leaves
//!   the repository, and a file that is missing or malformed are ignored, keeping what the
//!   configs followed so far define. A cycle stops at the first repeated file, and a chain stops
//!   after [`MAX_EXTENDS_DEPTH`] files.
//! - A solution-style config (`"files": []` next to non-empty `references`) that defines neither
//!   `paths` nor `baseUrl` after `extends` is not a config for the files below it: it is skipped,
//!   so the nearest ancestor config still applies, except for the files a referenced project
//!   includes (see [`super::project`]). Any other config without `paths` or `baseUrl` is kept as an
//!   empty config and hides ancestor aliases, as it does in TypeScript.
//! - A config that cannot be parsed, or whose `compilerOptions.baseUrl` or `paths` has the wrong
//!   type, is skipped silently, like a malformed `Cargo.toml` in [`super::library_crates`]: the
//!   directory has no config and ancestors apply. It is never fatal. A directory whose
//!   `tsconfig.json` is broken does not fall back to its `jsconfig.json`.
//! - A `baseUrl` that points outside the repository gives an empty config, since no alias can
//!   name a repository file.
//! - `include`, `exclude` and `files` do not decide which files a config governs, except for a
//!   referenced project (see [`super::project`]). Otherwise only the directory does.
//!
//! The files are JSONC. Comments, trailing commas and a leading byte order mark are accepted.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use jsonc_parser::ParseOptions;
use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::Value;

use super::alias::{AliasConfig, Pattern};
use super::project::{self, Project};
use super::resolve::join_normalised;
use crate::model::Language;

/// Config file names in the order they are looked for in one directory.
const CONFIG_FILES: [&str; 2] = ["tsconfig.json", "jsconfig.json"];

/// The most config files followed through `extends`, the first included.
pub const MAX_EXTENDS_DEPTH: usize = 16;

/// Reads a repository-relative file. `None` when it does not exist or cannot be read.
pub(super) type Reader<'a> = &'a dyn Fn(&Path) -> Option<String>;

/// Maps each directory that holds a usable `tsconfig.json` or `jsconfig.json` to the aliases it
/// defines, reading from under `root`.
///
/// Only the directories at or above a TypeScript or JavaScript file in `files` are looked at. See
/// the module documentation for how configs are merged and which are skipped.
#[must_use]
pub fn alias_configs(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> BTreeMap<PathBuf, AliasConfig> {
    let sources = files
        .iter()
        .filter(|(_, language)| matches!(language, Language::TypeScript | Language::JavaScript))
        .map(|(path, _)| path.as_path());
    discover(&directories(sources), &|path| {
        fs::read_to_string(root.join(path)).ok()
    })
}

/// Maps each directory that holds a solution-style `tsconfig.json` to the projects its `references`
/// name, reading from under `root`. A file is governed by the first project that includes it.
#[must_use]
pub fn project_configs(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> BTreeMap<PathBuf, Vec<Project>> {
    let sources = files
        .iter()
        .filter(|(_, language)| matches!(language, Language::TypeScript | Language::JavaScript))
        .map(|(path, _)| path.as_path());
    project::discover(&directories(sources), &|path| {
        fs::read_to_string(root.join(path)).ok()
    })
}

/// Every directory at or above a file in `files`, the repository root (the empty path) included.
pub(super) fn directories<'a>(files: impl IntoIterator<Item = &'a Path>) -> BTreeSet<PathBuf> {
    files
        .into_iter()
        .filter_map(Path::parent)
        .flat_map(Path::ancestors)
        .map(Path::to_path_buf)
        .collect()
}

/// Reads the config of each directory in `dirs` through `read`.
pub(super) fn discover(
    dirs: &BTreeSet<PathBuf>,
    read: Reader<'_>,
) -> BTreeMap<PathBuf, AliasConfig> {
    dirs.iter()
        .filter_map(|dir| Some((dir.clone(), load_directory(dir, read)?)))
        .collect()
}

/// The config file of `dir` with its text: `tsconfig.json`, else `jsconfig.json`.
pub(super) fn find_config(dir: &Path, read: Reader<'_>) -> Option<(PathBuf, String)> {
    CONFIG_FILES
        .iter()
        .map(|name| dir.join(name))
        .find_map(|path| read(&path).map(|text| (path, text)))
}

fn load_directory(dir: &Path, read: Reader<'_>) -> Option<AliasConfig> {
    let (path, text) = find_config(dir, read)?;
    let leaf = parse(&text)?;
    let solution_style = leaf.is_solution_style();
    effective(&layers(path, leaf, read), solution_style)
}

/// The options of one config file in an `extends` chain.
pub(super) struct Layer {
    /// Repository-relative directory of the file.
    pub(super) dir: PathBuf,
    base_url: Option<String>,
    paths: Option<Paths>,
    /// The `files`, `include` and `exclude` lists the file sets, each relative to `dir`.
    pub(super) files: Option<Vec<String>>,
    pub(super) include: Option<Vec<String>>,
    pub(super) exclude: Option<Vec<String>>,
}

/// The config file at `path` followed by the files it extends, nearest first.
pub(super) fn layers(path: PathBuf, leaf: RawConfig, read: Reader<'_>) -> Vec<Layer> {
    let mut layers = Vec::new();
    let mut seen = vec![path.clone()];
    let mut current = Some((path, leaf));
    while let Some((file, raw)) = current.take() {
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let next = extends_target(raw.extends.as_ref(), &dir);
        let (files, include, exclude) = (
            string_list(raw.files.as_ref()),
            string_list(raw.include.as_ref()),
            string_list(raw.exclude.as_ref()),
        );
        let options = raw.compiler_options.unwrap_or_default();
        layers.push(Layer {
            dir,
            base_url: options.base_url,
            paths: options.paths,
            files,
            include,
            exclude,
        });
        current = next
            .filter(|target| layers.len() < MAX_EXTENDS_DEPTH && !seen.contains(target))
            .and_then(|target| {
                let parent = parse(&read(&target)?)?;
                seen.push(target.clone());
                Some((target, parent))
            });
    }
    layers
}

/// The file a config extends, when it is written as a relative path inside the repository.
fn extends_target(extends: Option<&Value>, dir: &Path) -> Option<PathBuf> {
    let text = extends?.as_str()?;
    if !(text.starts_with("./") || text.starts_with("../")) {
        return None;
    }
    let has_extension = Path::new(text)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    let file = if has_extension {
        text.to_owned()
    } else {
        format!("{text}.json")
    };
    join_normalised(Some(dir), &file)
}

/// Merges a chain, nearest first, into the aliases it defines. `None` for a solution-style config
/// that defines none.
pub(super) fn effective(layers: &[Layer], solution_style: bool) -> Option<AliasConfig> {
    let base_url = layers
        .iter()
        .find_map(|layer| layer.base_url.as_deref().map(|text| (layer, text)));
    let paths = layers
        .iter()
        .find_map(|layer| layer.paths.as_ref().map(|paths| (layer, paths)));
    if base_url.is_none() && paths.is_none() {
        return (!solution_style).then(AliasConfig::default);
    }
    let base_url = match base_url {
        None => None,
        Some((layer, text)) => match join_normalised(Some(&layer.dir), text) {
            Some(dir) => Some(dir),
            None => return Some(AliasConfig::default()),
        },
    };
    let patterns = paths
        .map(|(_, paths)| {
            paths
                .0
                .iter()
                .filter_map(|(key, targets)| Pattern::parse(key, targets))
                .collect()
        })
        .unwrap_or_default();
    let base = base_url
        .clone()
        .or_else(|| paths.map(|(layer, _)| layer.dir.clone()))
        .unwrap_or_default();
    Some(AliasConfig {
        base,
        bare_imports: base_url.is_some(),
        patterns,
    })
}

/// The parts of a config file that matter here. Unknown fields are ignored.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawConfig {
    extends: Option<Value>,
    compiler_options: Option<RawOptions>,
    files: Option<Value>,
    include: Option<Value>,
    exclude: Option<Value>,
    references: Option<Value>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOptions {
    base_url: Option<String>,
    paths: Option<Paths>,
}

/// The strings of a JSON array. `None` when the value is absent or not an array.
fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    match value? {
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
        ),
        _ => None,
    }
}

impl RawConfig {
    /// The `path` of each entry of `references`, in the order written.
    pub(super) fn reference_paths(&self) -> Vec<String> {
        match &self.references {
            Some(Value::Array(entries)) => entries
                .iter()
                .filter_map(|entry| Some(entry.get("path")?.as_str()?.to_owned()))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) const fn is_solution_style(&self) -> bool {
        let no_files = matches!(&self.files, Some(Value::Array(files)) if files.is_empty());
        let references = matches!(&self.references, Some(Value::Array(refs)) if !refs.is_empty());
        no_files && references
    }
}

/// The `paths` object with its keys in declaration order, which decides ties between patterns. A
/// repeated key keeps its first position and its last value, as in a JSON object.
struct Paths(Vec<(String, Vec<String>)>);

impl<'de> Deserialize<'de> for Paths {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(PathsVisitor)
    }
}

struct PathsVisitor;

impl<'de> Visitor<'de> for PathsVisitor {
    type Value = Paths;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object mapping patterns to lists of targets")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Paths, A::Error> {
        let mut entries: Vec<(String, Vec<String>)> = Vec::new();
        while let Some((key, targets)) = map.next_entry::<String, Vec<String>>()? {
            match entries.iter_mut().find(|(held, _)| *held == key) {
                Some(slot) => slot.1 = targets,
                None => entries.push((key, targets)),
            }
        }
        Ok(Paths(entries))
    }
}

/// Parses JSONC, tolerating a byte order mark. `None` for anything that is not a config object.
pub(super) fn parse(text: &str) -> Option<RawConfig> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    jsonc_parser::parse_to_serde_value(text, &ParseOptions::default()).ok()
}
