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
/// - `import * as ns from`, `export * from`, `export * as ns from`, `import x = require()`,
///   `require()` and dynamic `import()`: [`Symbol::All`].
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
            symbols.insert(Symbol::All);
        }
    }
    symbols
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
