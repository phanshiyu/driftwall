//! The dynamic-import calls of Python, and the module a literal one names.

use tree_sitter::Node;

use super::bindings::{Bindings, Bound};
use crate::reading::syntax::literal::literal_text;
use crate::reading::syntax::resolution::Reference;

/// What the call around a reference is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `importlib.import_module(..)`, `__import__(..)` or `runpy.run_module(..)` under any name
    /// the file binds them to.
    Module {
        /// Whether a dotted prefix may be resolved against a package argument.
        relative: bool,
    },
    /// `runpy.run_path(..)`: a path relative to the working directory at run time.
    RunPath,
    /// `importlib.machinery.SourceFileLoader(..)` or `importlib.util.spec_from_file_location(..)`.
    LoadsFile,
}

/// The kind of dynamic-import call around `reference` (its first or path argument), `None` when
/// the call is some other call.
pub fn classify(reference: &Reference<'_>, bindings: &Bindings) -> Option<Call> {
    let function = reference
        .node
        .parent()?
        .parent()?
        .child_by_field_name("function")?;
    let text = |node: Node<'_>| node.utf8_text(reference.source.as_bytes()).ok();
    let bound = match function.kind() {
        "identifier" => bindings.get(text(function)?)?,
        "attribute" => {
            let object = function.child_by_field_name("object")?;
            // why: only the loader query matches a dotted object, and it is exact on its names.
            if object.kind() == "attribute" {
                return Some(Call::LoadsFile);
            }
            let attribute = text(function.child_by_field_name("attribute")?)?;
            match (bindings.get(text(object)?)?, attribute) {
                (Bound::Importlib, "import_module") => Bound::ImportModule,
                (Bound::Runpy, "run_module") => Bound::RunModule,
                (Bound::Runpy, "run_path") => Bound::RunPath,
                _ => return None,
            }
        }
        _ => return None,
    };
    match bound {
        Bound::DunderImport | Bound::ImportModule => Some(Call::Module { relative: true }),
        Bound::RunModule => Some(Call::Module { relative: false }),
        Bound::RunPath => Some(Call::RunPath),
        Bound::Importlib | Bound::Runpy | Bound::Other => None,
    }
}

/// The absolute dotted module the call around `reference` imports, or `None` when the call stays
/// dynamic.
///
/// The first argument must be a plain string literal naming a module. When it starts with dots it
/// is relative to a package (only when `relative`): the second argument, positional or `package=`, which must be a plain
/// string literal too. The package loses one trailing segment for every dot after the first, and
/// the rest of the literal is appended.
pub fn module_of(reference: &Reference<'_>, relative: bool) -> Option<String> {
    let literal = literal_text(reference.node, reference.source)?;
    let dots = literal.chars().take_while(|&c| c == '.').count();
    let rest = literal.trim_start_matches('.');
    if dots == 0 {
        return is_dotted_name(rest).then(|| rest.to_owned());
    }
    if !rest.is_empty() && !is_dotted_name(rest) {
        return None;
    }
    if !relative {
        return None;
    }
    let package = package_argument(reference)?;
    if !is_dotted_name(package) {
        return None;
    }
    let mut segments: Vec<&str> = package.split('.').collect();
    let keep = segments.len().checked_sub(dots - 1).filter(|&n| n > 0)?;
    segments.truncate(keep);
    segments.extend(rest.split('.').filter(|segment| !segment.is_empty()));
    Some(segments.join("."))
}

/// The text of the package argument when it is a plain string literal: the second positional
/// argument, else the keyword `package`.
fn package_argument<'s>(reference: &Reference<'s>) -> Option<&'s str> {
    let list = reference.node.parent()?;
    let mut cursor = list.walk();
    let arguments: Vec<Node<'s>> = list
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() != "comment")
        .collect();
    let keyword = |argument: &Node<'s>| argument.kind() == "keyword_argument";
    let positional = arguments.get(1).filter(|argument| !keyword(argument));
    let named = arguments.iter().find(|argument| {
        keyword(argument)
            && argument
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(reference.source.as_bytes()).ok())
                == Some("package")
    });
    let value = match positional {
        Some(node) => *node,
        None => named?.child_by_field_name("value")?,
    };
    literal_text(value, reference.source)
}

/// Whether `text` is dotted identifiers: non-empty segments of letters, digits and `_`.
fn is_dotted_name(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|segment| {
            !segment.is_empty() && segment.chars().all(|c| c.is_alphanumeric() || c == '_')
        })
}
