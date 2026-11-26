//! The edge label words that restrict which import sites an edge allows.

use super::error::DiagramError;
use crate::contract::NodeId;

const TEST_WORD: &str = "test";
const TYPE_WORD: &str = "type";

/// The words `type` and `test` of an edge label, as `(type, test)`.
///
/// A label is a comma-separated list. It is meaningful only when every word, trimmed, is `type` or
/// `test`; any other label is display only and gives `(false, false)`, so `type, helper` and `Test`
/// are plain labels.
///
/// # Errors
/// Returns [`DiagramError::RepeatedLabelWord`] when a meaningful label names a word twice, such as
/// `type, type`: a typo must not stay a silent no-op.
pub(super) fn restrictions(label: &str, line: u32) -> Result<(bool, bool), DiagramError> {
    let words: Vec<&str> = label.split(',').map(str::trim).collect();
    if !words
        .iter()
        .all(|word| matches!(*word, TYPE_WORD | TEST_WORD))
    {
        return Ok((false, false));
    }
    let mut seen = (false, false);
    for word in words {
        let slot = if word == TYPE_WORD {
            &mut seen.0
        } else {
            &mut seen.1
        };
        if *slot {
            return Err(DiagramError::RepeatedLabelWord {
                line,
                word: word.to_owned(),
            });
        }
        *slot = true;
    }
    Ok(seen)
}

/// Whether Mermaid cannot parse `id` as the id of a boundary: a node id it rejects, or `default`,
/// which only a `subgraph` cannot be.
pub(super) fn undrawable_boundary(id: &str) -> bool {
    NodeId::mermaid_rejects(id) || id == "default" || id.starts_with("default-")
}

/// Why Mermaid cannot parse the edge label `label` (the text between the `|`), or `None`.
///
/// A label must not be empty, and outside a leading `"..."` string it must not hold `"`, `(`, `)`,
/// `[`, `]`, `{` or `}`. A string must not be empty, and what follows it is checked the same way.
#[must_use]
pub(super) fn undrawable_edge_label(label: &str) -> Option<&'static str> {
    const BAD: [char; 7] = ['"', '(', ')', '[', ']', '{', '}'];
    if label.is_empty() {
        return Some("it is empty");
    }
    let rest = match label.strip_prefix('"') {
        Some(inner) => match inner.split_once('"') {
            Some(("", _)) => return Some("its quoted text is empty"),
            Some((_, after)) => after,
            None => return Some("it has a `\"` with no closing one"),
        },
        None => label,
    };
    rest.chars().find(|c| BAD.contains(c)).map(|c| match c {
        '"' => "it has a `\"` outside a leading quoted text",
        '(' | ')' => "it has a parenthesis",
        '[' | ']' => "it has a square bracket",
        _ => "it has a curly brace",
    })
}

/// Whether Mermaid cannot parse the comment line `text` (trimmed, starting with `%%`).
///
/// A `%%{` comment is read as a directive, and Mermaid fails on one that is not closed with `}%%`
/// after a name, as in `%%{init: {...}}%%`.
#[must_use]
pub(super) fn undrawable_comment(text: &str) -> bool {
    let Some(after) = text.strip_prefix("%%{") else {
        return false;
    };
    let named = after
        .trim_start()
        .starts_with(|c: char| c.is_alphanumeric() || c == '_');
    !(named && text.ends_with("}%%"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_words_are_read_in_either_order_with_any_spacing() {
        assert_eq!(restrictions("test", 1), Ok((false, true)));
        assert_eq!(restrictions(" type ", 1), Ok((true, false)));
        assert_eq!(restrictions("type, test", 1), Ok((true, true)));
        assert_eq!(restrictions("test,type", 1), Ok((true, true)));
    }

    #[test]
    fn every_other_label_is_plain() {
        for plain in [
            "",
            "Test",
            "tests",
            "type, helper",
            "types",
            "type test",
            "type,",
            "x",
        ] {
            assert_eq!(restrictions(plain, 1), Ok((false, false)), "{plain}");
        }
    }

    #[test]
    fn a_repeated_word_is_an_error_naming_it() {
        assert_eq!(
            restrictions("type, type", 4),
            Err(DiagramError::RepeatedLabelWord {
                line: 4,
                word: "type".to_owned()
            })
        );
        assert!(restrictions("test,test", 1).is_err());
    }

    #[test]
    fn a_boundary_also_cannot_be_default() {
        assert!(undrawable_boundary("default"));
        assert!(undrawable_boundary("default-x"));
        assert!(undrawable_boundary("end-x"));
        assert!(!undrawable_boundary("defaults"));
    }

    #[test]
    fn edge_labels_follow_mermaids_rule() {
        for bad in [
            "",
            "uses (x)",
            "x[1]",
            "{x}",
            "say \"hi\"",
            "\"\"",
            "\"a",
            "\"a\"(b",
            " \"a\"",
            "a]",
        ] {
            assert!(undrawable_edge_label(bad).is_some(), "{bad}");
        }
        for good in [
            "type, test",
            "a b",
            "a:b/c",
            "\"x (y)\"",
            "\"a\"b",
            " ",
            "a<b>",
            "a;b",
            "é",
            "a\\b",
        ] {
            assert_eq!(undrawable_edge_label(good), None, "{good}");
        }
    }

    #[test]
    fn a_directive_comment_must_be_closed() {
        for bad in ["%%{x}", "%%{}", "%%{", "%%{init: {}}", "%%{ }%%"] {
            assert!(undrawable_comment(bad), "{bad}");
        }
        for good in [
            "%% {x}",
            "%% x",
            "%%{init: {\"a\": 1}}%%",
            "%%{x}%%",
            "%% }",
        ] {
            assert!(!undrawable_comment(good), "{good}");
        }
    }
}
