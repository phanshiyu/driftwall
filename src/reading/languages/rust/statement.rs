//! Where the statement around a Rust specifier starts, and what kind it is.

use std::path::Path;

use tree_sitter::Node;

use crate::reading::syntax::statement::{Flags, in_directory};

/// The Rust `use` declaration around a specifier, or the `include_str!` invocation around a string.
/// A path in code is its own anchor.
pub fn anchor(specifier: Node<'_>) -> Node<'_> {
    let parent = specifier.parent();
    match parent {
        Some(parent) if parent.kind() == "use_declaration" => parent,
        Some(parent) if specifier.kind() == "string_literal" => {
            parent.parent().unwrap_or(specifier)
        }
        _ => specifier,
    }
}

/// The flags of the Rust `anchor` (see [`anchor`]) written in `file`.
///
/// Rust has no type-only import. Test code: a file for which [`test_file`] holds, or anything
/// written inside an item or module annotated `#[cfg(test)]`, or in a module whose body starts with
/// `#![cfg(test)]`, at any depth. A file that a test module declaration pulls in is test code too,
/// which this function cannot see: see `test_modules`.
pub fn flags(anchor: Node<'_>, source: &str, file: &Path) -> Flags {
    Flags::new(false, test_file(file) || in_cfg_test(anchor, source))
}

/// Whether the name or place of the Rust `file` says it is test code: a file under a `tests`
/// directory, named `tests.rs`, or named `*_test.rs` or `*_tests.rs`.
pub fn test_file(file: &Path) -> bool {
    let stem = file.file_stem().and_then(|stem| stem.to_str());
    stem.is_some_and(|stem| stem.ends_with("_test") || stem.ends_with("_tests"))
        || file.file_name().is_some_and(|name| name == "tests.rs")
        || in_directory(file, &["tests"])
}

/// Whether `node` is inside something annotated `#[cfg(test)]`: it, or an ancestor, is preceded by
/// the attribute, or an ancestor body starts with the inner attribute `#![cfg(test)]`.
pub fn in_cfg_test(node: Node<'_>, source: &str) -> bool {
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
