//! The symbols a TypeScript or JavaScript import statement uses, read from its syntax.

use std::collections::BTreeSet;

use tree_sitter::Node;

use super::extract::plain_string;
use crate::model::Symbol;

/// The exported name of a default import, as the module exports it.
const DEFAULT: &str = "default";

/// The symbols used by the statement around `specifier`, the string node of its module reference.
///
/// - `import x from`: `default`. `import { a as b } from` and `export { a as b } from`: `a`, the
///   exported name, never the local one. `import x, { y } from` records both.
/// - `const { a, b: c } = require('x')` and `const { a } = await import('x')`: `a` and `b`, the
///   exported names. A rest element (`...rest`) or a computed key beside them is [`Symbol::All`],
///   since the names it takes are not written.
/// - `import * as ns from`, `export * from`, `export * as ns from`, `import x = require()`, any
///   other `require()` and dynamic `import()`: [`Symbol::All`].
/// - `import 'a'` has no import clause and uses nothing.
///
/// `import type { T }` and `import { type T }` record `T` like any other named import.
pub fn used(specifier: Node<'_>, source: &str) -> BTreeSet<Symbol> {
    let mut symbols = BTreeSet::new();
    let Some(statement) = specifier.parent() else {
        return symbols;
    };
    match statement.kind() {
        "import_statement" => {
            let mut cursor = statement.walk();
            for clause in statement
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "import_clause")
            {
                clause_symbols(clause, source, &mut symbols);
            }
        }
        "export_statement" => export_symbols(statement, source, &mut symbols),
        // why: `import x = require('a')`, `require('a')` and `import('a')` hand over the module.
        _ => {
            let destructured = statement
                .parent()
                .and_then(|call| destructured_names(call, source));
            match destructured {
                Some(names) => symbols = names,
                None => {
                    symbols.insert(Symbol::All);
                }
            }
        }
    }
    symbols
}

/// The names a `const { a, b: c } = <call>` takes from the module the `call` (`require(..)` or
/// `import(..)`, possibly awaited) returns. `None` when the call is not the value of a declarator
/// whose name is an object pattern.
fn destructured_names(call: Node<'_>, source: &str) -> Option<BTreeSet<Symbol>> {
    let mut value = call;
    let mut parent = value.parent()?;
    while matches!(
        parent.kind(),
        "await_expression" | "parenthesized_expression" | "as_expression" | "non_null_expression"
    ) {
        value = parent;
        parent = value.parent()?;
    }
    if parent.kind() != "variable_declarator" || parent.child_by_field_name("value") != Some(value)
    {
        return None;
    }
    let pattern = parent.child_by_field_name("name")?;
    if pattern.kind() != "object_pattern" {
        return None;
    }
    let mut names = BTreeSet::new();
    let mut cursor = pattern.walk();
    for member in pattern.named_children(&mut cursor) {
        let name = match member.kind() {
            "shorthand_property_identifier_pattern" => {
                member.utf8_text(source.as_bytes()).ok().map(str::to_owned)
            }
            "object_assignment_pattern" => member
                .child_by_field_name("left")
                .and_then(|left| left.utf8_text(source.as_bytes()).ok())
                .map(str::to_owned),
            "pair_pattern" => member
                .child_by_field_name("key")
                .and_then(|key| match key.kind() {
                    "property_identifier" => key.utf8_text(source.as_bytes()).ok(),
                    "string" => plain_string(key, source),
                    _ => None,
                })
                .map(str::to_owned),
            _ => None,
        };
        // why: a rest element or a computed key takes names that are not written, so it is `*`.
        names.insert(name.map_or(Symbol::All, Symbol::Name));
    }
    Some(names)
}

fn clause_symbols(clause: Node<'_>, source: &str, out: &mut BTreeSet<Symbol>) {
    let mut cursor = clause.walk();
    for child in clause.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                out.insert(Symbol::Name(DEFAULT.to_owned()));
            }
            "namespace_import" => {
                out.insert(Symbol::All);
            }
            "named_imports" => specifier_names(child, "import_specifier", source, out),
            _ => {}
        }
    }
}

fn export_symbols(statement: Node<'_>, source: &str, out: &mut BTreeSet<Symbol>) {
    let mut cursor = statement.walk();
    for child in statement.children(&mut cursor) {
        match child.kind() {
            "*" | "namespace_export" => {
                out.insert(Symbol::All);
            }
            "export_clause" => specifier_names(child, "export_specifier", source, out),
            _ => {}
        }
    }
}

/// Adds the exported name of every `kind` child of the `{ ... }` list `list`.
fn specifier_names(list: Node<'_>, kind: &str, source: &str, out: &mut BTreeSet<Symbol>) {
    let mut cursor = list.walk();
    for specifier in list
        .named_children(&mut cursor)
        .filter(|child| child.kind() == kind)
    {
        let Some(name) = specifier.child_by_field_name("name") else {
            continue;
        };
        let text = if name.kind() == "string" {
            plain_string(name, source)
        } else {
            name.utf8_text(source.as_bytes()).ok()
        };
        out.extend(text.map(|text| Symbol::Name(text.to_owned())));
    }
}
