//! Where the statement around a shell specifier starts, and what kind it is.

use std::path::Path;

use tree_sitter::Node;

use crate::reading::syntax::statement::{Flags, enclosing, in_directory};

/// The shell command around a specifier: the `source` or `.` that holds it.
pub fn anchor(specifier: Node<'_>) -> Node<'_> {
    enclosing(specifier, &["command"])
}

/// The flags of the shell command written in `file`: always runtime. Test code: a file whose name up
/// to its first `.` starts with `test_` or ends with `_test`, or under a `test` or `tests` directory.
pub fn flags(_anchor: Node<'_>, _source: &str, file: &Path) -> Flags {
    Flags::new(false, test_file(file))
}

/// The kinds a `case` pattern is made of. An error node holding only these is the grammar's
/// gap with a pattern that joins several quoted strings and globs (`*"a"*"b"*)`): valid shell,
/// and a pattern is never a command, so it hides no import.
const PATTERN_KINDS: &[&str] = &[
    "concatenation",
    "expansion",
    "extglob_pattern",
    "number",
    "raw_string",
    "simple_expansion",
    "string",
    "string_content",
    "variable_name",
    "word",
];

/// Whether the error node `error` is a `case` pattern the grammar cannot read: it stands in a
/// `case` statement or item and holds nothing but pattern parts.
pub fn case_pattern_gap(error: Node<'_>) -> bool {
    let in_case = error
        .parent()
        .is_some_and(|parent| matches!(parent.kind(), "case_statement" | "case_item"));
    in_case && only_pattern_parts(error)
}

fn only_pattern_parts(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).all(|child| {
        !child.is_error()
            && !child.is_missing()
            && PATTERN_KINDS.contains(&child.kind())
            && only_pattern_parts(child)
    })
}

/// Whether `file` is a shell test file: a name that starts with `test_` or ends with `_test` before
/// its first `.`, or below a `test` or `tests` directory.
pub fn test_file(file: &Path) -> bool {
    let stem = file
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('.').next());
    stem.is_some_and(|stem| stem.starts_with("test_") || stem.ends_with("_test"))
        || in_directory(file, &["test", "tests"])
}
