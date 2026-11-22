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

/// JavaScript and JSX.
pub const JAVASCRIPT: &str = ecmascript_imports!();

/// TypeScript and TSX: the JavaScript forms plus `import x = require('a')`.
pub const TYPESCRIPT: &str = concat!(
    ecmascript_imports!(),
    "(import_require_clause source: (string) @specifier)\n"
);
