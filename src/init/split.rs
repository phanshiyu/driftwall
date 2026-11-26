//! The layer split: a large directory node whose subdirectories are layers of their own becomes one
//! node per layer. See `docs/DESIGN.md`, "The layer split".

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use super::layout::{Counted, Draft, Owner};
use crate::contract::{NodeId, Prefix};
use crate::ownership::{folds, peers_in};

/// Source files a node needs before it is split.
pub(super) const MIN_NODE_FILES: usize = 35;
/// Source files a child directory needs to become a node of its own.
pub(super) const MIN_CHILD_FILES: usize = 8;
/// Most layers a split may make: more child directories than this are a flat collection of
/// features, and the node stays whole.
pub(super) const MAX_LAYERS: usize = 4;

impl Draft {
    /// Splits every large directory node (and every large module node that stands for one
    /// directory) one level: each child directory with enough files of the node becomes
    /// `<id>-<child>`, and the rest stays in the node, now a directory node at the directory.
    pub fn split_layers(&mut self) {
        let mut by_owner: BTreeMap<Owner, Vec<PathBuf>> = BTreeMap::new();
        for (file, owner) in &self.owners {
            by_owner
                .entry(owner.clone())
                .or_default()
                .push(file.clone());
        }
        for (owner, files) in by_owner {
            if files.len() < MIN_NODE_FILES {
                continue;
            }
            let Some(dir) = self.splittable_dir(&owner, &files) else {
                continue;
            };
            let mut counts: BTreeMap<OsStringKey, usize> = BTreeMap::new();
            for file in &files {
                if let Some(child) = child_of(&dir, file) {
                    *counts.entry(child).or_default() += 1;
                }
            }
            let layers: BTreeSet<OsStringKey> = counts
                .into_iter()
                .filter(|(_, count)| *count >= MIN_CHILD_FILES)
                .map(|(child, _)| child)
                .collect();
            if layers.is_empty() || layers.len() > MAX_LAYERS {
                continue;
            }
            self.apply_split(&owner, &dir, &files, &layers);
        }
    }

    /// The directory a node stands for, when it can be split: its own directory node, or a module
    /// node whose files all lie below one subdirectory of a `node: *` directory.
    fn splittable_dir(&self, owner: &Owner, files: &[PathBuf]) -> Option<PathBuf> {
        match owner {
            Owner::Directory(dir) => (!self.module_dirs.contains_key(dir)).then(|| dir.clone()),
            Owner::Module(_) => {
                let dir = common_dir(files)?;
                let parent = dir.parent()?;
                (self.module_dirs.contains_key(parent) && !self.module_dirs.contains_key(&dir))
                    .then_some(dir)
            }
        }
    }

    fn apply_split(
        &mut self,
        owner: &Owner,
        dir: &Path,
        files: &[PathBuf],
        layers: &BTreeSet<OsStringKey>,
    ) {
        let base_name: Option<String> = match owner {
            Owner::Module(id) => Some(id.to_string()),
            Owner::Directory(found) => self.names.get(found).cloned(),
        };
        if let Owner::Module(id) = owner
            && self.split_into_modules(id, dir, files, layers)
        {
            return;
        }
        // why: a directory node left owning only folded glue must not exist, so the glue folds instead.
        let folded_glue: BTreeSet<&PathBuf> = match owner {
            Owner::Directory(_) => self
                .folded_glue(dir, files, layers)
                .map(|(glue, _)| glue)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            Owner::Module(_) => BTreeSet::new(),
        };
        let whole = self.whole_paths.contains(dir) || matches!(owner, Owner::Module(_));
        if let Owner::Module(id) = owner {
            // why: the module id moves to the directory node of the loose files, so it is no longer a derived module.
            self.modules.remove(id);
            self.fixed.remove(id);
            self.directories.insert(dir.to_path_buf());
            self.whole_paths.insert(dir.to_path_buf());
            if let Some(name) = &base_name {
                self.names.insert(dir.to_path_buf(), name.clone());
            }
        }
        for file in files {
            if folded_glue.contains(file) {
                self.owners.remove(file);
                continue;
            }
            let target = child_of(dir, file)
                .filter(|child| layers.contains(child))
                .map_or_else(|| dir.to_path_buf(), |child| dir.join(child.0.as_os_str()));
            if target != dir {
                self.directories.insert(target.clone());
                if whole {
                    self.whole_paths.insert(target.clone());
                }
                let name = target
                    .file_name()
                    .map(|part| part.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if let Some(base) = &base_name {
                    self.names.insert(target.clone(), format!("{base}-{name}"));
                }
            }
            self.owners.insert(file.clone(), Owner::Directory(target));
        }
        if !folded_glue.is_empty() {
            self.directories.remove(dir);
            self.groups.remove(dir);
        }
    }
}

impl Draft {
    /// Splits the module `id` of `dir` into the modules `<id>-<layer>` of a `node: <id>-*` tag at
    /// `dir`, when every file is in a layer or is glue that declares nothing; the glue then folds
    /// like `src/lib.rs` instead of keeping a node of its own. Changes nothing and returns false
    /// otherwise.
    fn split_into_modules(
        &mut self,
        id: &NodeId,
        dir: &Path,
        files: &[PathBuf],
        layers: &BTreeSet<OsStringKey>,
    ) -> bool {
        let Some((glue, layered)) = self.folded_glue(dir, files, layers) else {
            return false;
        };
        let Ok(prefix) = Prefix::new(&format!("{id}-")) else {
            return false;
        };
        let saved = self.modules.remove(id);
        let was_fixed = self.fixed.remove(id);
        let below: Vec<&Path> = layered.iter().map(|file| file.as_path()).collect();
        if self.try_modules(dir, &below, &prefix, Counted::FilesAndDirectories) {
            for file in glue {
                self.owners.remove(file);
            }
            return true;
        }
        // why: a refused split must leave the module as it was.
        if let Some(label) = saved {
            self.modules.insert(id.clone(), label);
        }
        if was_fixed {
            self.fixed.insert(id.clone());
        }
        false
    }

    /// The loose files of `files` (directly in `dir`) and the files in a layer, when every file is
    /// one or the other and every loose file is an entry file that declares nothing and folds;
    /// `None` when there is no loose file or any file is neither.
    fn folded_glue<'a>(
        &self,
        dir: &Path,
        files: &'a [PathBuf],
        layers: &BTreeSet<OsStringKey>,
    ) -> Option<(Vec<&'a PathBuf>, Vec<&'a PathBuf>)> {
        let peers = peers_in(
            dir,
            self.all_files
                .iter()
                .filter(|file| !self.trivial.contains(*file))
                .map(PathBuf::as_path),
        );
        let (glue, layered): (Vec<&PathBuf>, Vec<&PathBuf>) =
            files.iter().partition(|file| child_of(dir, file).is_none());
        let folded = |file: &&PathBuf| self.trivial.contains(*file) && folds(dir, file, &peers);
        let all_layered = layered
            .iter()
            .all(|file| child_of(dir, file).is_some_and(|child| layers.contains(&child)));
        (!glue.is_empty() && glue.iter().all(folded) && all_layered).then_some((glue, layered))
    }
}

/// A child directory name, ordered.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OsStringKey(std::ffi::OsString);

/// The first directory below `dir` that holds `file`, or `None` for a file directly in `dir`.
fn child_of(dir: &Path, file: &Path) -> Option<OsStringKey> {
    let below = file.strip_prefix(dir).ok()?;
    let mut parts = below.components();
    let first = parts.next()?;
    parts.next()?;
    match first {
        Component::Normal(name) => Some(OsStringKey(name.to_owned())),
        _ => None,
    }
}

/// The deepest directory that holds all of `files`.
fn common_dir(files: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = files.iter().filter_map(|file| file.parent());
    let first = iter.next()?;
    let mut common: Vec<Component> = first.components().collect();
    for dir in iter {
        let shared = common
            .iter()
            .zip(dir.components())
            .take_while(|(a, b)| *a == b)
            .count();
        common.truncate(shared);
    }
    Some(common.into_iter().collect())
}
