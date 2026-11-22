//! Where the statement around a captured specifier starts, and what kind of statement it is.
//!
//! The query captures the module reference, which is often not on the first line of its statement
//! (`import {\n a,\n} from './a'`). Each language names the statement node, and its start line is
//! the location of the import. The same node decides whether the import is type-only and whether
//! it is written in test code.

use std::path::Path;

use tree_sitter::Node;

use crate::model::ImportKind;

/// The nearest ancestor of `node`, or `node` itself, whose kind is one of `kinds`. `node` when
/// there is none, so a location is always found.
fn enclosing<'a>(node: Node<'a>, kinds: &[&str]) -> Node<'a> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if kinds.contains(&candidate.kind()) {
            return candidate;
        }
        current = candidate.parent();
    }
    node
}

/// The TypeScript and JavaScript statement or call expression around a specifier: an `import` or
/// `export` statement, or the `require(..)` or `import(..)` call.
pub fn ecmascript_anchor(specifier: Node<'_>) -> Node<'_> {
    enclosing(
        specifier,
        &["import_statement", "export_statement", "call_expression"],
    )
}

/// The Python `import` or `from` statement around a specifier, or the dynamic import call.
pub fn python_anchor(specifier: Node<'_>) -> Node<'_> {
    enclosing(
        specifier,
        &["import_statement", "import_from_statement", "call"],
    )
}

/// The Rust `use` declaration around a specifier, or the `include_str!` invocation around a string.
/// A path in code is its own anchor.
pub fn rust_anchor(specifier: Node<'_>) -> Node<'_> {
    let parent = specifier.parent();
    match parent {
        Some(parent) if parent.kind() == "use_declaration" => parent,
        Some(parent) if specifier.kind() == "string_literal" => {
            parent.parent().unwrap_or(specifier)
        }
        _ => specifier,
    }
}

/// What a statement is for and where it is written, as far as its syntax and its file name say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    /// [`ImportKind::Runtime`] or [`ImportKind::TypeOnly`]. A resolver may still decide on
    /// [`ImportKind::Embed`].
    pub kind: ImportKind,
    /// Whether the statement is written in test code.
    pub test_only: bool,
}

impl Flags {
    const fn new(type_only: bool, test_only: bool) -> Self {
        Self {
            kind: if type_only {
                ImportKind::TypeOnly
            } else {
                ImportKind::Runtime
            },
            test_only,
        }
    }
}

/// The flags of the TypeScript or JavaScript `anchor` (see [`ecmascript_anchor`]) written in `file`.
///
/// Type-only: a declaration file, `import type` and `export type ... from`, and a statement whose
/// every specifier is written `type X`. A default or namespace import beside type specifiers, an
/// empty specifier list, `require()` and `import()` are runtime. Test code: a `*.test.*` or
/// `*.spec.*` file, or one under a `__tests__`, `test` or `tests` directory.
pub fn ecmascript_flags(anchor: Node<'_>, source: &str, file: &Path) -> Flags {
    Flags::new(
        is_declaration_file(file) || ecmascript_type_only(anchor, source),
        has_test_segment(file, &["test", "spec"])
            || in_directory(file, &["__tests__", "test", "tests"]),
    )
}

/// The flags of the Python `anchor` (see [`python_anchor`]) written in `file`.
///
/// Type-only: written in the body of `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:`, at any
/// depth below it. Test code: `test_*.py`, `*_test.py`, `conftest.py`, or a file under a `tests` or
/// `test` directory.
pub fn python_flags(anchor: Node<'_>, source: &str, file: &Path) -> Flags {
    let name = file.file_name().and_then(|name| name.to_str());
    let stem = file.file_stem().and_then(|stem| stem.to_str());
    let test_file = name == Some("conftest.py")
        || stem.is_some_and(|stem| stem.starts_with("test_") || stem.ends_with("_test"));
    Flags::new(
        in_type_checking_block(anchor, source),
        test_file || in_directory(file, &["tests", "test"]),
    )
}

/// The flags of the Rust `anchor` (see [`rust_anchor`]) written in `file`.
///
/// Rust has no type-only import. Test code: a file under a `tests` directory or named `*_test.rs`
/// or `*_tests.rs`, or anything written inside an item or module annotated `#[cfg(test)]`, or in a
/// module whose body starts with `#![cfg(test)]`, at any depth.
pub fn rust_flags(anchor: Node<'_>, source: &str, file: &Path) -> Flags {
    let stem = file.file_stem().and_then(|stem| stem.to_str());
    let test_file = stem.is_some_and(|stem| stem.ends_with("_test") || stem.ends_with("_tests"));
    Flags::new(
        false,
        test_file || in_directory(file, &["tests"]) || in_cfg_test(anchor, source),
    )
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

/// Whether a middle segment of the file name, between its first and last dot, is one of `words`:
/// `a.test.ts` and `a.spec.tsx`, never `test.ts`.
fn has_test_segment(file: &Path, words: &[&str]) -> bool {
    let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let segments: Vec<&str> = name.split('.').collect();
    segments
        .iter()
        .skip(1)
        .take(segments.len().saturating_sub(2))
        .any(|segment| words.contains(segment))
}

/// Whether a directory above `file` is named one of `names`.
fn in_directory(file: &Path, names: &[&str]) -> bool {
    file.parent().is_some_and(|dir| {
        dir.components()
            .any(|component| names.iter().any(|name| component.as_os_str() == *name))
    })
}

fn ecmascript_type_only(statement: Node<'_>, source: &str) -> bool {
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

/// Whether `node` is written in the body of an `if TYPE_CHECKING:` statement.
fn in_type_checking_block(node: Node<'_>, source: &str) -> bool {
    let mut child = node;
    while let Some(parent) = child.parent() {
        if parent.kind() == "if_statement"
            && parent.child_by_field_name("consequence") == Some(child)
            && parent
                .child_by_field_name("condition")
                .and_then(|condition| condition.utf8_text(source.as_bytes()).ok())
                .is_some_and(|text| text == "TYPE_CHECKING" || text == "typing.TYPE_CHECKING")
        {
            return true;
        }
        child = parent;
    }
    false
}

/// Whether `node` is inside something annotated `#[cfg(test)]`: it, or an ancestor, is preceded by
/// the attribute, or an ancestor body starts with the inner attribute `#![cfg(test)]`.
fn in_cfg_test(node: Node<'_>, source: &str) -> bool {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if annotated(candidate, source) || starts_with_inner_cfg_test(candidate, source) {
            return true;
        }
        current = candidate.parent();
    }
    false
}

/// Whether the attributes directly above `node` include `#[cfg(test)]`.
fn annotated(node: Node<'_>, source: &str) -> bool {
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" if is_cfg_test(sibling, source) => return true,
            "attribute_item" | "line_comment" | "block_comment" => {}
            _ => return false,
        }
        previous = sibling.prev_sibling();
    }
    false
}

/// Whether the inner attributes at the start of `node`, a file or a module body, include
/// `#![cfg(test)]`.
fn starts_with_inner_cfg_test(node: Node<'_>, source: &str) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .take_while(|child| {
            matches!(
                child.kind(),
                "inner_attribute_item" | "line_comment" | "block_comment"
            )
        })
        .any(|child| child.kind() == "inner_attribute_item" && is_cfg_test(child, source))
}

/// Whether the attribute item is exactly `#[cfg(test)]` or `#![cfg(test)]`, spaces aside.
fn is_cfg_test(attribute: Node<'_>, source: &str) -> bool {
    attribute.utf8_text(source.as_bytes()).is_ok_and(|text| {
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        compact == "#[cfg(test)]" || compact == "#![cfg(test)]"
    })
}
