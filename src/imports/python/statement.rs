//! Where the statement around a Python specifier starts, and what kind it is.

use std::path::Path;

use tree_sitter::Node;

use crate::imports::syntax::statement::{Flags, enclosing, in_directory};

/// The Python `import` or `from` statement around a specifier, or the dynamic import call.
pub fn anchor(specifier: Node<'_>) -> Node<'_> {
    enclosing(
        specifier,
        &["import_statement", "import_from_statement", "call"],
    )
}

/// The flags of the Python `anchor` (see [`anchor`]) written in `file`.
///
/// Type-only: written in the body of `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:`, at any
/// depth below it. Test code: `test_*.py`, `*_test.py`, `conftest.py`, or a file under a `tests` or
/// `test` directory.
pub fn flags(anchor: Node<'_>, source: &str, file: &Path) -> Flags {
    Flags::new(in_type_checking_block(anchor, source), test_file(file))
}

/// Whether `file` is a Python test file: `test_*.py`, `*_test.py`, `conftest.py`, or below a
/// `tests` or `test` directory.
pub fn test_file(file: &Path) -> bool {
    let name = file.file_name().and_then(|name| name.to_str());
    let stem = file.file_stem().and_then(|stem| stem.to_str());
    name == Some("conftest.py")
        || stem.is_some_and(|stem| stem.starts_with("test_") || stem.ends_with("_test"))
        || in_directory(file, &["tests", "test"])
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
