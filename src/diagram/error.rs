//! Errors raised while parsing a contract.

use thiserror::Error;

use crate::model::{Endpoint, ModelError, NodeId};

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
    #[error("line {line}: invalid identifier")]
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
    /// An edge names a node or boundary that no statement declares.
    #[error("line {line}: edge endpoint `{id}` is not a declared node or boundary")]
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
        /// Importing end.
        from: Endpoint,
        /// Imported end.
        to: Endpoint,
        /// Line of the first edge.
        first_line: u32,
    },
    /// An edge has a boundary at one end that declares no node, so it would cover nothing.
    #[error(
        "line {line}: boundary `{id}` declares no node, so an edge to or from it covers nothing"
    )]
    EmptyBoundary {
        /// Line of the edge.
        line: u32,
        /// The boundary.
        id: String,
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
    /// A node and a boundary share an id (one namespace, as in Mermaid).
    #[error(
        "line {line}: `{id}` is already declared as a {} on line {first_line}",
        if *first_is_boundary { "boundary" } else { "node" }
    )]
    IdClash {
        /// Line of the second declaration.
        line: u32,
        /// The shared id.
        id: String,
        /// Line of the first declaration.
        first_line: u32,
        /// Whether the first declaration was the `subgraph` (else the node).
        first_is_boundary: bool,
    },
    /// A `subgraph` was never closed with `end`: the innermost one that is still open.
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
    /// A `%% exports` line that is not `%% exports <node>: <Name>, <Name>`.
    #[error("line {line}: malformed `%% exports` line: {reason}")]
    MalformedExports {
        /// Line of the directive.
        line: u32,
        /// What is wrong with it.
        reason: String,
    },
    /// Two `%% exports` lines name the same node.
    #[error("line {line}: node `{id}` already has an `%% exports` line on line {first_line}")]
    DuplicateExports {
        /// Line of the second directive.
        line: u32,
        /// The node id.
        id: NodeId,
        /// Line of the first directive.
        first_line: u32,
    },
    /// A `%% exports` line names a node that no statement declares.
    #[error("line {line}: `%% exports` names `{id}`, which is not a declared node")]
    UndeclaredExportsNode {
        /// Line of the directive.
        line: u32,
        /// The undeclared node.
        id: NodeId,
    },
    /// An edge label such as `type, type` names one of its words twice.
    #[error("line {line}: the edge label names `{word}` twice")]
    RepeatedLabelWord {
        /// Line of the edge.
        line: u32,
        /// The repeated word.
        word: String,
    },
    /// A rule directive (`%% order:` and the others of "Contract rules") that is malformed or names
    /// something that is not declared.
    #[error("line {line}: malformed `%% {rule}` line: {reason}")]
    MalformedRule {
        /// Line of the directive.
        line: u32,
        /// The directive keyword, such as `order`.
        rule: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A rule directive given twice (once per node for `%% budget`).
    #[error("line {line}: `%% {rule}` is already given on line {first_line}")]
    DuplicateRule {
        /// Line of the second directive.
        line: u32,
        /// The directive keyword and its node for a budget, such as `order` or `budget lint`.
        rule: String,
        /// Line of the first directive.
        first_line: u32,
    },
    /// The declared edges form a cycle that a `%% no-cycles` line forbids.
    #[error(
        "line {line}: `%% no-cycles` forbids {what} cycles, but the declared edges form one: {cycle}"
    )]
    DeclaredCycle {
        /// Line of the directive.
        line: u32,
        /// What the directive covers: `node` or `boundary`.
        what: String,
        /// The cycle, written as its edges.
        cycle: String,
    },
    /// An arrow that points up the `%% order`.
    #[error(
        "line {line}: edge `{from}` to `{to}` points upward: {relation} in the `%% order` on line {order_line}"
    )]
    UpwardArrow {
        /// Line of the edge.
        line: u32,
        /// Importing end.
        from: Endpoint,
        /// Imported end.
        to: Endpoint,
        /// What puts it upward, such as "`shared` is below `engine`".
        relation: String,
        /// Line of the `%% order` directive.
        order_line: u32,
    },
    /// A declared edge that joins two children of a `%% isolated` boundary.
    #[error(
        "line {line}: edge `{edge}` joins `{}` and `{}`, children of `{boundary}`, which `%% isolated` on line {isolated_line} keeps apart",
        apart.0, apart.1
    )]
    IsolatedArrow {
        /// Line of the edge.
        line: u32,
        /// The edge as written, `a --> b`, label included.
        edge: String,
        /// The isolated boundary.
        boundary: String,
        /// The two children the edge joins.
        // why: boxed so the largest variant stays small; clippy flags every `Result` otherwise.
        apart: Box<(String, String)>,
        /// Line of the `%% isolated` directive.
        isolated_line: u32,
    },
    /// A `%% packages` or `%% forbid-packages` line that is not `%% packages <node>: <name>, <name>`.
    #[error("line {line}: malformed `%% {directive}` line: {reason}")]
    MalformedPackages {
        /// Line of the directive.
        line: u32,
        /// The directive keyword, `packages` or `forbid-packages`.
        directive: &'static str,
        /// What is wrong with it.
        reason: String,
    },
    /// A `%% packages` or `%% forbid-packages` line names a node that no statement declares.
    #[error("line {line}: `%% {directive}` names `{id}`, which is not a declared node")]
    UndeclaredPackagesNode {
        /// Line of the directive.
        line: u32,
        /// The directive keyword, `packages` or `forbid-packages`.
        directive: &'static str,
        /// The undeclared node.
        id: NodeId,
    },
    /// A `%% map` line that is not `%% map <target>: <glob>, <glob>`.
    #[error("line {line}: malformed `%% map` line: {reason}")]
    MalformedMap {
        /// Line of the directive.
        line: u32,
        /// What is wrong with it.
        reason: String,
    },
    /// The same glob appears on two `%% map` lines, or twice on one.
    #[error("line {line}: map glob `{glob}` is already mapped on line {first_line}")]
    DuplicateMapGlob {
        /// Line of the second occurrence.
        line: u32,
        /// The glob as written there.
        glob: String,
        /// Line of the first occurrence.
        first_line: u32,
    },
    /// A `%% map` line names a node that no statement declares.
    #[error("line {line}: `%% map` names `{id}`, which is not a declared node")]
    UndeclaredMapNode {
        /// Line of the directive.
        line: u32,
        /// The undeclared node.
        id: NodeId,
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
            | Self::EmptyBoundary { line, .. }
            | Self::SelfEdge { line, .. }
            | Self::DuplicateBoundary { line, .. }
            | Self::IdClash { line, .. }
            | Self::UnclosedSubgraph { line, .. }
            | Self::StrayEnd { line }
            | Self::MalformedExports { line, .. }
            | Self::DuplicateExports { line, .. }
            | Self::UndeclaredExportsNode { line, .. }
            | Self::MalformedPackages { line, .. }
            | Self::UndeclaredPackagesNode { line, .. }
            | Self::MalformedMap { line, .. }
            | Self::DuplicateMapGlob { line, .. }
            | Self::RepeatedLabelWord { line, .. }
            | Self::MalformedRule { line, .. }
            | Self::DuplicateRule { line, .. }
            | Self::UpwardArrow { line, .. }
            | Self::IsolatedArrow { line, .. }
            | Self::DeclaredCycle { line, .. }
            | Self::UndeclaredMapNode { line, .. } => Some(*line),
        }
    }
}
