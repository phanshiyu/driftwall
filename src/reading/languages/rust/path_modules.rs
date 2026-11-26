//! The Rust modules a `#[path = ".."]` attribute places at a file of its own choosing.
//!
//! `#[path = "b/impl.rs"] mod a;` declares module `a` at `b/impl.rs`, wherever the name-based
//! layout would have put it. The module-path model of `rust_module` consults this table first.
//! See "Rust resolution" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use tree_sitter::{Node, Parser};

use super::module::Site;
use super::spec::GRAMMAR as RUST_GRAMMAR;
use super::tree::enclosing_modules;
use crate::code::Language;
use crate::reading::syntax::paths::join_normalised;

/// Files that are the root of a module directory: the directory of the file is the directory of
/// the module.
const MOD_RS: [&str; 3] = ["lib.rs", "main.rs", "mod.rs"];

/// The modules placed by `#[path]`, keyed both ways.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathModules {
    /// (crate root file, module names from the root) to the file of that module.
    files: BTreeMap<(PathBuf, Vec<String>), PathBuf>,
    /// The file of a placed module to its crate root file and module names.
    modules: BTreeMap<PathBuf, (PathBuf, Vec<String>)>,
}

impl PathModules {
    /// The file `#[path]` placed the module `names` of the crate rooted at `root` in.
    #[must_use]
    pub fn file_of(&self, root: &Path, names: &[&str]) -> Option<&PathBuf> {
        let key = (
            root.to_path_buf(),
            names.iter().map(|name| (*name).to_owned()).collect(),
        );
        self.files.get(&key)
    }

    /// The crate root file and module names of a file `#[path]` placed a module in.
    #[must_use]
    pub fn module_of(&self, file: &Path) -> Option<&(PathBuf, Vec<String>)> {
        self.modules.get(file)
    }

    fn insert(&mut self, root: &Path, names: Vec<String>, file: &Path) {
        self.modules
            .entry(file.to_path_buf())
            .or_insert_with(|| (root.to_path_buf(), names.clone()));
        self.files
            .entry((root.to_path_buf(), names))
            .or_insert_with(|| file.to_path_buf());
    }
}

/// The `#[path]` module declarations of the Rust files of `files` whose target is a known file.
/// A file that cannot be read declares nothing here: reading it is reported when its imports are
/// extracted.
#[must_use]
pub fn path_modules(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
    known: &BTreeSet<PathBuf>,
) -> PathModules {
    let found: Vec<Vec<Placed>> = files
        .iter()
        .filter(|(_, language)| **language == Language::Rust)
        .map(|(path, _)| path)
        .collect::<Vec<_>>()
        .par_iter()
        .filter_map(|&path| {
            let source = fs::read_to_string(root.join(path)).ok()?;
            let placed = declarations(&source, path, known);
            (!placed.is_empty()).then_some(placed)
        })
        .collect();
    let mut table = PathModules::default();
    for placed in found.into_iter().flatten() {
        table.insert(&placed.root, placed.names, &placed.file);
    }
    table
}

/// One placed module.
struct Placed {
    root: PathBuf,
    names: Vec<String>,
    file: PathBuf,
}

/// The `#[path] mod name;` declarations of `source`, the text of `file`.
fn declarations(source: &str, file: &Path, known: &BTreeSet<PathBuf>) -> Vec<Placed> {
    if !source.contains("path") {
        return Vec::new();
    }
    let mut parser = Parser::new();
    if parser.set_language(&(RUST_GRAMMAR.language)()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut cursor = tree.walk();
    'walk: loop {
        let node = cursor.node();
        if node.kind() == "mod_item"
            && let Some(placed) = declaration(node, source, file, known)
        {
            found.push(placed);
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
    found
}

/// The module `node` places when it is `#[path = ".."] mod name;` with a known target file.
fn declaration(
    node: Node<'_>,
    source: &str,
    file: &Path,
    known: &BTreeSet<PathBuf>,
) -> Option<Placed> {
    if node.child_by_field_name("body").is_some() {
        return None;
    }
    let written = path_attribute(node, source)?;
    let name = node
        .child_by_field_name("name")?
        .utf8_text(source.as_bytes())
        .ok()?;
    let inline = enclosing_modules(node, source);
    let (root, mut names) = Site::of(file, &inline, known, &PathModules::default())?.placement();
    names.push(name.to_owned());
    let target = join_normalised(Some(&base_dir(file, &inline)), written)?;
    known.contains(&target).then_some(Placed {
        root,
        names,
        file: target,
    })
}

/// The directory a `#[path]` written in `file`, inside the inline modules `inline`, is relative to.
fn base_dir(file: &Path, inline: &[&str]) -> PathBuf {
    let mut dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let is_mod_rs = file
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| MOD_RS.contains(&name));
    if !inline.is_empty() {
        // why: a file that is not a mod-rs file owns the directory named after its stem.
        if !is_mod_rs && let Some(stem) = file.file_stem() {
            dir.push(stem);
        }
        dir.extend(inline);
    }
    dir
}

/// The string of the `#[path = ".."]` attribute directly above `node`, if any.
fn path_attribute<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" => {
                if let Some(value) = path_value(sibling.utf8_text(source.as_bytes()).ok()?) {
                    return Some(value);
                }
            }
            "line_comment" | "block_comment" => {}
            _ => return None,
        }
        previous = sibling.prev_sibling();
    }
    None
}

/// `b/impl.rs` out of `#[path = "b/impl.rs"]`, spaces aside.
fn path_value(attribute: &str) -> Option<&str> {
    let inner = attribute.strip_prefix("#[")?.strip_suffix(']')?;
    let (name, value) = inner.split_once('=')?;
    if name.trim() != "path" {
        return None;
    }
    let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
    (!value.contains(['"', '\\'])).then_some(value)
}
