//! The re-export table of a TypeScript or JavaScript file: its `export .. from` statements and the
//! names it exports itself, see "Re-exports" in `docs/DESIGN.md`.

use std::path::Path;

use tree_sitter::{Node, Tree};

use super::extract::literal_text;
use super::reexport::Table;
use super::{Environment, Reference, Resolution, spec};
use crate::model::Language;

/// The table of `file`, whose parsed text is `source`.
pub fn table(
    tree: &Tree,
    source: &str,
    file: &Path,
    language: Language,
    env: &Environment<'_>,
) -> Table {
    let mut table = Table::default();
    let root = tree.root_node();
    let mut cursor = root.walk();
    for statement in root.children(&mut cursor) {
        if statement.kind() != "export_statement" {
            continue;
        }
        match statement.child_by_field_name("source") {
            Some(from) => reexport(statement, from, source, file, language, env, &mut table),
            None => local(statement, source, &mut table),
        }
    }
    table
}

/// Files the names of `export { .. } from` and `export * from` in `table`.
fn reexport(
    statement: Node<'_>,
    from: Node<'_>,
    source: &str,
    file: &Path,
    language: Language,
    env: &Environment<'_>,
    table: &mut Table,
) {
    let (Some(spec), Some(specifier)) = (spec::spec(language), literal_text(from, source)) else {
        return;
    };
    let bindings = (spec.bindings)(statement, source);
    let reference = Reference {
        specifier,
        names: &[],
        node: from,
        source,
        bindings: &bindings,
    };
    let target = (spec.resolve)(&reference, file, env)
        .into_iter()
        .find_map(|resolution| match resolution {
            Resolution::Resolved { target, .. } => Some(target),
            _ => None,
        });
    table
        .lines
        .insert(u32::try_from(statement.start_position().row + 1).unwrap_or(u32::MAX));
    let Some(target) = target.filter(|target| target != file) else {
        return;
    };
    let mut cursor = statement.walk();
    for child in statement.children(&mut cursor) {
        match child.kind() {
            "*" => table.stars.push(target.clone()),
            "export_clause" => {
                for (original, exported) in specifiers(child, source) {
                    table.names.insert(exported, (target.clone(), original));
                }
            }
            _ => {}
        }
    }
}

/// Records the names `statement`, an `export` with no source, defines.
fn local(statement: Node<'_>, source: &str, table: &mut Table) {
    if let Some(declaration) = statement.child_by_field_name("declaration") {
        declared(declaration, source, &mut table.local);
    }
    if statement.child_by_field_name("value").is_some() || has_default(statement) {
        table.local.insert("default".to_owned());
    }
    let mut cursor = statement.walk();
    for clause in statement
        .children(&mut cursor)
        .filter(|child| child.kind() == "export_clause")
    {
        table
            .local
            .extend(specifiers(clause, source).into_iter().map(|(_, name)| name));
    }
}

fn has_default(statement: Node<'_>) -> bool {
    let mut cursor = statement.walk();
    statement
        .children(&mut cursor)
        .any(|child| child.kind() == "default")
}

/// The names a declaration brings: its own name, or the identifiers a `const`/`let`/`var` binds.
fn declared(declaration: Node<'_>, source: &str, out: &mut std::collections::BTreeSet<String>) {
    if matches!(
        declaration.kind(),
        "lexical_declaration" | "variable_declaration"
    ) {
        let mut cursor = declaration.walk();
        for declarator in declaration.named_children(&mut cursor) {
            if let Some(name) = declarator
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok())
            {
                out.insert(name.to_owned());
            }
        }
    } else if let Some(name) = declaration
        .child_by_field_name("name")
        .and_then(|name| name.utf8_text(source.as_bytes()).ok())
    {
        out.insert(name.trim_matches(['"', '\'']).to_owned());
    }
}

/// The (original name, exported name) of each specifier of the `{ .. }` list `clause`.
fn specifiers(clause: Node<'_>, source: &str) -> Vec<(String, String)> {
    let text = |node: Node<'_>| node.utf8_text(source.as_bytes()).ok().map(str::to_owned);
    let mut cursor = clause.walk();
    clause
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "export_specifier")
        .filter_map(|specifier| {
            let original = text(specifier.child_by_field_name("name")?)?;
            let exported = specifier
                .child_by_field_name("alias")
                .and_then(text)
                .unwrap_or_else(|| original.clone());
            Some((original, exported))
        })
        .collect()
}
