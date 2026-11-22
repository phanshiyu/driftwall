//! The module a literal `importlib.import_module(..)` or `__import__(..)` call names.

use tree_sitter::Node;

use super::Reference;
use super::extract::literal_text;

/// The absolute dotted module the call around `reference` imports, or `None` when the call stays
/// dynamic.
///
/// The first argument must be a plain string literal naming a module. When it starts with dots it
/// is relative to a package: the second argument, positional or `package=`, which must be a plain
/// string literal too. The package loses one trailing segment for every dot after the first, and
/// the rest of the literal is appended.
pub fn module_of(reference: &Reference<'_>) -> Option<String> {
    let literal = literal_text(reference.node, reference.source)?;
    let dots = literal.chars().take_while(|&c| c == '.').count();
    let rest = literal.trim_start_matches('.');
    if dots == 0 {
        return is_dotted_name(rest).then(|| rest.to_owned());
    }
    if !rest.is_empty() && !is_dotted_name(rest) {
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
