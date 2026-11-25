//! The projects a solution-style `tsconfig.json` references, and which files each governs.
//!
//! A solution config (`"files": []` and non-empty `references`) holds no aliases of its own. Its
//! `references` name the configs that do, and a file is governed by the first of them whose
//! `files` and `include` select it. See "Governing config" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::alias::AliasConfig;
use super::glob;
use super::paths::join_normalised;
use super::tsconfig::{Layer, RawConfig, Reader, effective, find_config, layers, parse};

/// One referenced config: the aliases it defines and the files it governs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Repository-relative files listed in `files`.
    files: Vec<PathBuf>,
    /// `include` patterns, relative to the repository root.
    include: Vec<String>,
    /// `exclude` patterns, relative to the repository root.
    exclude: Vec<String>,
    config: AliasConfig,
}

impl Project {
    /// The aliases this project defines.
    #[must_use]
    pub const fn config(&self) -> &AliasConfig {
        &self.config
    }

    /// Whether this project governs `file`: it is listed in `files`, or an `include` pattern selects
    /// it and no `exclude` pattern does.
    #[must_use]
    pub fn includes(&self, file: &Path) -> bool {
        self.files.iter().any(|listed| listed == file)
            || (self
                .include
                .iter()
                .any(|pattern| glob::covers(pattern, file))
                && !self
                    .exclude
                    .iter()
                    .any(|pattern| glob::covers(pattern, file)))
    }
}

/// The projects each solution-style config in `dirs` references, in the order written. A directory
/// whose config is not solution-style, or references no readable project, has no entry.
pub(super) fn discover(
    dirs: &BTreeSet<PathBuf>,
    read: Reader<'_>,
) -> BTreeMap<PathBuf, Vec<Project>> {
    dirs.iter()
        .filter_map(|dir| {
            let projects = projects_of(dir, read);
            (!projects.is_empty()).then(|| (dir.clone(), projects))
        })
        .collect()
}

fn projects_of(dir: &Path, read: Reader<'_>) -> Vec<Project> {
    let Some((_, text)) = find_config(dir, read) else {
        return Vec::new();
    };
    let Some(solution) = parse(&text).filter(RawConfig::is_solution_style) else {
        return Vec::new();
    };
    solution
        .reference_paths()
        .iter()
        .filter_map(|reference| {
            let target = join_normalised(Some(dir), reference)?;
            referenced(&target, reference, read)
        })
        .collect()
}

/// The project at `target`: a config file when the reference names a `.json` file, else a
/// directory holding one. Nothing for a config that is missing, malformed or solution-style.
fn referenced(target: &Path, written: &str, read: Reader<'_>) -> Option<Project> {
    let is_file = Path::new(written)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    let (path, text) = if is_file {
        (target.to_path_buf(), read(target)?)
    } else {
        find_config(target, read)?
    };
    let raw = parse(&text).filter(|config| !config.is_solution_style())?;
    let chain = layers(path, raw, read);
    let config = effective(&chain, false)?;
    let leaf = chain.first()?;
    let defaults = chain
        .iter()
        .all(|layer| layer.files.is_none() && layer.include.is_none());
    let mut include = patterns(&chain, |layer| layer.include.as_ref());
    if defaults {
        // why: a config with neither `files` nor `include` includes every file below it.
        include.push(everything_below(&leaf.dir));
    }
    Some(Project {
        files: listed(&chain),
        include,
        exclude: patterns(&chain, |layer| layer.exclude.as_ref()),
        config,
    })
}

/// The first layer of the chain that sets the list `field` picks, with that list.
fn setting<'a>(
    chain: &'a [Layer],
    field: impl Fn(&'a Layer) -> Option<&'a Vec<String>>,
) -> Option<(&'a Layer, &'a Vec<String>)> {
    chain.iter().find_map(|layer| Some((layer, field(layer)?)))
}

/// The patterns of the nearest layer that sets them, made relative to the repository root.
fn patterns<'a>(
    chain: &'a [Layer],
    field: impl Fn(&'a Layer) -> Option<&'a Vec<String>>,
) -> Vec<String> {
    setting(chain, field).map_or_else(Vec::new, |(layer, list)| {
        list.iter()
            .filter_map(|pattern| join_normalised(Some(&layer.dir), pattern))
            .map(|path| slash(&path))
            .collect()
    })
}

/// The `files` of the nearest layer that sets them, relative to the repository root.
fn listed(chain: &[Layer]) -> Vec<PathBuf> {
    setting(chain, |layer| layer.files.as_ref()).map_or_else(Vec::new, |(layer, list)| {
        list.iter()
            .filter_map(|file| join_normalised(Some(&layer.dir), file))
            .collect()
    })
}

/// The pattern selecting every file below `dir`.
fn everything_below(dir: &Path) -> String {
    let dir = slash(dir);
    if dir.is_empty() {
        "**/*".to_owned()
    } else {
        format!("{dir}/**/*")
    }
}

/// `path` with `/` separators, whatever the platform.
fn slash(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
