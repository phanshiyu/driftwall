//! Tree-sitter queries that capture import specifiers, one constant per language.
//!
//! Every query must capture the module reference node as `@specifier`. A query may also capture
//! imported names as `@name`; all captures of one match are resolved together.

/// Forms shared by TypeScript and JavaScript: `import ... from`, `import 'a'`, every
/// `export ... from`, dynamic `import(...)`, `require(...)` and `new URL(..., import.meta.url)`. The argument of a call is captured
/// whatever it is, so the resolver can tell a literal from a computed specifier. The forms it
/// cannot follow are captured too, so they are recorded and never dropped: `import.meta.glob(..)`,
/// `require.resolve(..)`, `createRequire(..)(..)`, and a variable set to `require` or to
/// `createRequire(..)`.
macro_rules! ecmascript_imports {
    () => {
        r#"
(import_statement source: (string) @specifier)
(export_statement source: (string) @specifier)
(call_expression
  function: (import)
  arguments: (arguments . (_) @specifier))
(call_expression
  function: (identifier) @callee
  arguments: (arguments . (_) @specifier .)
  (#eq? @callee "require"))
(call_expression
  function: (member_expression
    object: (meta_property)
    property: (property_identifier) @glob)
  arguments: (arguments . (_) @specifier)
  (#any-of? @glob "glob" "globEager"))
(call_expression
  function: (member_expression
    object: (identifier) @owner
    property: (property_identifier) @method)
  arguments: (arguments . (_) @specifier)
  (#eq? @owner "require")
  (#eq? @method "resolve"))
(call_expression
  function: (call_expression
    function: (identifier) @maker)
  arguments: (arguments . (_) @specifier)
  (#eq? @maker "createRequire"))
(variable_declarator
  name: (identifier) @alias
  value: (identifier) @specifier
  (#eq? @specifier "require")
  (#not-eq? @alias "require"))
(variable_declarator
  name: (identifier) @alias
  value: (call_expression
    function: (identifier) @maker) @specifier
  (#eq? @maker "createRequire")
  (#not-eq? @alias "require"))
(new_expression
  constructor: (identifier) @constructor
  arguments: (arguments . (_) @specifier . (_) @base .)
  (#eq? @constructor "URL")
  (#eq? @base "import.meta.url"))
"#
    };
}

/// Python: `import a.b`, `import a.b as c`, and `from a.b import c, d as e` or `import *`, plus the
/// first argument of every call to a name or a `name.attr` (the resolver keeps `import_module`,
/// `__import__`, `run_module` and `run_path` under the names the file binds), and the path (second
/// argument) of `importlib.machinery.SourceFileLoader(..)` and `importlib.util.spec_from_file_location(..)`.
///
/// A `from` statement is a single match carrying every imported name, so the resolver can try
/// submodules before the module itself. `from __future__` is its own node and never matches.
pub const PYTHON: &str = r#"
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
(call
  function: (identifier)
  arguments: (argument_list . (_) @specifier))
(call
  function: (attribute
    object: (identifier)
    attribute: (identifier))
  arguments: (argument_list . (_) @specifier))
(call
  function: (attribute
    object: (attribute
      object: (identifier) @module
      attribute: (identifier) @submodule)
    attribute: (identifier) @method)
  arguments: (argument_list . (_) . (_) @specifier)
  (#eq? @module "importlib")
  (#any-of? @submodule "machinery" "util")
  (#any-of? @method "SourceFileLoader" "spec_from_file_location"))
"#;

/// Swift: the module path of every `import` declaration, whatever attribute or keyword it carries.
pub const SWIFT: &str = "(import_declaration (identifier) @specifier)\n";

/// Shell: the first argument of every `source` and `.` command. The resolver evaluates the path.
pub const SHELL: &str = r#"
(command
  name: (command_name (word) @callee)
  .
  argument: (_) @specifier
  (#any-of? @callee "source" "."))
"#;

/// JavaScript and JSX.
pub const JAVASCRIPT: &str = ecmascript_imports!();

/// TypeScript and TSX: the JavaScript forms plus `import x = require('a')`.
pub const TYPESCRIPT: &str = concat!(
    ecmascript_imports!(),
    "(import_require_clause source: (string) @specifier)\n"
);

/// Rust: the argument of every `use` declaration, every scoped path in any position, every
/// token inside a macro argument list (macro arguments are not parsed into expressions, so the
/// resolver reassembles `a::b::c` from the tokens), and the string literal of `include_str!`,
/// `include_bytes!` and `include!`. `mod foo;` is containment and is not matched.
///
/// A match is only a candidate. The resolver keeps the outermost path and drops the paths that
/// are inside a `use` declaration or a visibility modifier, which have their own handling.
pub const RUST: &str = r#"
(use_declaration argument: (_) @specifier)
(scoped_identifier) @specifier
(scoped_type_identifier) @specifier
(token_tree [(crate) (self) (super) (identifier)] @specifier)
(macro_invocation
  macro: [
    (identifier) @macro
    (scoped_identifier name: (identifier) @macro)
  ]
  (token_tree . (string_literal) @specifier .)
  (#any-of? @macro "include_str" "include_bytes" "include"))
"#;
