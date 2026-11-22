//! Rule 2 of `init`: what the code units decide, before the automatic cut sees the rest.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use super::ids::sanitize;
use super::layout::{Counted, Draft, Owner};
use super::units::{CRATE_SOURCE, Unit, UnitKind};
use crate::model::Prefix;

/// Lets the units of the repository decide the files in `fresh` they cover, and returns the files
/// nobody decided yet.
///
/// Every unit is decomposed (see `docs/DESIGN.md`, "Code units"): the source directory of a crate,
/// a Python package or the primary unit gets `node: <prefix>*` when it directly holds two or more
/// modules, and the source directory of any other JavaScript unit is split by subdirectory. The
/// rest of the unit becomes directory nodes. A unit that cannot be decomposed is one directory
/// node, except the primary unit, whose files are left to the automatic cut. Units without a file
/// in `fresh` do not count.
pub(super) fn claim<'a>(draft: &mut Draft, fresh: &[&'a Path], units: &[Unit]) -> Vec<&'a Path> {
    let mut kinds: BTreeMap<&Path, UnitKind> = BTreeMap::new();
    for unit in units {
        let kind = kinds.entry(unit.dir.as_path()).or_insert(unit.kind);
        *kind = (*kind).min(unit.kind);
    }
    let swift_names: BTreeMap<&Path, &str> = units
        .iter()
        .filter(|unit| unit.kind == UnitKind::SwiftTarget)
        .filter_map(|unit| Some((unit.dir.as_path(), unit.name.as_deref()?)))
        .rev()
        .collect();
    let mut files_of: BTreeMap<&Path, Vec<&'a Path>> = BTreeMap::new();
    for file in fresh {
        let deepest = kinds
            .keys()
            .filter(|dir| file.starts_with(dir))
            .max_by_key(|dir| dir.components().count());
        if let Some(dir) = deepest {
            files_of.entry(dir).or_default().push(file);
        }
    }
    let primary = primary_unit(&files_of, &kinds);
    let has_source_directories = fresh
        .iter()
        .any(|file| file.parent().is_some_and(|dir| !dir.as_os_str().is_empty()));

    let mut taken: BTreeSet<&Path> = BTreeSet::new();
    for (dir, files) in &files_of {
        let kind = kinds.get(dir).copied().unwrap_or(UnitKind::Package);
        if kind == UnitKind::SwiftTarget {
            claim_swift_target(draft, dir, swift_names.get(dir).copied(), files);
            taken.extend(files.iter().copied());
            continue;
        }
        let is_primary = primary == Some(*dir);
        let prefix = if is_primary {
            Some(Prefix::default())
        } else {
            prefix_of(dir)
        };
        let source = source_dir(dir, kind, files);
        if kind == UnitKind::Package
            && !is_primary
            && let Some(prefix) = &prefix
        {
            claim_layers(draft, dir, &source, prefix, files);
            taken.extend(files.iter().copied());
            continue;
        }
        if kind == UnitKind::Package
            && is_primary
            && source.as_os_str().is_empty()
            && has_source_directories
        {
            // why: root files are tooling next to the code, never `node: *` ("Root files").
            draft.cut_directories(files, 1);
            taken.extend(files.iter().copied());
            continue;
        }
        let inside: Vec<&Path> = files
            .iter()
            .copied()
            .filter(|file| file.starts_with(&source))
            .collect();
        let decomposed = prefix.as_ref().is_some_and(|prefix| {
            draft.try_modules(&source, &inside, prefix, Counted::FilesAndDirectories)
        });
        if decomposed {
            taken.extend(inside);
            for file in files.iter().filter(|file| !file.starts_with(&source)) {
                let owner = rest_directory(draft, dir, file);
                draft.directories.insert(owner.clone());
                if !is_primary {
                    draft.whole_paths.insert(owner.clone());
                }
                draft
                    .owners
                    .insert((*file).to_path_buf(), Owner::Directory(owner));
                taken.insert(file);
            }
        } else if !is_primary {
            draft.directories.insert(dir.to_path_buf());
            draft.whole_paths.insert(dir.to_path_buf());
            for file in files {
                draft
                    .owners
                    .insert((*file).to_path_buf(), Owner::Directory(dir.to_path_buf()));
                taken.insert(file);
            }
        }
    }
    fresh
        .iter()
        .copied()
        .filter(|file| !taken.contains(file))
        .collect()
}

/// A JavaScript unit that is not the primary one is cut by directory: the files directly in its
/// source directory are one group, each subdirectory of the source directory is one node named
/// `<prefix><subdirectory>`, and the rest of the unit is directory nodes of the whole path.
fn claim_layers(draft: &mut Draft, dir: &Path, source: &Path, prefix: &Prefix, files: &[&Path]) {
    for file in files {
        let owner = match file.strip_prefix(source) {
            Ok(below) if below.components().count() == 1 => {
                draft.groups.insert(source.to_path_buf());
                source.to_path_buf()
            }
            Ok(below) => {
                let first = below.components().next().map(Component::as_os_str);
                let layer = source.join(first.unwrap_or_default());
                let name = first.unwrap_or_default().to_string_lossy();
                draft
                    .names
                    .insert(layer.clone(), format!("{}{name}", prefix.as_str()));
                layer
            }
            Err(_) => rest_directory(draft, dir, file),
        };
        draft.directories.insert(owner.clone());
        draft.whole_paths.insert(owner.clone());
        draft
            .owners
            .insert((*file).to_path_buf(), Owner::Directory(owner));
    }
}

/// A Swift target is one directory node named after the target, whatever it holds: its files
/// never import each other, so cutting it into modules would only scatter the module.
fn claim_swift_target(draft: &mut Draft, dir: &Path, name: Option<&str>, files: &[&Path]) {
    draft.directories.insert(dir.to_path_buf());
    draft.whole_paths.insert(dir.to_path_buf());
    if let Some(name) = name {
        draft.names.insert(dir.to_path_buf(), name.to_owned());
    }
    for file in files {
        draft
            .owners
            .insert((*file).to_path_buf(), Owner::Directory(dir.to_path_buf()));
    }
}

/// The unit at the repository root, or else the only unit, not counting Swift targets.
fn primary_unit<'a>(
    files_of: &BTreeMap<&'a Path, Vec<&Path>>,
    kinds: &BTreeMap<&Path, UnitKind>,
) -> Option<&'a Path> {
    let counted: Vec<&'a Path> = files_of
        .keys()
        .copied()
        .filter(|dir| kinds.get(dir) != Some(&UnitKind::SwiftTarget))
        .collect();
    let root = counted
        .iter()
        .copied()
        .find(|dir| dir.as_os_str().is_empty());
    match (root, counted.len()) {
        (Some(root), _) => Some(root),
        (None, 1) => counted.first().copied(),
        (None, _) => None,
    }
}

/// The prefix of a unit that is not primary: its directory path as an id, then `-`.
fn prefix_of(dir: &Path) -> Option<Prefix> {
    let joined = dir
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("-");
    Prefix::new(&format!("{}-", sanitize(&joined))).ok()
}

/// The directory whose modules are the unit's nodes: a crate's `src/`, a JavaScript unit's `src/`
/// when `files` has a file there and else the unit directory, a Python package's own directory.
fn source_dir(dir: &Path, kind: UnitKind, files: &[&Path]) -> PathBuf {
    let source = dir.join(CRATE_SOURCE);
    match kind {
        UnitKind::Crate => source,
        UnitKind::Package if files.iter().any(|file| file.starts_with(&source)) => source,
        UnitKind::Package | UnitKind::PythonPackage | UnitKind::SwiftTarget => dir.to_path_buf(),
    }
}

/// The directory node of a file of the unit at `dir` that lies outside its source directory: its
/// first subdirectory below `dir`, or `dir` itself (a group) for a file directly in it.
fn rest_directory(draft: &mut Draft, dir: &Path, file: &Path) -> PathBuf {
    if file.parent() == Some(dir) {
        draft.groups.insert(dir.to_path_buf());
        return dir.to_path_buf();
    }
    file.strip_prefix(dir)
        .ok()
        .and_then(|below| below.components().next())
        .map_or_else(|| dir.to_path_buf(), |first| dir.join(first))
}
