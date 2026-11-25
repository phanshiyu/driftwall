//! What was read from the repository: imports, packages, skipped references, owners and tags.
//!
//! This model may use [`crate::contract`] (for node ids) and nothing else in the crate.

mod imports;
mod located;
mod ownership;
mod package;
mod skipped;

pub use imports::{ImportKind, ResolvedImport, Symbol, UnresolvedImport, Via};
pub use located::{FileKind, Located, Owner, Rule};
pub use ownership::{DirTag, Exclusion, MODULES_MARK, MapSite, Ownership, SCOPE_HERE, Scope, Tag};
pub use package::{PackageKind, PackageUse};
pub use skipped::{SkipReason, Skipped};

use std::path::PathBuf;

use serde::Serialize;

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
