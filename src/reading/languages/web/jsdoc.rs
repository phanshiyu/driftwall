//! The `import('x')` types written in `JSDoc` comments, see "`JSDoc` type imports" in
//! `docs/DESIGN.md`.

use tree_sitter::Node;

use crate::reading::syntax::spec::DocImport;

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
                reference_path(text, first.saturating_add(1), &mut found);
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
            relative: false,
        });
    }
}

/// Adds the path of the triple-slash directive `/// <reference path="x" />` that `text` is, if it
/// is one. `types` and `lib` directives name packages and are not read.
fn reference_path<'s>(text: &'s str, line: u32, found: &mut Vec<DocImport<'s>>) {
    let Some(rest) = text
        .strip_prefix("///")
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix("<reference"))
    else {
        return;
    };
    let Some((_, after)) = rest.split_once("path") else {
        return;
    };
    let Some(value) = after.trim_start().strip_prefix('=').map(str::trim_start) else {
        return;
    };
    let Some(quote) = value.chars().next().filter(|c| matches!(c, '\'' | '"')) else {
        return;
    };
    let Some(body) = value.get(1..) else {
        return;
    };
    if let Some(end) = body.find(quote)
        && let Some(specifier) = body.get(..end).filter(|path| !path.is_empty())
    {
        found.push(DocImport {
            specifier,
            member: None,
            line,
            relative: true,
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
