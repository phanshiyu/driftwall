//! Core domain types shared by every other module.
//!
//! Every collection here is ordered (`BTreeMap`, `BTreeSet`, sorted `Vec`) so output is
//! deterministic regardless of filesystem or thread ordering.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

/// Errors raised when constructing model values.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    /// A node identifier did not match `[A-Za-z][A-Za-z0-9_-]*`.
    #[error("invalid node id `{0}`: expected [A-Za-z][A-Za-z0-9_-]*")]
    InvalidNodeId(String),
}

/// A validated architecture node identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Validates and wraps `raw`.
    ///
    /// # Errors
    /// Returns [`ModelError::InvalidNodeId`] when `raw` is empty or uses disallowed characters.
    pub fn new(raw: &str) -> Result<Self, ModelError> {
        let mut chars = raw.chars();
        let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
        let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if first_ok && rest_ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ModelError::InvalidNodeId(raw.to_owned()))
        }
    }

    /// The identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a declared dependency is enforced today or only intended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    /// Solid arrow (`-->`): the dependency must exist in code, otherwise it is stale.
    Implemented,
    /// Dotted arrow (`-.->`): declared intent, tolerated whether or not code uses it yet.
    Planned,
}

/// A directed dependency between two nodes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Edge {
    /// The importing node.
    pub from: NodeId,
    /// The imported node.
    pub to: NodeId,
}

/// Metadata for a node declared in the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeInfo {
    /// Human readable label, if the diagram gave one.
    pub label: Option<String>,
    /// Enclosing boundary (`subgraph`) id, if any. Grouping only, never semantic.
    pub boundary: Option<String>,
}

/// The parsed architecture contract: the source of truth.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Contract {
    /// Declared nodes.
    pub nodes: BTreeMap<NodeId, NodeInfo>,
    /// Declared edges and whether each is enforced.
    pub edges: BTreeMap<Edge, EdgeKind>,
}

/// Languages driftwall can extract imports from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// TypeScript and TSX.
    TypeScript,
    /// JavaScript and JSX.
    JavaScript,
    /// Python.
    Python,
    /// Rust, used to lint driftwall itself.
    Rust,
}

/// A location in a source file, 1-indexed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Location {
    /// Path relative to the repository root, using `/` separators.
    pub file: PathBuf,
    /// 1-indexed line.
    pub line: u32,
}

/// Which node owns each source file, plus the tags that named unknown nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ownership {
    /// Owner per repository-relative source file. Files absent here are unmapped.
    pub owners: BTreeMap<PathBuf, NodeId>,
    /// Source files with no inherited tag.
    pub unmapped: BTreeSet<PathBuf>,
    /// `.arch` tags, as (tag file, line, raw node name), for validating against the contract.
    pub tags: Vec<(Location, String)>,
}

/// One import statement resolved to a repository file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ResolvedImport {
    /// Where the import statement is.
    pub at: Location,
    /// Repository-relative file it resolves to.
    pub target: PathBuf,
}

/// An import that points at a repository-relative path that could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct UnresolvedImport {
    /// Where the import statement is.
    pub at: Location,
    /// The raw specifier as written.
    pub specifier: String,
}

/// Kinds of finding the linter can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViolationKind {
    /// A source file has no inherited `.arch` tag.
    UnmappedFile,
    /// A `.arch` tag names a node the diagram does not declare.
    UnknownNode,
    /// Code in node A imports node B and the diagram has no A to B edge.
    UndeclaredEdge,
    /// A solid diagram edge has no import backing it.
    StaleEdge,
    /// A planned (dotted) edge is now used by code and should be made solid.
    PlannedRealized,
    /// A relative import could not be resolved to a file.
    UnresolvedImport,
}

/// How serious a violation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Fails the lint (exit code 1).
    Error,
    /// Reported but does not fail unless `--strict`.
    Warning,
}

/// One finding, with enough location detail for an agent to act on it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Violation {
    /// What went wrong.
    pub kind: ViolationKind,
    /// How serious it is.
    pub severity: Severity,
    /// Offending location, when the finding has one.
    pub at: Option<Location>,
    /// One-line description.
    pub message: String,
    /// Suggested fix, phrased as an action.
    pub hint: Option<String>,
}

/// The complete result of a lint run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LintReport {
    /// Findings, sorted by kind, location, message.
    pub violations: Vec<Violation>,
}

impl LintReport {
    /// True when any finding is an [`Severity::Error`], or any finding at all when `strict`.
    #[must_use]
    pub fn failed(&self, strict: bool) -> bool {
        self.violations
            .iter()
            .any(|v| strict || v.severity == Severity::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_accepts_identifiers() {
        assert!(NodeId::new("billing-service_2").is_ok());
    }

    #[test]
    fn node_id_rejects_bad_input() {
        for bad in ["", "1abc", "a b", "a.b"] {
            assert_eq!(
                NodeId::new(bad),
                Err(ModelError::InvalidNodeId(bad.to_owned()))
            );
        }
    }

    #[test]
    fn warnings_only_fail_when_strict() {
        let report = LintReport {
            violations: vec![Violation {
                kind: ViolationKind::StaleEdge,
                severity: Severity::Warning,
                at: None,
                message: String::new(),
                hint: None,
            }],
        };
        assert!(!report.failed(false));
        assert!(report.failed(true));
    }
}
