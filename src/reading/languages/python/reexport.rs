//! The re-export table of a Python file: the `from .x import Y` statements of an `__init__.py` and
//! the names the file defines, see "Re-exports" in `docs/DESIGN.md`.

use std::collections::BTreeSet;
use std::path::Path;

use tree_sitter::{Node, Tree};

use super::layout::PythonLayout;
use super::resolve::resolve;
use crate::code::Symbol;
use crate::reading::syntax::resolution::{Files, Reference, Resolution};
use crate::reading::syntax::table::Table;

/// The table of `file`, whose parsed text is `source`.
pub fn table(
    tree: &Tree,
    source: &str,
    file: &Path,
    files: &Files<'_>,
    layout: &PythonLayout,
) -> Table {
    let mut table = Table {
        public_only: true,
        ..Table::default()
    };
    let facade = file.file_name().is_some_and(|name| name == "__init__.py");
    visit(
        tree.root_node(),
        source,
        file,
        facade,
        (files, layout),
        &mut table,
    );
    table
}

/// Reads `node` and, outside function and class bodies, everything below it.
fn visit(
    node: Node<'_>,
    source: &str,
    file: &Path,
    facade: bool,
    env: (&Files<'_>, &PythonLayout),
    table: &mut Table,
) {
    match node.kind() {
        "function_definition" | "class_definition" => {
            let name = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok());
            table.local.extend(name.map(str::to_owned));
        }
        "import_from_statement" if facade => reexport(node, source, file, env, table),
        "expression_statement" => assignment(node, source, table),
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                visit(child, source, file, facade, env, table);
            }
        }
    }
}

/// Records `name = ..` as a local name, and `__all__ = [..]` as the names a glob exports.
fn assignment(statement: Node<'_>, source: &str, table: &mut Table) {
    let Some(assign) = statement
        .named_child(0)
        .filter(|n| n.kind() == "assignment")
    else {
        return;
    };
    let Some(left) = assign
        .child_by_field_name("left")
        .filter(|left| left.kind() == "identifier")
        .and_then(|left| left.utf8_text(source.as_bytes()).ok())
    else {
        return;
    };
    if left == "__all__" {
        table.all = assign
            .child_by_field_name("right")
            .and_then(|right| string_list(right, source));
    }
    table.local.insert(left.to_owned());
}

/// The strings of a list or tuple of plain string literals, `None` for anything else.
fn string_list(node: Node<'_>, source: &str) -> Option<BTreeSet<String>> {
    if !matches!(node.kind(), "list" | "tuple") {
        return None;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .map(|item| crate::reading::syntax::literal::literal_text(item, source).map(str::to_owned))
        .collect()
}

/// Files the names of one `from m import ..` statement of an `__init__.py`.
fn reexport(
    statement: Node<'_>,
    source: &str,
    file: &Path,
    (files, layout): (&Files<'_>, &PythonLayout),
    table: &mut Table,
) {
    let Some(module) = statement.child_by_field_name("module_name") else {
        return;
    };
    table
        .lines
        .insert(u32::try_from(statement.start_position().row + 1).unwrap_or(u32::MAX));
    let text = |node: Node<'_>| node.utf8_text(source.as_bytes()).ok();
    let mut written: Vec<(&str, &str)> = Vec::new();
    let mut star = false;
    let mut cursor = statement.walk();
    for child in statement.children_by_field_name("name", &mut cursor) {
        match child.kind() {
            "dotted_name" => written.extend(text(child).map(|name| (name, name))),
            "aliased_import" => {
                let original = child.child_by_field_name("name").and_then(text);
                let alias = child.child_by_field_name("alias").and_then(text);
                written.extend(original.map(|original| (original, alias.unwrap_or(original))));
            }
            _ => {}
        }
    }
    let mut other = statement.walk();
    star |= statement
        .children(&mut other)
        .any(|child| child.kind() == "wildcard_import");
    let names: Vec<&str> = written.iter().map(|(original, _)| *original).collect();
    let reference = Reference {
        specifier: text(module).unwrap_or_default(),
        names: &names,
        node: module,
        source,
    };
    let resolutions = resolve(&reference, file, files, layout);
    if star {
        table
            .stars
            .extend(resolutions.into_iter().filter_map(|r| match r {
                Resolution::Resolved { target, symbols } if symbols.contains(&Symbol::All) => {
                    Some(target)
                }
                _ => None,
            }));
        return;
    }
    for ((original, exported), resolution) in written.iter().zip(resolutions) {
        // why: a name that is a submodule resolves with no symbol and is a module binding.
        if let Resolution::Resolved { target, symbols } = resolution
            && symbols.contains(&Symbol::Name((*original).to_owned()))
        {
            table
                .names
                .insert((*exported).to_owned(), (target, (*original).to_owned()));
        }
    }
}
