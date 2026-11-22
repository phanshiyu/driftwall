//! Parsing of `.arch` files.

use std::path::Path;

use super::error::{ArchProblem, OwnershipError};
use crate::model::{Location, MODULES_MARK, ModelError, NodeId, Prefix, SCOPE_HERE, Scope, Tag};

/// The tag a `.arch` file declares, with where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchTag {
    /// What the file declares.
    pub tag: Tag,
    /// How far the tag reaches below its directory.
    pub scope: Scope,
    /// The raw value text as written.
    pub raw: String,
    /// 1-indexed line of the `node:` declaration.
    pub line: u32,
}

/// Parses the value of a `node:` line: an id, `*`, or a prefix followed by `*`.
fn parse_tag(raw: &str) -> Result<Tag, ModelError> {
    match raw.strip_suffix(MODULES_MARK) {
        Some("") => Ok(Tag::Modules(Prefix::default())),
        Some(prefix) => Prefix::new(prefix).map(Tag::Modules),
        None => NodeId::new(raw).map(Tag::Node),
    }
}

/// Parses the contents of a `.arch` file. `file` is only used to label errors.
///
/// A file without a `node:` line is reported at line 1, since no better line exists.
pub fn parse(file: &Path, contents: &str) -> Result<ArchTag, OwnershipError> {
    let fail = |line: u32, problem: ArchProblem| OwnershipError::InvalidArch {
        at: Location {
            file: file.to_path_buf(),
            line,
        },
        problem,
    };

    let mut found: Option<(Tag, String, u32)> = None;
    let mut scope: Option<Scope> = None;
    for (index, text) in contents.lines().enumerate() {
        let line = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("scope:") {
            if scope.is_some() {
                return Err(fail(line, ArchProblem::DuplicateScope));
            }
            let value = value.trim();
            if value != SCOPE_HERE {
                return Err(fail(line, ArchProblem::InvalidScope(value.to_owned())));
            }
            scope = Some(Scope::Here);
            continue;
        }
        let Some(value) = trimmed.strip_prefix("node:") else {
            return Err(fail(
                line,
                ArchProblem::UnrecognisedLine(trimmed.to_owned()),
            ));
        };
        if found.is_some() {
            return Err(fail(line, ArchProblem::DuplicateNode));
        }
        let raw = value.trim();
        let tag = parse_tag(raw).map_err(|e| fail(line, ArchProblem::InvalidId(e)))?;
        found = Some((tag, raw.to_owned(), line));
    }
    let (tag, raw, line) = found.ok_or_else(|| fail(1, ArchProblem::MissingNode))?;
    Ok(ArchTag {
        tag,
        scope: scope.unwrap_or_default(),
        raw,
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelError;

    fn problem_of(contents: &str) -> (u32, ArchProblem) {
        match parse(Path::new("x/.arch"), contents) {
            Err(OwnershipError::InvalidArch { at, problem }) => {
                assert_eq!(at.file, Path::new("x/.arch"));
                (at.line, problem)
            }
            other => panic!("expected InvalidArch, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_node_line_among_comments_and_blanks() {
        let tag = parse(Path::new(".arch"), "# owner\n\n  node:   billing  \n").unwrap();
        assert_eq!(tag.tag, Tag::Node(NodeId::new("billing").unwrap()));
        assert_eq!(tag.raw, "billing");
        assert_eq!(tag.line, 3);
    }

    #[test]
    fn accepts_no_space_after_colon() {
        let tag = parse(Path::new(".arch"), "node:core").unwrap();
        assert_eq!(tag.tag, Tag::Node(NodeId::new("core").unwrap()));
    }

    #[test]
    fn a_star_asks_for_module_nodes() {
        let tag = parse(Path::new(".arch"), "# one node per module\nnode: *\n").unwrap();
        assert_eq!(tag.tag, Tag::Modules(Prefix::default()));
        assert_eq!(tag.scope, Scope::Subtree);
        assert_eq!(tag.raw, "*");
        assert_eq!(tag.line, 2);
        assert_eq!(tag.tag.to_string(), "*");
    }

    #[test]
    fn a_prefix_before_the_star_names_the_module_prefix() {
        for (raw, prefix) in [("ui-*", "ui-"), ("a_b-c-*", "a_b-c-"), ("X9--*", "X9--")] {
            let tag = parse(Path::new(".arch"), &format!("node: {raw}\n")).unwrap();
            assert_eq!(tag.tag, Tag::Modules(Prefix::new(prefix).unwrap()), "{raw}");
            assert_eq!(tag.raw, raw);
            assert_eq!(tag.tag.to_string(), raw);
        }
    }

    #[test]
    fn an_invalid_prefix_is_an_invalid_id_at_its_line() {
        for bad in [
            "ui*", "ui_*", "-*", "9-*", "u i-*", "ui.-*", "ui-**", "*-*", "ui-*a", "*a",
        ] {
            let (line, problem) = problem_of(&format!("# c\nnode: {bad}\n"));
            assert_eq!(line, 2, "{bad}");
            assert!(matches!(problem, ArchProblem::InvalidId(_)), "{bad}");
        }
        assert_eq!(
            problem_of("node: ui*\n"),
            (
                1,
                ArchProblem::InvalidId(ModelError::InvalidPrefix("ui".to_owned()))
            )
        );
    }

    #[test]
    fn a_star_mixed_with_text_is_an_invalid_id() {
        for bad in ["node: **", "node: *a", "node: a*"] {
            let (line, problem) = problem_of(bad);
            assert_eq!(line, 1, "{bad}");
            assert!(matches!(problem, ArchProblem::InvalidId(_)), "{bad}");
        }
    }

    #[test]
    fn scope_here_limits_the_tag_to_its_directory() {
        for contents in [
            "node: root\nscope: here\n",
            "# c\nscope:   here \n\nnode: root\n",
        ] {
            let tag = parse(Path::new(".arch"), contents).unwrap();
            assert_eq!(tag.scope, Scope::Here, "{contents:?}");
            assert_eq!(tag.tag, Tag::Node(NodeId::new("root").unwrap()));
        }
        let tag = parse(Path::new(".arch"), "scope: here\nnode: ui-*\n").unwrap();
        assert_eq!(tag.scope, Scope::Here);
        assert_eq!(tag.tag, Tag::Modules(Prefix::new("ui-").unwrap()));
        assert_eq!(tag.line, 2);
    }

    #[test]
    fn the_scope_defaults_to_the_whole_subtree() {
        let tag = parse(Path::new(".arch"), "node: root\n").unwrap();
        assert_eq!(tag.scope, Scope::Subtree);
    }

    #[test]
    fn a_bad_scope_is_reported_at_its_line() {
        assert_eq!(
            problem_of("node: a\nscope: subtree\n"),
            (2, ArchProblem::InvalidScope("subtree".to_owned()))
        );
        assert_eq!(
            problem_of("scope:\nnode: a\n"),
            (1, ArchProblem::InvalidScope(String::new()))
        );
        assert_eq!(
            problem_of("node: a\nscope: here\n# c\nscope: here\n"),
            (4, ArchProblem::DuplicateScope)
        );
    }

    #[test]
    fn a_scope_alone_misses_the_node_line() {
        assert_eq!(problem_of("scope: here\n"), (1, ArchProblem::MissingNode));
    }

    #[test]
    fn a_node_tag_displays_its_id() {
        let tag = parse(Path::new(".arch"), "node: core\n").unwrap();
        assert_eq!(tag.tag.to_string(), "core");
    }

    #[test]
    fn empty_and_comment_only_files_miss_the_node_line() {
        assert_eq!(problem_of(""), (1, ArchProblem::MissingNode));
        assert_eq!(problem_of("# nothing\n\n"), (1, ArchProblem::MissingNode));
    }

    #[test]
    fn second_node_line_is_reported_at_its_own_line() {
        assert_eq!(
            problem_of("node: a\n# c\nnode: b\n"),
            (3, ArchProblem::DuplicateNode)
        );
    }

    #[test]
    fn bad_id_is_reported_at_its_line() {
        assert_eq!(
            problem_of("# c\nnode: 9lives\n"),
            (
                2,
                ArchProblem::InvalidId(ModelError::InvalidNodeId("9lives".to_owned()))
            )
        );
        assert_eq!(
            problem_of("node:\n"),
            (
                1,
                ArchProblem::InvalidId(ModelError::InvalidNodeId(String::new()))
            )
        );
    }

    #[test]
    fn garbage_line_is_reported_at_its_line() {
        assert_eq!(
            problem_of("node: a\nowner: b\n"),
            (2, ArchProblem::UnrecognisedLine("owner: b".to_owned()))
        );
    }

    #[test]
    fn trailing_comment_on_node_line_is_not_allowed() {
        let (line, problem) = problem_of("node: a # why\n");
        assert_eq!(line, 1);
        assert!(matches!(problem, ArchProblem::InvalidId(_)));
    }
}
