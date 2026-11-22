//! Where the statement around a captured specifier starts, and what kind of statement it is.
//!
//! The query captures the module reference, which is often not on the first line of its statement
//! (`import {\n a,\n} from './a'`). Each language names the statement node, and its start line is
//! the location of the import.

use tree_sitter::Node;

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

/// The Rust `use` declaration around a specifier. A path in code is its own anchor.
pub fn rust_anchor(specifier: Node<'_>) -> Node<'_> {
    match specifier.parent() {
        Some(parent) if parent.kind() == "use_declaration" => parent,
        _ => specifier,
    }
}
