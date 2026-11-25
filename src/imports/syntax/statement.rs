//! Where the statement around a captured specifier starts, and what kind of statement it is.
//!
//! The query captures the module reference, which is often not on the first line of its statement
//! (`import {\n a,\n} from './a'`). Each language names the statement node, and its start line is
//! the location of the import. The same node decides whether the import is type-only and whether
//! it is written in test code.

use std::path::Path;

use tree_sitter::Node;

use crate::code::ImportKind;

/// The nearest ancestor of `node`, or `node` itself, whose kind is one of `kinds`. `node` when
/// there is none, so a location is always found.
pub fn enclosing<'a>(node: Node<'a>, kinds: &[&str]) -> Node<'a> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if kinds.contains(&candidate.kind()) {
            return candidate;
        }
        current = candidate.parent();
    }
    node
}

/// What a statement is for and where it is written, as far as its syntax and its file name say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    /// [`ImportKind::Runtime`] or [`ImportKind::TypeOnly`]. A resolver may still decide on
    /// [`ImportKind::Embed`].
    pub kind: ImportKind,
    /// Whether the statement is written in test code.
    pub test_only: bool,
}

impl Flags {
    /// The flags of a statement that is `type_only` and `test_only`, as far as syntax says.
    #[must_use]
    pub const fn new(type_only: bool, test_only: bool) -> Self {
        Self {
            kind: if type_only {
                ImportKind::TypeOnly
            } else {
                ImportKind::Runtime
            },
            test_only,
        }
    }
}

/// Whether a middle segment of the file name, between its first and last dot, is one of `words`:
/// `a.test.ts` and `a.spec.tsx`, never `test.ts`.
pub fn has_test_segment(file: &Path, words: &[&str]) -> bool {
    let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let segments: Vec<&str> = name.split('.').collect();
    segments
        .iter()
        .skip(1)
        .take(segments.len().saturating_sub(2))
        .any(|segment| words.contains(segment))
}

/// Whether a directory above `file` is named one of `names`.
pub fn in_directory(file: &Path, names: &[&str]) -> bool {
    file.parent().is_some_and(|dir| {
        dir.components()
            .any(|component| names.iter().any(|name| component.as_os_str() == *name))
    })
}
