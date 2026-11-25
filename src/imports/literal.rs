//! Reading the text of string literals, in every grammar that has them.

use tree_sitter::Node;

/// The contents of `node` when it is a plain literal: a string, a template without
/// substitutions, a Rust string or a Python string (see `python_literal`), written without escapes. `None` for anything else, which is
/// computed or cannot be read without unescaping.
pub fn literal_text<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    if node.kind() == "string" && node.child(0).is_some_and(|c| c.kind() == "string_start") {
        return python_literal(node, source);
    }
    let content = match node.kind() {
        "string" | "template_string" => "string_fragment",
        "string_literal" => "string_content",
        _ => return None,
    };
    match node.named_child_count() {
        0 => Some(""),
        1 => {
            let fragment = node.named_child(0)?;
            if fragment.kind() == content {
                fragment.utf8_text(source.as_bytes()).ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The contents of a Python string literal: no prefix but `r` or `u`, no escape sequence, no
/// interpolation and a single piece of text. Adjacent literals are a different node and never get
/// here. `None` for anything else.
fn python_literal<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    let start = node.child(0)?.utf8_text(source.as_bytes()).ok()?;
    let prefix = start.trim_end_matches(['"', '\'']);
    if !(prefix.is_empty() || prefix.eq_ignore_ascii_case("r") || prefix.eq_ignore_ascii_case("u"))
    {
        return None;
    }
    let mut cursor = node.walk();
    let mut content = None;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "string_start" | "string_end" => {}
            "string_content" if child.named_child_count() == 0 && content.is_none() => {
                content = Some(child);
            }
            _ => return None,
        }
    }
    content.map_or(Some(""), |text| text.utf8_text(source.as_bytes()).ok())
}

/// The text of a string literal made of a single fragment, so escapes and templates never produce
/// a half-read specifier.
pub fn plain_string<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    if node.kind() != "string" || node.named_child_count() != 1 {
        return None;
    }
    let fragment = node.named_child(0)?;
    if fragment.kind() != "string_fragment" {
        return None;
    }
    fragment.utf8_text(source.as_bytes()).ok()
}
