//! Tree-sitter queries that capture import specifiers, one constant per language.
//!
//! Every query must capture the module reference node as `@specifier`. A query may also capture
//! imported names as `@name`; all captures of one match are resolved together.

/// Forms shared by TypeScript and JavaScript: `import ... from`, `import 'a'`, every
/// `export ... from`, dynamic `import(...)` and `require(...)`.
macro_rules! ecmascript_imports {
    () => {
        r#"
(import_statement source: (string) @specifier)
(export_statement source: (string) @specifier)
(call_expression
  function: (import)
  arguments: (arguments . (string) @specifier))
(call_expression
  function: (identifier) @callee
  arguments: (arguments . (string) @specifier .)
  (#eq? @callee "require"))
"#
    };
}

/// Python: `import a.b`, `import a.b as c`, and `from a.b import c, d as e` or `import *`.
///
/// A `from` statement is a single match carrying every imported name, so the resolver can try
/// submodules before the module itself. `from __future__` is its own node and never matches.
pub const PYTHON: &str = r"
(import_statement
  name: (dotted_name) @specifier)
(import_statement
  name: (aliased_import name: (dotted_name) @specifier))
(import_from_statement
  module_name: (_) @specifier
  [
    name: (dotted_name) @name
    name: (aliased_import name: (dotted_name) @name)
  ]*)
";

/// JavaScript and JSX.
pub const JAVASCRIPT: &str = ecmascript_imports!();

/// TypeScript and TSX: the JavaScript forms plus `import x = require('a')`.
pub const TYPESCRIPT: &str = concat!(
    ecmascript_imports!(),
    "(import_require_clause source: (string) @specifier)\n"
);

/// Rust: the argument of every `use` declaration, every scoped path in any position, and every
/// token inside a macro argument list (macro arguments are not parsed into expressions, so the
/// resolver reassembles `a::b::c` from the tokens). `mod foo;` is containment and is not matched.
///
/// A match is only a candidate. The resolver keeps the outermost path and drops the paths that
/// are inside a `use` declaration or a visibility modifier, which have their own handling.
pub const RUST: &str = r"
(use_declaration argument: (_) @specifier)
(scoped_identifier) @specifier
(scoped_type_identifier) @specifier
(token_tree [(crate) (self) (super) (identifier)] @specifier)
";
