//! The Rust files that are test code because a test module declaration pulls them in.
//!
//! `#[cfg(test)] mod tests;` makes the file `tests.rs` (or `tests/mod.rs`) test code, and so is
//! every file that file declares in turn. The file is found through the module-path model of
//! `module`, relative to the declaring file, or where `#[path]` placed it (`path_modules`). See "Import kind" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use tree_sitter::{Node, Parser};

use super::module::Site;
use super::path_modules::PathModules;
use super::spec::GRAMMAR as RUST_GRAMMAR;
use super::statement::{in_cfg_test, test_file as rust_test_file};
use super::tree::enclosing_modules;
use crate::code::Language;

/// One `mod name;` declaration: the file it pulls in and whether it is written in test code.
struct Declaration {
    file: PathBuf,
    in_test_code: bool,
}

/// The Rust files of `files` that a test module declaration pulls in, directly or through the
/// modules of such a file. `known` is every repository file. A file that cannot be read declares
/// nothing here: reading it is reported when its imports are extracted.
#[must_use]
pub fn test_files(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
    known: &BTreeSet<PathBuf>,
    paths: &PathModules,
) -> BTreeSet<PathBuf> {
    let rust: Vec<&PathBuf> = files
        .iter()
        .filter(|(_, language)| **language == Language::Rust)
        .map(|(path, _)| path)
        .collect();
    let declared: BTreeMap<&PathBuf, Vec<Declaration>> = rust
        .par_iter()
        .filter_map(|&path| {
            let source = fs::read_to_string(root.join(path)).ok()?;
            let found = declarations(&source, path, known, paths);
            (!found.is_empty()).then_some((path, found))
        })
        .collect();
    let mut tests: BTreeSet<PathBuf> = rust
        .iter()
        .filter(|path| rust_test_file(path))
        .map(|&path| path.clone())
        .collect();
    for found in declared.values() {
        tests.extend(
            found
                .iter()
                .filter(|declaration| declaration.in_test_code)
                .map(|declaration| declaration.file.clone()),
        );
    }
    // why: a module of a test module is test code, so follow declarations until nothing is added.
    let mut pending: Vec<PathBuf> = tests.iter().cloned().collect();
    while let Some(file) = pending.pop() {
        for declaration in declared.get(&file).into_iter().flatten() {
            if tests.insert(declaration.file.clone()) {
                pending.push(declaration.file.clone());
            }
        }
    }
    tests
        .into_iter()
        .filter(|path| files.get(path) == Some(&Language::Rust))
        .collect()
}

/// The `mod name;` declarations of `source`, the text of `file`, whose module file is known.
fn declarations(
    source: &str,
    file: &Path,
    known: &BTreeSet<PathBuf>,
    paths: &PathModules,
) -> Vec<Declaration> {
    if !source.contains("mod ") {
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
            && let Some(declaration) = declaration(node, source, file, known, paths)
        {
            found.push(declaration);
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

/// The declaration `node` makes when it is `mod name;` with no body and the module file is known.
fn declaration(
    node: Node<'_>,
    source: &str,
    file: &Path,
    known: &BTreeSet<PathBuf>,
    paths: &PathModules,
) -> Option<Declaration> {
    if node.child_by_field_name("body").is_some() {
        return None;
    }
    let name = node
        .child_by_field_name("name")?
        .utf8_text(source.as_bytes())
        .ok()?;
    let inline = enclosing_modules(node, source);
    let site = Site::of(file, &inline, known, paths)?;
    Some(Declaration {
        file: site.declared_file(name, known, paths)?,
        in_test_code: in_cfg_test(node, source),
    })
}
