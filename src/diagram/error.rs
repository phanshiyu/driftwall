//! Errors raised while parsing a contract.

use thiserror::Error;

use crate::model::{ModelError, NodeId};

/// A contract file used syntax outside the supported subset, or contradicted itself.
///
/// Every variant except [`DiagramError::MissingHeader`] carries the 1-indexed line it refers to.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiagramError {
    /// The file contains no statements, so there is no `flowchart` header.
    #[error("missing `flowchart` header: the file has no statements")]
    MissingHeader,
    /// The first statement is not `flowchart LR|RL|TB|BT`.
    #[error(
        "line {line}: expected `flowchart LR|RL|TB|BT` as the first statement, found `{found}`"
    )]
    BadHeader {
        /// Line of the offending statement.
        line: u32,
        /// The statement as written.
        found: String,
    },
    /// A `flowchart` header appeared after the first statement.
    #[error("line {line}: `flowchart` header must be the first statement")]
    MisplacedHeader {
        /// Line of the repeated header.
        line: u32,
    },
    /// Syntax outside the supported subset.
    #[error("line {line}: unsupported statement `{statement}`")]
    Unsupported {
        /// Line of the statement.
        line: u32,
        /// The statement as written.
        statement: String,
    },
    /// An identifier does not match `[A-Za-z][A-Za-z0-9_-]*`.
    #[error("line {line}: {source}")]
    InvalidId {
        /// Line of the statement.
        line: u32,
        /// Why the identifier was rejected.
        source: ModelError,
    },
    /// A node id collides with a keyword of the format.
    #[error("line {line}: `{word}` is a reserved word and cannot be a node id")]
    ReservedWord {
        /// Line of the statement.
        line: u32,
        /// The reserved word used as an id.
        word: String,
    },
    /// An edge names a node that no statement declares.
    #[error("line {line}: edge endpoint `{id}` is not a declared node")]
    UndeclaredNode {
        /// Line of the edge.
        line: u32,
        /// The undeclared endpoint.
        id: NodeId,
    },
    /// A node was declared twice.
    #[error("line {line}: node `{id}` is already declared on line {first_line}")]
    DuplicateNode {
        /// Line of the second declaration.
        line: u32,
        /// The node id.
        id: NodeId,
        /// Line of the first declaration.
        first_line: u32,
    },
    /// Two edges connect the same ordered pair, whatever their kinds.
    #[error("line {line}: edge `{from}` to `{to}` is already declared on line {first_line}")]
    DuplicateEdge {
        /// Line of the second edge.
        line: u32,
        /// Importing node.
        from: NodeId,
        /// Imported node.
        to: NodeId,
        /// Line of the first edge.
        first_line: u32,
    },
    /// An edge starts and ends at the same node.
    #[error("line {line}: self edge on `{id}`")]
    SelfEdge {
        /// Line of the edge.
        line: u32,
        /// The node.
        id: NodeId,
    },
    /// A boundary id was used by two `subgraph` blocks.
    #[error("line {line}: boundary `{id}` is already declared on line {first_line}")]
    DuplicateBoundary {
        /// Line of the second `subgraph`.
        line: u32,
        /// The boundary id.
        id: String,
        /// Line of the first `subgraph`.
        first_line: u32,
    },
    /// A `subgraph` opened while another was still open.
    #[error(
        "line {line}: nested subgraph (boundary `{open}` opened on line {open_line} is still open)"
    )]
    NestedSubgraph {
        /// Line of the inner `subgraph`.
        line: u32,
        /// The boundary that is still open.
        open: String,
        /// Line where it opened.
        open_line: u32,
    },
    /// A `subgraph` was never closed with `end`.
    #[error("line {line}: subgraph `{id}` is never closed with `end`")]
    UnclosedSubgraph {
        /// Line of the `subgraph`.
        line: u32,
        /// The boundary id.
        id: String,
    },
    /// An `end` with no open `subgraph`.
    #[error("line {line}: `end` without an open subgraph")]
    StrayEnd {
        /// Line of the `end`.
        line: u32,
    },
}

impl DiagramError {
    /// The 1-indexed line the error refers to, or `None` when the file has no statements at all.
    #[must_use]
    pub const fn line(&self) -> Option<u32> {
        match self {
            Self::MissingHeader => None,
            Self::BadHeader { line, .. }
            | Self::MisplacedHeader { line }
            | Self::Unsupported { line, .. }
            | Self::InvalidId { line, .. }
            | Self::ReservedWord { line, .. }
            | Self::UndeclaredNode { line, .. }
            | Self::DuplicateNode { line, .. }
            | Self::DuplicateEdge { line, .. }
            | Self::SelfEdge { line, .. }
            | Self::DuplicateBoundary { line, .. }
            | Self::NestedSubgraph { line, .. }
            | Self::UnclosedSubgraph { line, .. }
            | Self::StrayEnd { line } => Some(*line),
        }
    }
}
