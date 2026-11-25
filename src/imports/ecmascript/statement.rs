//! Where the statement around a TypeScript or JavaScript specifier starts, and what kind it is.

use std::path::Path;

use tree_sitter::Node;

use crate::imports::syntax::statement::{Flags, enclosing, has_test_segment, in_directory};

/// The TypeScript and JavaScript statement or call expression around a specifier: an `import` or
/// `export` statement, the `require(..)` or `import(..)` call, or the `new URL(..)` expression.
pub fn anchor(specifier: Node<'_>) -> Node<'_> {
    // why: the nearest of these wins, so a `new URL(..)` inside another call is located at its own line.
    enclosing(
        specifier,
        &[
            "import_statement",
            "export_statement",
            "call_expression",
            "new_expression",
        ],
    )
}

/// The flags of the TypeScript or JavaScript `anchor` (see [`anchor`]) written in `file`.
///
/// Type-only: a declaration file, `import type` and `export type ... from`, a statement whose
/// every specifier is written `type X`, and an `import("x")` written in a type (see
/// [`in_type_position`]). A default or namespace import beside type specifiers, an empty
/// specifier list, `require()` and an `import()` in an expression are runtime. Test code: a `*.test.*` or
/// `*.spec.*` file, or one under a `__tests__`, `test` or `tests` directory.
pub fn flags(anchor: Node<'_>, source: &str, file: &Path) -> Flags {
    Flags::new(
        is_declaration_file(file) || type_only(anchor, source) || in_type_position(anchor),
        test_file(file),
    )
}

/// Whether the `import("x")` call `call` is written in a type: below a type annotation, a `typeof`
/// type query, type arguments, a heritage clause or the value of a type alias, or as the type of an
/// `as` or `satisfies` expression. An `import()` in an expression is not, even beside a type.
pub fn in_type_position(call: Node<'_>) -> bool {
    if call.kind() != "call_expression" {
        return false;
    }
    let mut child = call;
    while let Some(parent) = child.parent() {
        match parent.kind() {
            "type_annotation"
            | "type_query"
            | "type_arguments"
            | "extends_type_clause"
            | "implements_clause" => return true,
            "type_alias_declaration" => {
                return parent.child_by_field_name("value") == Some(child);
            }
            "as_expression" | "satisfies_expression" => {
                return parent.named_child(0) != Some(child);
            }
            "member_expression" => {}
            _ => return false,
        }
        child = parent;
    }
    false
}

/// The flags of an import that loads a whole file (an HTML `<script src>`) written in `file`:
/// runtime, and test code by the same file rules as [`flags`].
pub fn load_flags(file: &Path) -> Flags {
    Flags::new(false, test_file(file))
}

/// The test flag of a `JSDoc` type import written in `file`, by the file rules of
/// [`flags`]. The import is type-only whatever the file.
pub fn doc_flags(file: &Path) -> Flags {
    Flags::new(true, test_file(file))
}

/// Whether `file` is a TypeScript or JavaScript test file: `*.test.*`, `*.spec.*`, or below a
/// `__tests__`, `test` or `tests` directory.
pub fn test_file(file: &Path) -> bool {
    has_test_segment(file, &["test", "spec"]) || in_directory(file, &["__tests__", "test", "tests"])
}

/// Whether `file` is a `.d.ts`, `.d.mts` or `.d.cts` declaration file.
fn is_declaration_file(file: &Path) -> bool {
    file.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [".d.ts", ".d.mts", ".d.cts"]
                .iter()
                .any(|end| name.ends_with(end))
        })
}

fn type_only(statement: Node<'_>, source: &str) -> bool {
    if !matches!(statement.kind(), "import_statement" | "export_statement") {
        return false;
    }
    let mut cursor = statement.walk();
    let children: Vec<Node<'_>> = statement.children(&mut cursor).collect();
    if children.iter().any(|child| is_type_keyword(*child, source)) {
        return true;
    }
    let (mut named, mut typed) = (0_usize, 0_usize);
    for child in &children {
        match child.kind() {
            "import_clause" => {
                let mut inner = child.walk();
                for part in child.named_children(&mut inner) {
                    if part.kind() != "named_imports" {
                        return false;
                    }
                    count_typed(part, &mut named, &mut typed);
                }
            }
            "export_clause" => count_typed(*child, &mut named, &mut typed),
            "namespace_export" => return false,
            _ => {}
        }
    }
    named > 0 && named == typed
}

/// Adds the specifiers of the `{ ... }` list `list`, and how many of them are written `type X`.
fn count_typed(list: Node<'_>, named: &mut usize, typed: &mut usize) {
    let mut cursor = list.walk();
    for specifier in list.named_children(&mut cursor) {
        *named += 1;
        let mut inner = specifier.walk();
        if specifier
            .children(&mut inner)
            .any(|part| part.kind() == "type")
        {
            *typed += 1;
        }
    }
}

/// Whether `node` is the `type` keyword of a statement. `export type * from` does not parse in the
/// grammar, which recovers with an error node holding the keyword, and so does `import type` in
/// JavaScript.
fn is_type_keyword(node: Node<'_>, source: &str) -> bool {
    match node.kind() {
        "type" => true,
        "ERROR" => node
            .utf8_text(source.as_bytes())
            .is_ok_and(|text| text == "type"),
        _ => false,
    }
}
