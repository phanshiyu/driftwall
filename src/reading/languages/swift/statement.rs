//! Where the statement around a Swift specifier starts, and what kind it is.

use std::path::Path;

use tree_sitter::Node;

use crate::reading::syntax::statement::{Flags, enclosing, in_directory};

/// The Swift `import` declaration around a specifier, its `@testable` attribute included.
pub fn anchor(specifier: Node<'_>) -> Node<'_> {
    enclosing(specifier, &["import_declaration"])
}

/// The flags of the Swift import written in `file`: always runtime. Test code: a file under a
/// directory named `Tests`, `Test`, `tests` or `test`.
pub fn flags(_anchor: Node<'_>, _source: &str, file: &Path) -> Flags {
    Flags::new(false, test_file(file))
}

/// Whether `file` is below a directory named `Tests`, `Test`, `tests` or `test`.
pub fn test_file(file: &Path) -> bool {
    in_directory(file, &["Tests", "Test", "tests", "test"])
}
