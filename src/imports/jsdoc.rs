//! The `import('x')` types written in `JSDoc` comments, see "`JSDoc` type imports" in
//! `docs/DESIGN.md`.

use tree_sitter::Node;

/// A module named by a type in a `/**` comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocImport<'s> {
    /// The string between the quotes of `import('..')`.
    pub specifier: &'s str,
    /// The identifier after `import('..').`, when there is one.
    pub member: Option<&'s str>,
    /// The line of the `import` word.
    pub line: u32,
}

/// The `JSDoc` imports of every comment below `root`, in source order.
pub fn read<'s>(root: Node<'_>, source: &'s str) -> Vec<DocImport<'s>> {
    let mut found = Vec::new();
    let mut cursor = root.walk();
    'walk: loop {
        let node = cursor.node();
        if node.kind() == "comment" {
            if let Some(text) = source.get(node.byte_range()) {
                let first = u32::try_from(node.start_position().row).unwrap_or(u32::MAX);
                imports_in(text, first.saturating_add(1), &mut found);
            }
        } else if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
    found
}

/// Adds the imports of the comment `text`, whose first line is `first_line`, to `found`. Only a
/// comment that starts with `/**` (and is not `/**/`) is read.
fn imports_in<'s>(text: &'s str, first_line: u32, found: &mut Vec<DocImport<'s>>) {
    if !text.starts_with("/**") || text == "/**/" {
        return;
    }
    for (start, _) in text.match_indices("import") {
        let before = text.get(..start).and_then(|head| head.chars().next_back());
        if before.is_some_and(|c| is_identifier(c) || c == '.') {
            continue;
        }
        let Some((specifier, member)) = text.get(start + "import".len()..).and_then(call) else {
            continue;
        };
        let newlines = text
            .get(..start)
            .map_or(0, |head| head.matches('\n').count());
        found.push(DocImport {
            specifier,
            member,
            line: first_line.saturating_add(u32::try_from(newlines).unwrap_or(u32::MAX)),
        });
    }
}

/// The specifier and member of `('x')` or `("x").Member` at the start of `rest`.
fn call(rest: &str) -> Option<(&str, Option<&str>)> {
    let rest = rest.trim_start().strip_prefix('(')?.trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let body = rest.get(1..)?;
    let end = body.find(quote)?;
    let specifier = body.get(..end)?;
    if specifier.contains(['\\', '\n']) {
        return None;
    }
    let after = body.get(end + 1..)?.trim_start().strip_prefix(')')?;
    let member = after.strip_prefix('.').map(|tail| {
        let length = tail.find(|c: char| !is_identifier(c)).unwrap_or(tail.len());
        tail.get(..length).unwrap_or_default()
    });
    Some((specifier, member.filter(|name| !name.is_empty())))
}

fn is_identifier(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}
