//! Import statements as extraction resolves them.

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

use serde::{Serialize, Serializer};

use crate::contract::EdgeKind;

use super::Location;

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

impl EdgeKind {
    /// Whether an import site of `kind`, `test_only` or not, is allowed on the edge: any site on an
    /// unlabelled edge, otherwise a type-only site on a `type` edge or a test-only site on a `test`
    /// edge (either, on `type, test`).
    #[must_use]
    pub fn allows(self, kind: ImportKind, test_only: bool) -> bool {
        !self.is_restricted()
            || (self.is_type() && kind == ImportKind::TypeOnly)
            || (self.is_test() && test_only)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_kinds_allow_the_sites_their_label_names() {
        let sites = [
            (ImportKind::Runtime, false),
            (ImportKind::TypeOnly, false),
            (ImportKind::Runtime, true),
            (ImportKind::TypeOnly, true),
        ];
        let allowed = |kind: EdgeKind| sites.map(|(import, test)| kind.allows(import, test));
        assert_eq!(allowed(EdgeKind::Implemented), [true; 4]);
        assert_eq!(allowed(EdgeKind::Planned), [true; 4]);
        assert_eq!(allowed(EdgeKind::Test), [false, false, true, true]);
        assert_eq!(allowed(EdgeKind::Type), [false, true, false, true]);
        assert_eq!(allowed(EdgeKind::TypeTest), [false, true, true, true]);
        assert_eq!(
            allowed(EdgeKind::PlannedTypeTest),
            [false, true, true, true]
        );
        assert!(!EdgeKind::Type.allows(ImportKind::Embed, false));
        assert!(!EdgeKind::Type.allows(ImportKind::Asset, false));
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
    fn symbols_display_as_their_serialized_text() {
        assert_eq!(Symbol::Name("fetch".to_owned()).to_string(), "fetch");
        assert_eq!(Symbol::All.to_string(), "*");
    }
}
