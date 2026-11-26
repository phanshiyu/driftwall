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
