//! Parsing of `.arch` files.

use std::fmt;
use std::path::Path;

use super::error::{ArchProblem, OwnershipError};
use crate::model::{Location, NodeId};

/// The `node:` value that asks for one node per module.
const MODULES_MARK: &str = "*";

/// What a `.arch` file says about its directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    /// `node: <id>`: the directory and everything below it is one node.
    Node(NodeId),
    /// `node: *`: every module directly in the directory is its own node, named by its position.
    Modules,
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(id) => id.fmt(f),
            Self::Modules => f.write_str(MODULES_MARK),
        }
    }
}

/// The tag a `.arch` file declares, with where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchTag {
    /// What the file declares.
    pub tag: Tag,
    /// The raw value text as written.
    pub raw: String,
    /// 1-indexed line of the `node:` declaration.
    pub line: u32,
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

    let mut found: Option<ArchTag> = None;
    for (index, text) in contents.lines().enumerate() {
        let line = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
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
        let tag = if raw == MODULES_MARK {
            Tag::Modules
        } else {
            Tag::Node(NodeId::new(raw).map_err(|e| fail(line, ArchProblem::InvalidId(e)))?)
        };
        found = Some(ArchTag {
            tag,
            raw: raw.to_owned(),
            line,
        });
    }
    found.ok_or_else(|| fail(1, ArchProblem::MissingNode))
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
        assert_eq!(tag.tag, Tag::Modules);
        assert_eq!(tag.raw, "*");
        assert_eq!(tag.line, 2);
        assert_eq!(tag.tag.to_string(), "*");
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
