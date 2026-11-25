//! What a language resolver is handed and what it answers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::model::{PackageKind, SkipReason, Symbol};

/// The repository files every resolver may consult.
#[derive(Debug, Clone, Copy)]
pub struct Files<'a> {
    /// Repository-relative paths of every repository file.
    pub known: &'a BTreeSet<PathBuf>,
    /// Repository-relative paths of every asset: a walked file that is not source. Only
    /// TypeScript and JavaScript resolution reads it, so that an import of one resolves.
    pub assets: &'a BTreeSet<PathBuf>,
    /// The assets no `.arch` tag covers, a subset of `assets`. Only Rust resolution reads it: an
    /// embed of an asset records an edge only when something owns the asset.
    pub unowned: &'a BTreeSet<PathBuf>,
}

/// One query match handed to a language resolver: the module reference as written and, for
/// languages whose statements import several things from one module, the imported names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference<'a> {
    /// The text of the `@specifier` capture. For string-based languages the string contents.
    pub specifier: &'a str,
    /// The text of every `@name` capture of the same match, in source order. Usually empty.
    pub names: &'a [&'a str],
    /// The syntax node of the `@specifier` capture, for resolvers that need its surroundings.
    pub node: tree_sitter::Node<'a>,
    /// The source text `node` was parsed from.
    pub source: &'a str,
}

/// What a language resolver decided about one specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The specifier names this repository file, and the importing code uses `symbols` of it.
    Resolved {
        /// The repository-relative file.
        target: PathBuf,
        /// What is used from `target`. Empty for a side-effect import.
        symbols: BTreeSet<Symbol>,
    },
    /// The specifier points inside the repository but matches no file.
    Unresolved,
    /// The specifier embeds this repository file as data or source, and uses no item of it.
    Embedded {
        /// The repository-relative file.
        target: PathBuf,
    },
    /// The specifier names this repository asset, a file that is not source, and uses no item of
    /// it.
    Asset {
        /// The repository-relative file.
        target: PathBuf,
    },
    /// Not a repository import: an alias with no file behind it, a virtual module, a repository
    /// package. Not a package use either.
    Ignored,
    /// An import of code outside the repository: a declared third party package, a builtin, the
    /// standard library.
    Package {
        /// The package, see [`PackageUse::name`](crate::model::PackageUse::name).
        name: String,
        /// Third party or standard library.
        kind: PackageKind,
    },
    /// An import of a third party package nothing declares, so a package use and also the skipped
    /// reference [`SkipReason::UndeclaredPackage`].
    UndeclaredPackage {
        /// The package, see [`PackageUse::name`](crate::model::PackageUse::name).
        name: String,
        /// The explanation of the skipped reference, when it has one.
        message: Option<String>,
    },
    /// A reference that could not be followed, and is reported as such.
    Skipped(SkipReason),
    /// A specifier that a Vite `resolve.alias` entry matches whose target has no file in the
    /// repository: skipped as [`SkipReason::AliasTargetMissing`], naming the alias and the target.
    AliasTargetMissing {
        /// The alias key as written in the config.
        alias: String,
        /// The repository-relative path the alias points at.
        target: PathBuf,
    },
    /// A reference that could not be followed, and is reported as such with an explanation: a
    /// Python module that no import root reaches ([`SkipReason::UnreachableModule`]) or a
    /// root-absolute path of an HTML file that finds no file ([`SkipReason::UnresolvedRootPath`]).
    SkippedWith {
        /// Why it was skipped.
        reason: SkipReason,
        /// The explanation, naming what the reference was compared with.
        message: String,
    },
}

impl Resolution {
    /// A resolution to `target` that uses no symbol yet.
    #[must_use]
    pub const fn resolved(target: PathBuf) -> Self {
        Self::Resolved {
            target,
            symbols: BTreeSet::new(),
        }
    }

    /// This resolution, with `symbols` as what the importing code uses. Resolutions that did not
    /// resolve are returned unchanged.
    #[must_use]
    pub fn using(self, symbols: BTreeSet<Symbol>) -> Self {
        match self {
            Self::Resolved { target, .. } => Self::Resolved { target, symbols },
            other => other,
        }
    }
}
