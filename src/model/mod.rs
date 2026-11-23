//! Core domain types shared by every other module.
//!
//! Every collection here is ordered (`BTreeMap`, `BTreeSet`, sorted `Vec`) so output is
//! deterministic regardless of filesystem or thread ordering.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::{Serialize, Serializer};
use thiserror::Error;

mod blind;
mod map;

pub use map::{GlobError, MapGlob, MapRule, MapSite, MapTarget, Specificity};

pub use blind::{
    BlindFile, BlindRef, BlindSpots, BlindSummary, BlindTally, UnreadGroup, UnreadTotal,
};

/// Contract path used when none is given, relative to the repository root.
pub const DEFAULT_CONTRACT: &str = "architecture.mmd";

/// Errors raised when constructing model values.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    /// A node identifier did not match `[A-Za-z][A-Za-z0-9_-]*`.
    #[error("invalid node id `{0}`: expected [A-Za-z][A-Za-z0-9_-]*")]
    InvalidNodeId(String),
    /// A node id prefix did not match `[A-Za-z][A-Za-z0-9_-]*-`.
    #[error("invalid node id prefix `{0}`: expected [A-Za-z][A-Za-z0-9_-]*-")]
    InvalidPrefix(String),
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

/// The text put in front of the module names under `node: <prefix>*`, such as `ui-`.
///
/// A prefix is either empty (a plain `node: *`) or matches `[A-Za-z][A-Za-z0-9_-]*-`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Prefix(String);

impl Prefix {
    /// Validates and wraps a non-empty prefix: a valid node id spelling that ends in `-`.
    ///
    /// # Errors
    /// Returns [`ModelError::InvalidPrefix`] when `raw` is empty, does not end in `-`, or uses
    /// disallowed characters.
    pub fn new(raw: &str) -> Result<Self, ModelError> {
        if raw.ends_with('-') && NodeId::new(raw).is_ok() {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ModelError::InvalidPrefix(raw.to_owned()))
        }
    }

    /// The prefix as a string slice, empty for none.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The node id of the module called `name` under this prefix: the prefix followed by the name.
    ///
    /// # Errors
    /// Returns [`ModelError::InvalidNodeId`] when `name` is empty or the result is not a valid id.
    pub fn derive(&self, name: &str) -> Result<NodeId, ModelError> {
        let id = format!("{}{name}", self.0);
        if name.is_empty() {
            return Err(ModelError::InvalidNodeId(id));
        }
        NodeId::new(&id)
    }
}

impl fmt::Display for Prefix {
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
    /// Solid arrow labelled `test` (`-->|test|`): only test-only imports are allowed, and one must
    /// exist, otherwise it is stale.
    Test,
    /// Dotted arrow labelled `test` (`-.->|test|`): test-only imports are allowed, none is needed.
    PlannedTest,
}

impl EdgeKind {
    /// Whether the edge is tolerated without code (dotted).
    #[must_use]
    pub const fn is_planned(self) -> bool {
        matches!(self, Self::Planned | Self::PlannedTest)
    }

    /// The arrow the contract writes the edge with, label included.
    #[must_use]
    pub const fn arrow(self) -> &'static str {
        match self {
            Self::Implemented => "-->",
            Self::Planned => "-.->",
            Self::Test => "-->|test|",
            Self::PlannedTest => "-.->|test|",
        }
    }

    /// Whether the edge allows only test-only imports.
    #[must_use]
    pub const fn is_test(self) -> bool {
        matches!(self, Self::Test | Self::PlannedTest)
    }
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
    /// The `%% map` lines in file order. When any exists, they alone decide ownership.
    #[serde(skip)]
    pub maps: Vec<MapRule>,
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
    /// Svelte components: the `<script>` elements of a `.svelte` file.
    Svelte,
    /// HTML pages that hold a `<script` element: their inline scripts, `src` and module preloads.
    Html,
    /// Swift: `import` declarations, resolved through the targets of `Package.swift`.
    Swift,
    /// Shell scripts (`sh`, `bash`, `zsh`, `dash`): `source` and `.` commands.
    Shell,
}

impl Language {
    /// Whether the language's imports are TypeScript or JavaScript imports, resolved with the
    /// TypeScript and JavaScript rules and governed by `tsconfig.json` and `package.json`.
    #[must_use]
    pub const fn is_ecmascript(self) -> bool {
        matches!(
            self,
            Self::TypeScript | Self::JavaScript | Self::Svelte | Self::Html
        )
    }
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
    /// `node: <prefix>*`: every module directly in the directory is its own node, named by its
    /// position and the prefix (empty for a plain `node: *`).
    Modules(Prefix),
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(id) => id.fmt(f),
            Self::Modules(prefix) => write!(f, "{prefix}{MODULES_MARK}"),
        }
    }
}

/// How far a `.arch` file reaches below its own directory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// The default: the tag covers the directory and every subdirectory, until a nearer `.arch`.
    #[default]
    Subtree,
    /// `scope: here`: the tag covers only the files directly in its directory.
    Here,
}

/// The value of `scope:` that limits a tag to its own directory.
pub const SCOPE_HERE: &str = "here";

/// What one `.arch` file says: the tag and how far it reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirTag {
    /// What the directory is tagged with.
    pub tag: Tag,
    /// How far the tag reaches below the directory.
    pub scope: Scope,
    /// For a `node: *` tag: the names of the non-test modules directly in the directory (see
    /// `ownership::peers_in`), which decide the node of a test file there. Empty for any other tag.
    pub peers: BTreeSet<String>,
}

/// Why the scan left a source file out of the repository's own source. Serialized as the word
/// [`Exclusion::as_str`] gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Exclusion {
    /// The file is below a directory named `vendor` or `third_party`.
    Vendored,
    /// The file name has the form `<name>.min.<ext>`.
    Minified,
    /// The file is below a directory named `dist`, or announces itself as generated in its first
    /// lines.
    Generated,
}

impl Exclusion {
    /// The reason as a lowercase word, for reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vendored => "vendored",
            Self::Minified => "minified",
            Self::Generated => "generated",
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
    /// Tags as (file, line, raw node name), for validating against the contract: `.arch` files,
    /// or the contract itself for the nodes a module-form `%% map` derives.
    pub tags: Vec<(Location, String)>,
    /// What each `.arch` file the scan saw says, by the repository-relative directory it is in
    /// (empty for the root).
    pub tag_dirs: BTreeMap<PathBuf, DirTag>,
    /// With `%% map` lines: the rule and glob that decided every owned file (source, asset or
    /// excluded). Empty when `.arch` files decide.
    pub map_sites: BTreeMap<PathBuf, MapSite>,
    /// With `%% map` lines: every glob that matches no walked file.
    pub stale_maps: Vec<MapSite>,
    /// With `%% map` lines: the `.arch` files that exist and are not read, sorted.
    pub ignored_tags: Vec<PathBuf>,
    /// Source files the scan left out, by reason. They are neither owned nor unmapped.
    pub excluded: BTreeMap<Exclusion, BTreeSet<PathBuf>>,
    /// Owner per excluded file that a tag covers, by the same rules as source files and assets.
    /// An excluded file is still an import target, so an import of it is a dependency on this
    /// node. It stays in [`Ownership::excluded`] and in no other field: one that no tag covers is
    /// in no map here.
    pub excluded_owners: BTreeMap<PathBuf, NodeId>,
    /// Owner per repository-relative asset: every other walked file (`data.json`, `style.css`),
    /// by the same tag rules as source files. Assets are not source, so they are in no other field
    /// and count in no file total.
    pub asset_owners: BTreeMap<PathBuf, NodeId>,
    /// Assets with no inherited tag, or whose module name under `node: *` is not a node id. Never
    /// a finding: they only let an import of them resolve.
    pub asset_unmapped: BTreeSet<PathBuf>,
    /// Walked code files driftwall has no language for, by key: the extension (`swift`, `sh`) or
    /// `shebang` for an extensionless script. Neither owned nor unmapped, and never a finding.
    pub unread: BTreeMap<String, BTreeSet<PathBuf>>,
    /// Entry files that declare nothing and that no other module of a `node: *` directory shares a
    /// name with (see "Folded entry files" in `docs/DESIGN.md`). They are neither owned nor
    /// unmapped, but they are known files, so an import of one resolves.
    pub folded: BTreeSet<PathBuf>,
    /// Source files that only `.driftwallignore` hides. Never owned or unmapped, and never a
    /// finding.
    pub ignored: BTreeSet<PathBuf>,
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
/// Serialized in kebab-case as `runtime`, `type-only`, `embed` and `asset`. The order is the order
/// entries of equal location and target are sorted in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
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
    /// A non-source repository file (`./data.json`, `./style.css`) a TypeScript or JavaScript
    /// import depends on. It uses no named item of its target.
    Asset,
}

/// How an import came to depend on a node without resolving to a file (see "Vite aliases" in
/// `docs/DESIGN.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Via {
    /// A Vite alias whose target is a build output no file exists for, inside a node's directory.
    AliasToBuildOutput,
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
    /// Set when `target` is not a file: the import depends on the node that owns it by another
    /// route. Not set by import extraction; `lint` adds such imports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
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
            via: None,
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
    /// A Python absolute import that no import root resolves, although its top-level name matches
    /// a first-party module or package elsewhere in the repository.
    UnreachableModule,
    /// A root-absolute `src`, `href` or import specifier of an HTML file that finds no file below
    /// the Vite root of the page.
    UnresolvedRootPath,
    /// An import of an asset or excluded file whose owner is not a node the contract declares, so
    /// it can be no edge. Recorded by the linter, not by import extraction.
    UndeclaredOwner,
    /// A specifier that a Vite `resolve.alias` entry matches whose target path has no file in the
    /// repository, typically a build output.
    AliasTargetMissing,
    /// A `new URL('<literal>', import.meta.url)` whose target is a directory, missing or outside
    /// the repository.
    FileReferenceMissing,
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
    /// A sentence of explanation, set for [`SkipReason::UnreachableModule`] (the repository path
    /// the top-level name matches) and [`SkipReason::UnresolvedRootPath`] (the Vite root that was
    /// searched), [`SkipReason::AliasTargetMissing`] (the alias and its target) and
    /// [`SkipReason::UndeclaredOwner`] (the asset or excluded file and its owner). Not serialized when
    /// absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The repository path that was looked for and not found, set for
    /// [`SkipReason::AliasTargetMissing`] (the alias target), so that a report that knows the
    /// owners can name the node whose directory holds it. Not serialized.
    #[serde(skip)]
    pub target: Option<PathBuf>,
    /// Whether the reference is written in test code. Only read for
    /// [`SkipReason::AliasTargetMissing`], to flag the edge it makes. Not serialized.
    #[serde(skip)]
    pub test_only: bool,
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
    /// A solid edge is realised only by test-only imports and should be declared `-->|test|`.
    TestOnlyEdge,
    /// A relative import could not be resolved to a file.
    UnresolvedImport,
    /// An import uses a symbol outside the public surface its target node declares.
    PrivateSymbol,
    /// A `%% map` glob matches no file.
    StaleMap,
    /// The contract maps files itself, and `.arch` files exist that are not read.
    IgnoredTagFiles,
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

/// How many example import sites a text report shows per edge unless it is asked for all of them.
/// The model keeps every site.
pub const MAX_SITES: usize = 3;

/// One import statement behind an observed edge.
///
/// Serialized flat: `file`, `line`, `kind`, `test_only`. Sites sort by location, then kind, then
/// test flag, so an edge lists its sites in the same order on every run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Site {
    /// Where the import statement starts.
    #[serde(flatten)]
    pub at: Location,
    /// What the import is for.
    pub kind: ImportKind,
    /// Whether the import is written in test code.
    pub test_only: bool,
    /// Set for an import that depends on a node without a file behind it, see [`Via`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
    /// Whether the import uses the target module itself: it uses no symbol and is neither an embed
    /// nor an asset. Not serialized: the edge says whether any of its sites does.
    #[serde(skip)]
    pub module: bool,
}

impl Site {
    /// A plain runtime import at `at` that is not test code and uses a named symbol.
    #[must_use]
    pub const fn plain(at: Location) -> Self {
        Self {
            at,
            kind: ImportKind::Runtime,
            test_only: false,
            via: None,
            module: false,
        }
    }

    /// The site of `import`.
    #[must_use]
    pub fn of(import: &ResolvedImport) -> Self {
        let names_nothing = import.symbols.is_empty();
        Self {
            at: import.at.clone(),
            kind: import.kind,
            test_only: import.test_only,
            via: import.via,
            module: names_nothing
                && matches!(import.kind, ImportKind::Runtime | ImportKind::TypeOnly),
        }
    }
}

/// One declared node as the overview presents it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeSummary {
    /// The node identifier.
    pub id: NodeId,
    /// Human readable label, if the diagram gave one.
    pub label: Option<String>,
    /// Where the node's code lives, sorted: the directories of the plain `node:` tags that name it
    /// and the module paths of its files that a `node: *` tag owns (see "Node directories" in
    /// `docs/DESIGN.md`). The repository root is `.`.
    pub dirs: Vec<PathBuf>,
    /// Source files this node owns.
    pub files: usize,
    /// Asset files this node owns (see "Asset files" in `docs/DESIGN.md`); they are no source
    /// files and are not in `files`.
    pub assets: usize,
    /// Excluded files this node owns (see "Excluded files"); neither source files nor assets.
    pub excluded: usize,
    /// The node's declared public surface, or `None` when it declares none and is unrestricted.
    /// Omitted from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<BTreeSet<String>>,
    /// What the node is blind to: its unread, excluded and ignored files and its skipped and
    /// unresolved references.
    pub blind_spots: BlindTally,
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
    /// Every import site that realises the edge, in sorted order.
    pub sites: Vec<Site>,
    /// Every symbol the imports behind the edge use, over all sites, sorted with `*` last. Empty
    /// when nothing uses the edge or every import is a side-effect import.
    pub symbols: BTreeSet<Symbol>,
    /// Whether some import behind the edge uses the target module itself rather than a named
    /// symbol (`use crate::gitlab;`, `from x import sub`, `import './x'`).
    pub module: bool,
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
    /// Everything the analysis could not see, as counts.
    pub blind_spots: BlindSummary,
    /// The blind spots no node owns, as counts.
    pub unmapped: BlindTally,
    /// The blind spots no node owns, each one by name.
    pub unmapped_items: BlindSpots,
    /// Every skipped reference, by reason, sorted: what `brief` lists.
    pub skipped_references: BTreeMap<SkipReason, Vec<BlindRef>>,
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
    /// The source files the node owns, sorted.
    pub files: Vec<PathBuf>,
    /// The asset files the node owns, sorted.
    pub asset_files: Vec<PathBuf>,
    /// The excluded files the node owns, sorted.
    pub excluded_files: Vec<PathBuf>,
    /// Every item the node is blind to.
    pub blind_spots: BlindSpots,
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
    fn a_prefix_ends_in_a_dash_and_is_otherwise_an_id() {
        for good in ["ui-", "a-", "ui-kit-", "a_b-", "A1--"] {
            assert_eq!(Prefix::new(good).unwrap().as_str(), good);
        }
        for bad in [
            "", "-", "ui", "ui_", "9-", "-ui-", "u i-", "u.i-", "*-", "ui-*",
        ] {
            assert_eq!(
                Prefix::new(bad),
                Err(ModelError::InvalidPrefix(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_prefix_derives_the_id_as_prefix_then_name() {
        let ui = Prefix::new("ui-").unwrap();
        assert_eq!(ui.derive("components").unwrap().as_str(), "ui-components");
        assert_eq!(ui.derive("9lives").unwrap().as_str(), "ui-9lives");
        assert_eq!(Prefix::default().derive("a").unwrap().as_str(), "a");
        let none = Prefix::default();
        for (prefix, name) in [(&ui, ""), (&ui, "a.b"), (&none, "9"), (&none, "")] {
            assert!(prefix.derive(name).is_err(), "{prefix}{name}");
        }
    }

    #[test]
    fn tags_display_as_written_in_the_arch_file() {
        let modules = |prefix: &str| Tag::Modules(Prefix::new(prefix).unwrap_or_default());
        assert_eq!(modules("").to_string(), "*");
        assert_eq!(modules("ui-").to_string(), "ui-*");
        assert_eq!(Tag::Node(NodeId::new("core").unwrap()).to_string(), "core");
        assert_eq!(Scope::default(), Scope::Subtree);
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
    fn kinds_serialize_as_kebab_case_strings() {
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
            r#""kind":"type-only","test_only":true}"#
        );
        assert_eq!(
            tail(ImportKind::Embed),
            r#""kind":"embed","test_only":true}"#
        );
        assert_eq!(
            tail(ImportKind::Asset),
            r#""kind":"asset","test_only":true}"#
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
                message: None,
                target: None,
                test_only: false,
            })
            .unwrap()
        };
        assert_eq!(
            json(SkipReason::UndeclaredPackage),
            r#"{"at":{"file":"a.ts","line":2},"specifier":"x","reason":"undeclared-package"}"#
        );
        assert!(json(SkipReason::DynamicSpecifier).ends_with(r#""reason":"dynamic-specifier"}"#));
        assert!(json(SkipReason::EmbedNonSource).ends_with(r#""reason":"embed-non-source"}"#));
        assert!(json(SkipReason::UnreachableModule).ends_with(r#""reason":"unreachable-module"}"#));
        assert!(
            json(SkipReason::UnresolvedRootPath).ends_with(r#""reason":"unresolved-root-path"}"#)
        );
        assert!(json(SkipReason::UndeclaredOwner).ends_with(r#""reason":"undeclared-owner"}"#));
    }

    #[test]
    fn a_skip_message_is_serialized_after_the_reason_when_present() {
        let json = serde_json::to_string(&Skipped {
            at: Location {
                file: PathBuf::from("a.py"),
                line: 1,
            },
            specifier: "x".to_owned(),
            reason: SkipReason::UnreachableModule,
            message: Some("matches s/x.py".to_owned()),
            target: None,
            test_only: false,
        })
        .unwrap();
        assert!(json.ends_with(r#""reason":"unreachable-module","message":"matches s/x.py"}"#));
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
