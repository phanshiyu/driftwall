//! How files are cut into nodes: directory nodes at a depth, module nodes at a fork, and the
//! automatic choice between them.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::ids::RESERVED;
use crate::model::{NodeId, Prefix};
use crate::ownership::module_name;

/// Label of the repository root directory.
const ROOT_LABEL: &str = ".";
/// Distinct modules that must sit directly in a directory before it is worth `node: *`.
const MIN_MODULES: usize = 2;

/// What counts towards the two modules a directory needs before it is worth `node: *`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Counted {
    /// Only source files directly in the directory (the automatic cut).
    Files,
    /// Source files and subdirectories directly in the directory (a unit's source directory).
    FilesAndDirectories,
}

/// The node a file ends up in, before directory ids are assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Owner {
    /// The directory node tagged at this directory (empty for the repository root).
    Directory(PathBuf),
    /// A module node, whose id the module rule fixes.
    Module(NodeId),
}

/// The decisions made so far about files that no existing tag covers.
#[derive(Debug, Default)]
pub(super) struct Draft {
    /// Directories that get a `node: <id>` tag, the id assigned once all are known.
    pub directories: BTreeSet<PathBuf>,
    /// Directories that get a `node: <prefix>*` tag, with the prefix.
    pub module_dirs: BTreeMap<PathBuf, Prefix>,
    /// Directory nodes of units that have a prefix: their ids are made of the whole path.
    pub whole_paths: BTreeSet<PathBuf>,
    /// Directory nodes that are named by something other than their path: a Swift target is named
    /// after the target. The name is sanitised when the id is assigned.
    pub names: BTreeMap<PathBuf, String>,
    /// Directory nodes that are groups (the files directly in a unit directory), which may get
    /// `scope: here`. The repository root is a group too, without being listed.
    pub groups: BTreeSet<PathBuf>,
    /// Module nodes decided so far, with their labels.
    pub modules: BTreeMap<NodeId, String>,
    /// The node of every file decided so far.
    pub owners: BTreeMap<PathBuf, Owner>,
    /// Ids that cannot be used for a new module node, or for a directory node without a suffix:
    /// the nodes existing tags name, and the modules in `modules`.
    pub fixed: BTreeSet<NodeId>,
}

impl Draft {
    /// A draft that must not reuse the `fixed` ids.
    pub fn new(fixed: BTreeSet<NodeId>) -> Self {
        Self {
            fixed,
            ..Self::default()
        }
    }

    /// Every file in `files` is owned by its ancestor directory at `depth`, or by its own
    /// directory when that is shallower.
    pub fn cut_directories(&mut self, files: &[&Path], depth: usize) {
        for file in files {
            let dir = truncate(&parent_of(file), depth);
            self.directories.insert(dir.clone());
            self.owners
                .insert((*file).to_path_buf(), Owner::Directory(dir));
        }
    }

    /// Tags `fork` with `node: <prefix>*` and gives each file of `files` (all below `fork`) its
    /// module node. Fails, changing nothing, when fewer than two distinct modules sit directly in
    /// `fork` (what is a module is up to `counted`), or a derived id is not valid, is reserved, or
    /// is already fixed.
    pub fn try_modules(
        &mut self,
        fork: &Path,
        files: &[&Path],
        prefix: &Prefix,
        counted: Counted,
    ) -> bool {
        let mut derived: BTreeMap<PathBuf, (NodeId, String)> = BTreeMap::new();
        let mut direct: BTreeSet<&NodeId> = BTreeSet::new();
        for file in files {
            let Some(name) = module_name(fork, file) else {
                return false;
            };
            let Ok(id) = prefix.derive(&name) else {
                return false;
            };
            if RESERVED.contains(&id.as_str()) || self.fixed.contains(&id) {
                return false;
            }
            derived.insert((*file).to_path_buf(), (id, name));
        }
        for (file, (id, _)) in &derived {
            if counted == Counted::FilesAndDirectories || file.parent() == Some(fork) {
                direct.insert(id);
            }
        }
        if direct.len() < MIN_MODULES {
            return false;
        }
        for (file, (id, name)) in derived {
            self.modules
                .entry(id.clone())
                .or_insert_with(|| label_of(&fork.join(name)));
            self.fixed.insert(id.clone());
            self.owners.insert(file, Owner::Module(id));
        }
        self.module_dirs.insert(fork.to_path_buf(), prefix.clone());
        true
    }

    /// The automatic cut of `files` (see `docs/DESIGN.md`, "The automatic cut"): module nodes at
    /// the fork when it directly holds two or more modules, otherwise directory nodes just below
    /// the fork. The repository root is never a module root when it also has source directories.
    pub fn cut_automatically(&mut self, files: &[&Path]) {
        let source_dirs: BTreeSet<PathBuf> = files.iter().map(|file| parent_of(file)).collect();
        let fork = automatic_fork(&source_dirs);
        let root_with_directories = fork.as_os_str().is_empty()
            && source_dirs.iter().any(|dir| !dir.as_os_str().is_empty());
        if root_with_directories
            || !self.try_modules(&fork, files, &Prefix::default(), Counted::Files)
        {
            self.cut_directories(files, component_count(&fork) + 1);
        }
    }
}

fn parent_of(file: &Path) -> PathBuf {
    file.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The first `depth` components of `dir`, or all of them when it is shallower.
fn truncate(dir: &Path, depth: usize) -> PathBuf {
    dir.components().take(depth).collect()
}

fn component_count(dir: &Path) -> usize {
    dir.components().count()
}

/// Finds the directory where the layout forks, descending from the root through pure corridors.
///
/// From the root, descend while the current directory holds no source file itself and exactly one
/// of its child directories holds sources. The stopping directory has own files or two or more
/// source-bearing branches; nodes are cut just below it, and its own files belong to it.
fn automatic_fork(source_dirs: &BTreeSet<PathBuf>) -> PathBuf {
    let mut prefix = PathBuf::new();
    loop {
        let children: BTreeSet<OsString> = source_dirs
            .iter()
            .filter_map(|dir| dir.strip_prefix(&prefix).ok()?.components().next())
            .map(Component::as_os_str)
            .map(OsString::from)
            .collect();
        let has_own_files = source_dirs.contains(&prefix);
        let mut branches = children.into_iter();
        match (has_own_files, branches.next(), branches.next()) {
            (false, Some(only), None) => prefix.push(only),
            _ => return prefix,
        }
    }
}

/// The label a node gets for `dir`: its path with `/`, or `.` for the repository root.
pub(super) fn label_of(dir: &Path) -> String {
    let parts: Vec<String> = dir
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return ROOT_LABEL.to_owned();
    }
    parts
        .join("/")
        .chars()
        .map(|c| {
            if matches!(c, '"' | '\n' | '\r') {
                '\''
            } else {
                c
            }
        })
        .collect()
}
