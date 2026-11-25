//! How files are cut into nodes: directory nodes at a depth, module nodes at a fork, and the
//! automatic choice between them.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::ids::{RESERVED, sanitize};
use crate::contract::{NodeId, Prefix};
use crate::ownership::{folds, is_test_file, module_name, peers_in};

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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Owner {
    /// The directory node tagged at this directory (empty for the repository root).
    Directory(PathBuf),
    /// A module node, whose id the module rule fixes.
    Module(NodeId),
}

/// The modules of a fork: each file with its id and module name, and the peers directly in the fork.
type Derived = (BTreeMap<PathBuf, (NodeId, String)>, BTreeSet<String>);

/// A directory that gets a `node: <prefix>*` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModuleDir {
    /// The prefix of every id the tag derives.
    pub prefix: Prefix,
    /// The non-test modules directly in the directory, as the scan will see them.
    pub peers: BTreeSet<String>,
}

/// The decisions made so far about files that no existing tag covers.
#[derive(Debug, Default)]
pub(super) struct Draft {
    /// Directories that get a `node: <id>` tag, the id assigned once all are known.
    pub directories: BTreeSet<PathBuf>,
    /// Directories that get a `node: <prefix>*` tag.
    pub module_dirs: BTreeMap<PathBuf, ModuleDir>,
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
    /// The ids of the units that have a prefix (`engine-core` for the prefix `engine-core-`): the
    /// id families of directories other than the one a plain `node: *` tags.
    pub families: BTreeSet<String>,
    /// Every source file of the repository, owned or not: the modules a `node: *` directory has
    /// are the ones the scan will see there, whoever owns them.
    pub all_files: BTreeSet<PathBuf>,
    /// The entry files among `all_files` that declare nothing: under `node: *` they make no module
    /// of their own unless another module has their name (see "Folded entry files").
    pub trivial: BTreeSet<PathBuf>,
}

impl Draft {
    /// A draft that must not reuse the `fixed` ids, for a repository with the source files
    /// `all_files`.
    pub fn new(fixed: BTreeSet<NodeId>, all_files: BTreeSet<PathBuf>) -> Self {
        Self {
            fixed,
            all_files,
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
    ///
    /// A tag with no prefix whose ids collide with the id family of another directory (see
    /// [`Draft::families`]) is written with the prefix `<parent>-` instead, `src-engine` for the
    /// module `engine` of `src/`.
    pub fn try_modules(
        &mut self,
        fork: &Path,
        files: &[&Path],
        prefix: &Prefix,
        counted: Counted,
    ) -> bool {
        let mut prefix = prefix.clone();
        let mut found = self.derive(fork, files, &prefix);
        if prefix.as_str().is_empty()
            && let Some((derived, _)) = &found
            && derived.values().any(|(id, _)| self.collides(id))
            && let Some(parent) = parent_prefix(fork)
        {
            prefix = parent;
            found = self.derive(fork, files, &prefix);
        }
        let Some((derived, peers)) = found else {
            return false;
        };
        let mut direct: BTreeSet<&NodeId> = BTreeSet::new();
        // why: a test file never makes a module of its own, so it does not count towards two.
        for (file, (id, _)) in &derived {
            let is_direct = file.parent() == Some(fork);
            if is_direct && is_test_file(file) {
                continue;
            }
            if counted == Counted::FilesAndDirectories || is_direct {
                direct.insert(id);
            }
        }
        if direct.len() < MIN_MODULES {
            return false;
        }
        for (file, (id, name)) in derived {
            self.modules
                .entry(id.clone())
                .or_insert_with(|| module_label(fork, &name, &peers));
            self.fixed.insert(id.clone());
            self.owners.insert(file, Owner::Module(id));
        }
        self.module_dirs
            .insert(fork.to_path_buf(), ModuleDir { prefix, peers });
        true
    }

    /// The module of every file of `files` under `prefix`, with the non-test modules directly in
    /// `fork`; `None` when a name or id is not valid, or an id is reserved or already fixed.
    fn derive(&self, fork: &Path, files: &[&Path], prefix: &Prefix) -> Option<Derived> {
        let mut derived: BTreeMap<PathBuf, (NodeId, String)> = BTreeMap::new();
        let peers = peers_in(
            fork,
            self.all_files
                .iter()
                .filter(|file| !self.trivial.contains(*file))
                .map(PathBuf::as_path),
        );
        for file in files {
            // why: a folded entry file declares no node, so the module decision never sees it.
            if self.trivial.contains(*file) && folds(fork, file, &peers) {
                continue;
            }
            let name = module_name(fork, file, &peers)?;
            let id = prefix.derive(&name).ok()?;
            if RESERVED.contains(&id.as_str()) || self.fixed.contains(&id) {
                return None;
            }
            derived.insert((*file).to_path_buf(), (id, name));
        }
        Some((derived, peers))
    }

    /// Whether the unprefixed module id `id` equals or is a dash-prefix of the id family of another
    /// directory: a unit id (`engine-core`) or a fixed id, so `engine` against `engine-core-anim`.
    fn collides(&self, id: &NodeId) -> bool {
        let lead = format!("{id}-");
        self.families
            .iter()
            .any(|family| family == id.as_str() || family.starts_with(&lead))
            || self
                .fixed
                .iter()
                .any(|fixed| fixed.as_str().starts_with(&lead))
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

/// The label of the module `name` of the `node: *` directory `dir`, whose non-test modules are
/// `peers`: `dir/name` for a module a file or directory of that name makes, and `dir` itself for
/// the module that only test files there make (see "Module nodes" in `docs/DESIGN.md`), since
/// no path `dir/name` exists for it.
pub(super) fn module_label(dir: &Path, name: &str, peers: &BTreeSet<String>) -> String {
    if peers.contains(name) {
        label_of(&dir.join(name))
    } else {
        label_of(dir)
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

/// The prefix `<parent>-` for the modules of `fork`, named after its own directory, or `None` for
/// the repository root, which has no name.
fn parent_prefix(fork: &Path) -> Option<Prefix> {
    let name = fork.file_name()?.to_string_lossy();
    Prefix::new(&format!("{}-", sanitize(&name))).ok()
}
