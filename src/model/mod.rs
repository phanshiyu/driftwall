//! Core domain types shared by every other module.
//!
//! Every collection here is ordered (`BTreeMap`, `BTreeSet`, sorted `Vec`) so output is
//! deterministic regardless of filesystem or thread ordering.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::{Serialize, Serializer};
use thiserror::Error;

/// Contract path used when none is given, relative to the repository root.
pub const DEFAULT_CONTRACT: &str = "architecture.mmd";

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
    /// Public surface per node, from `%% exports` lines. A node absent here is unrestricted, and a
    /// node present here has at least one name.
    pub surfaces: BTreeMap<NodeId, BTreeSet<String>>,
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

/// The `node:` value of a `.arch` file that asks for one node per module.
pub const MODULES_MARK: &str = "*";

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

/// Why the scan left a source file out of the repository's own source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Exclusion {
    /// The file is below a directory named `vendor` or `third_party`.
    Vendored,
    /// The file name has the form `<name>.min.<ext>`.
    Minified,
}

impl Exclusion {
    /// The reason as a lowercase word, for reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vendored => "vendored",
            Self::Minified => "minified",
        }
    }
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
    /// What each `.arch` file the scan saw says, by the repository-relative directory it is in
    /// (empty for the root).
    pub tag_dirs: BTreeMap<PathBuf, Tag>,
    /// Source files the scan left out, by reason. They are neither owned nor unmapped.
    pub excluded: BTreeMap<Exclusion, BTreeSet<PathBuf>>,
}

/// A symbol an import uses from its target file, exactly as the importing code writes it.
///
/// Serialized as a plain string: the name itself, or `*` for [`Symbol::All`]. A name never
/// contains `*`, so the two cannot collide. Ordering is by name, with `*` last, so a set of
/// symbols serializes in the same order on every run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Symbol {
    /// One named item: a function, type, constant or module inside the target, or `default`.
    Name(String),
    /// The whole target: a glob, a namespace import, or a Python module imported as a whole. A
    /// Rust module name that is only bound (`use crate::gitlab;`) is not this: it uses nothing.
    All,
}

impl Symbol {
    /// The text a symbol is serialized and displayed as: the name, or `*` for [`Symbol::All`].
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Name(name) => name,
            Self::All => "*",
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Symbol {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// What an import statement is for.
///
/// Serialized as the lower-case strings `runtime`, `typeonly` and `embed`. The order is the order
/// entries of equal location and target are sorted in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportKind {
    /// An import the program needs at run time. The default, and anything not known to be another.
    #[default]
    Runtime,
    /// An import that only names types and is erased before the program runs: TypeScript
    /// `import type`, an import inside `if TYPE_CHECKING:`, anything in a declaration file.
    TypeOnly,
    /// A file the program embeds as data or source at build time, such as Rust `include_str!`. It
    /// uses no named item of its target.
    Embed,
}

/// One import statement resolved to a repository file.
///
/// Imports reaching the same file from the same location collapse into one `ResolvedImport`
/// whose `symbols` is the union, but only when their `kind` and `test_only` agree. Statements
/// on different lines stay separate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ResolvedImport {
    /// Where the import statement starts.
    pub at: Location,
    /// Repository-relative file it resolves to.
    pub target: PathBuf,
    /// What the import uses from `target`, sorted. Empty for a side-effect import, which uses no
    /// symbol. Serialized as an array of strings, see [`Symbol`].
    pub symbols: BTreeSet<Symbol>,
    /// What the import is for.
    pub kind: ImportKind,
    /// Whether the import is written in test code: a test file, or Rust code under `#[cfg(test)]`.
    pub test_only: bool,
}

impl ResolvedImport {
    /// A runtime import at `at` of `target` that uses no symbol, such as a side-effect import, and
    /// is not test code.
    #[must_use]
    pub const fn new(at: Location, target: PathBuf) -> Self {
        Self {
            at,
            target,
            symbols: BTreeSet::new(),
            kind: ImportKind::Runtime,
            test_only: false,
        }
    }

    /// The same import using `symbols`, in addition to any it already uses.
    #[must_use]
    pub fn with_symbols(mut self, symbols: impl IntoIterator<Item = Symbol>) -> Self {
        self.symbols.extend(symbols);
        self
    }

    /// The same import of the given `kind`.
    #[must_use]
    pub const fn with_kind(mut self, kind: ImportKind) -> Self {
        self.kind = kind;
        self
    }

    /// The same import, written in test code when `test_only`.
    #[must_use]
    pub const fn with_test_only(mut self, test_only: bool) -> Self {
        self.test_only = test_only;
        self
    }
}

/// An import that points at a repository-relative path that could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct UnresolvedImport {
    /// Where the import statement is.
    pub at: Location,
    /// The raw specifier as written.
    pub specifier: String,
}

/// Why a reference was recorded as skipped instead of resolved. Serialized in kebab-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// A TypeScript or JavaScript package that no alias maps and `package.json` does not declare.
    UndeclaredPackage,
    /// A module reference computed at run time, so it names no file: `import(expr)`,
    /// `require(expr)`, Python `importlib.import_module(..)` and `__import__(..)`.
    DynamicSpecifier,
    /// A Rust `include_str!` or `include_bytes!` of a file that is not a known source file.
    EmbedNonSource,
}

/// A reference driftwall saw and could not follow, kept so it is never silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Skipped {
    /// Where the statement that holds the reference starts.
    pub at: Location,
    /// The specifier as written: the module name, the text of a computed argument, or an embed path.
    pub specifier: String,
    /// Why it was skipped.
    pub reason: SkipReason,
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
    /// An import uses a symbol outside the public surface its target node declares.
    PrivateSymbol,
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
    /// Counts of error and warning findings, in that order.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        let errors = self
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count();
        (errors, self.violations.len() - errors)
    }

    /// True when any finding is an [`Severity::Error`], or any finding at all when `strict`.
    #[must_use]
    pub fn failed(&self, strict: bool) -> bool {
        self.violations
            .iter()
            .any(|v| strict || v.severity == Severity::Error)
    }
}

/// How many example import sites an [`EdgeSummary`] keeps.
pub const MAX_SITES: usize = 3;

/// One declared node as the overview presents it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeSummary {
    /// The node identifier.
    pub id: NodeId,
    /// Human readable label, if the diagram gave one.
    pub label: Option<String>,
    /// Directories whose `.arch` file tags this node, sorted. The repository root is `.`.
    pub dirs: Vec<PathBuf>,
    /// Source files this node owns.
    pub files: usize,
    /// The node's declared public surface, or `None` when it declares none and is unrestricted.
    /// Omitted from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<BTreeSet<String>>,
}

/// One edge as the overview presents it: declared, observed in code, or both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdgeSummary {
    /// The importing node.
    pub from: NodeId,
    /// The imported node.
    pub to: NodeId,
    /// The declared kind, or `None` when code uses the edge and the contract does not declare it.
    pub kind: Option<EdgeKind>,
    /// Number of import sites in code that realise the edge.
    pub realised: usize,
    /// The first [`MAX_SITES`] import sites, in sorted order.
    pub sites: Vec<Location>,
    /// Every symbol the imports behind the edge use, over all sites, sorted with `*` last. Empty
    /// when nothing uses the edge or every import is a side-effect import.
    pub symbols: BTreeSet<Symbol>,
}

/// The whole architecture at a glance: every node and every declared or observed edge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Overview {
    /// Declared nodes, sorted by id.
    pub nodes: Vec<NodeSummary>,
    /// Declared and observed edges, sorted by `(from, to)`.
    pub edges: Vec<EdgeSummary>,
    /// Findings of error severity in the lint run behind this overview.
    pub errors: usize,
    /// Findings of warning severity in the lint run behind this overview.
    pub warnings: usize,
}

/// One node with the edges that touch it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeView {
    /// The node itself.
    pub node: NodeSummary,
    /// Edges leaving the node, sorted by target.
    pub outgoing: Vec<EdgeSummary>,
    /// Edges entering the node, sorted by source.
    pub incoming: Vec<EdgeSummary>,
}

/// What changed between two versions of the contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ContractDiff {
    /// Nodes only the new contract declares, sorted.
    pub added_nodes: Vec<NodeId>,
    /// Nodes only the old contract declares, sorted.
    pub removed_nodes: Vec<NodeId>,
    /// Edges only the new contract declares, with their kind, sorted.
    pub added_edges: Vec<(Edge, EdgeKind)>,
    /// Edges only the old contract declares, with their kind, sorted.
    pub removed_edges: Vec<(Edge, EdgeKind)>,
    /// Edges both declare with a different kind, as `(edge, old kind, new kind)`, sorted.
    pub changed_edges: Vec<(Edge, EdgeKind, EdgeKind)>,
    /// Names only the new contract exports, as `(node, name)`, sorted.
    pub added_exports: Vec<(NodeId, String)>,
    /// Names only the old contract exports, as `(node, name)`, sorted. A node that lost its whole
    /// surface lists every name it had: it is unrestricted from then on.
    pub removed_exports: Vec<(NodeId, String)>,
}

impl ContractDiff {
    /// True when the two contracts declare the same nodes, edges with the same kinds, and surfaces.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
            && self.changed_edges.is_empty()
            && self.added_exports.is_empty()
            && self.removed_exports.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_split_errors_from_warnings() {
        let violation = |severity| Violation {
            kind: ViolationKind::StaleEdge,
            severity,
            at: None,
            message: String::new(),
            hint: None,
        };
        let report = LintReport {
            violations: vec![
                violation(Severity::Error),
                violation(Severity::Warning),
                violation(Severity::Warning),
            ],
        };
        assert_eq!(report.counts(), (1, 2));
        assert_eq!(LintReport::default().counts(), (0, 0));
    }

    #[test]
    fn contract_diff_is_empty_only_when_nothing_changed() {
        let id = NodeId::new("a").unwrap();
        let edge = Edge {
            from: id.clone(),
            to: NodeId::new("b").unwrap(),
        };
        assert!(ContractDiff::default().is_empty());
        let cases = [
            ContractDiff {
                added_nodes: vec![id.clone()],
                ..ContractDiff::default()
            },
            ContractDiff {
                removed_nodes: vec![id],
                ..ContractDiff::default()
            },
            ContractDiff {
                added_edges: vec![(edge.clone(), EdgeKind::Planned)],
                ..ContractDiff::default()
            },
            ContractDiff {
                removed_edges: vec![(edge.clone(), EdgeKind::Planned)],
                ..ContractDiff::default()
            },
            ContractDiff {
                changed_edges: vec![(edge, EdgeKind::Planned, EdgeKind::Implemented)],
                ..ContractDiff::default()
            },
            ContractDiff {
                added_exports: vec![(NodeId::new("a").unwrap(), "X".to_owned())],
                ..ContractDiff::default()
            },
            ContractDiff {
                removed_exports: vec![(NodeId::new("a").unwrap(), "X".to_owned())],
                ..ContractDiff::default()
            },
        ];
        for diff in cases {
            assert!(!diff.is_empty(), "{diff:?}");
        }
    }

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
    fn symbols_serialize_as_names_with_a_star_for_all_and_sort_names_first() {
        let import = ResolvedImport::new(
            Location {
                file: PathBuf::from("a.rs"),
                line: 3,
            },
            PathBuf::from("b.rs"),
        )
        .with_symbols([
            Symbol::All,
            Symbol::Name("zeta".to_owned()),
            Symbol::Name("alpha".to_owned()),
        ]);
        let json = serde_json::to_string(&import).unwrap();
        assert_eq!(
            json,
            r#"{"at":{"file":"a.rs","line":3},"target":"b.rs","symbols":["alpha","zeta","*"],"kind":"runtime","test_only":false}"#
        );
    }

    #[test]
    fn an_import_without_symbols_serializes_an_empty_array() {
        let import = ResolvedImport::new(
            Location {
                file: PathBuf::from("a.rs"),
                line: 1,
            },
            PathBuf::from("b.rs"),
        );
        assert!(import.symbols.is_empty());
        let json = serde_json::to_string(&import).unwrap();
        assert!(json.contains(r#""symbols":[]"#), "{json}");
    }

    #[test]
    fn kinds_serialize_as_lower_case_strings() {
        let import = |kind| {
            ResolvedImport::new(
                Location {
                    file: PathBuf::from("a.ts"),
                    line: 1,
                },
                PathBuf::from("b.ts"),
            )
            .with_kind(kind)
            .with_test_only(true)
        };
        let tail = |kind| {
            let json = serde_json::to_string(&import(kind)).unwrap();
            json.split_once(r#""symbols":[],"#).unwrap().1.to_owned()
        };
        assert_eq!(
            tail(ImportKind::Runtime),
            r#""kind":"runtime","test_only":true}"#
        );
        assert_eq!(
            tail(ImportKind::TypeOnly),
            r#""kind":"typeonly","test_only":true}"#
        );
        assert_eq!(
            tail(ImportKind::Embed),
            r#""kind":"embed","test_only":true}"#
        );
    }

    #[test]
    fn skipped_references_serialize_their_reason_in_kebab_case() {
        let json = |reason| {
            serde_json::to_string(&Skipped {
                at: Location {
                    file: PathBuf::from("a.ts"),
                    line: 2,
                },
                specifier: "x".to_owned(),
                reason,
            })
            .unwrap()
        };
        assert_eq!(
            json(SkipReason::UndeclaredPackage),
            r#"{"at":{"file":"a.ts","line":2},"specifier":"x","reason":"undeclared-package"}"#
        );
        assert!(json(SkipReason::DynamicSpecifier).ends_with(r#""reason":"dynamic-specifier"}"#));
        assert!(json(SkipReason::EmbedNonSource).ends_with(r#""reason":"embed-non-source"}"#));
    }

    #[test]
    fn symbols_display_as_their_serialized_text() {
        assert_eq!(Symbol::Name("fetch".to_owned()).to_string(), "fetch");
        assert_eq!(Symbol::All.to_string(), "*");
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
