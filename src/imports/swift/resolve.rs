//! Resolution of Swift imports to the entry file of a module declared in a `Package.swift`.
//!
//! See "Swift resolution" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::{Path, PathBuf};

use super::package::SwiftTarget;
use super::system::is_system;
use crate::imports::syntax::resolution::{Files, Reference, Resolution};
use crate::model::{PackageKind, Symbol};

/// The extension of Swift source files.
const SOURCE_EXTENSION: &str = "swift";
/// The manifest, which is never the entry file of a target.
const MANIFEST_FILE: &str = "Package.swift";
/// The keywords that qualify an `import` with the kind of declaration it names.
const DECLARATION_KEYWORDS: &[&str] = &[
    "struct",
    "class",
    "enum",
    "protocol",
    "func",
    "var",
    "let",
    "typealias",
];

/// The Swift targets of a repository by the module name an `import` writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwiftModules {
    modules: BTreeMap<String, Vec<SwiftTarget>>,
}

impl SwiftModules {
    /// The modules of `targets`.
    #[must_use]
    pub fn new(targets: Vec<SwiftTarget>) -> Self {
        let mut modules: BTreeMap<String, Vec<SwiftTarget>> = BTreeMap::new();
        for target in targets {
            modules
                .entry(target.module.clone())
                .or_default()
                .push(target);
        }
        Self { modules }
    }

    /// The target called `module` that is nearest to `importer`: one whose package contains the
    /// importer before one that does not, then the longer shared leading directory path, then the
    /// package directory first in path order.
    fn nearest(&self, module: &str, importer: &Path) -> Option<&SwiftTarget> {
        let dir = importer.parent().unwrap_or_else(|| Path::new(""));
        self.modules.get(module)?.iter().min_by_key(|target| {
            let shared = shared_components(&target.package, dir);
            let contains = dir.starts_with(&target.package);
            (
                std::cmp::Reverse(contains),
                std::cmp::Reverse(shared),
                target.package.clone(),
                target.dir.clone(),
            )
        })
    }
}

/// How many leading path components `a` and `b` share.
fn shared_components(a: &Path, b: &Path) -> usize {
    a.components()
        .zip(b.components())
        .take_while(|(x, y)| x == y)
        .count()
}

/// The table entry. `reference.specifier` is the module path as written (`X`, `X.Y`).
///
/// The first segment names the module. A module that no target declares is a package use. Otherwise the
/// import resolves to the entry file of the nearest target (see [`SwiftModules::nearest`]), and
/// uses the last segment when the import is qualified with a declaration keyword
/// (`import struct X.Y`).
pub fn resolve(
    reference: &Reference<'_>,
    importer: &Path,
    files: &Files<'_>,
    modules: &SwiftModules,
) -> Vec<Resolution> {
    let written: String = reference
        .specifier
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut segments = written.split('.');
    let Some(module) = segments.next() else {
        return vec![Resolution::Ignored];
    };
    let Some(target) = modules.nearest(module, importer) else {
        // why: a module no target declares is code outside the repository.
        let kind = if is_system(module) {
            PackageKind::Stdlib
        } else {
            PackageKind::ThirdParty
        };
        return vec![Resolution::Package {
            name: module.to_owned(),
            kind,
        }];
    };
    let Some(entry) = entry_file(target, files.known) else {
        return vec![Resolution::Ignored];
    };
    let mut symbols = BTreeSet::new();
    if is_qualified(reference)
        && let Some(item) = segments.next_back().filter(|item| !item.is_empty())
    {
        symbols.insert(Symbol::Name(item.to_owned()));
    }
    vec![Resolution::resolved(entry).using(symbols)]
}

/// Whether the declaration around the specifier is written `import <keyword> X.Y`.
fn is_qualified(reference: &Reference<'_>) -> bool {
    let Some(declaration) = reference.node.parent() else {
        return false;
    };
    let mut cursor = declaration.walk();
    declaration
        .children(&mut cursor)
        .any(|child| !child.is_named() && DECLARATION_KEYWORDS.contains(&child.kind()))
}

/// The entry file of `target` among `known`: `<dir>/<name>.swift`, else `<dir>/<module>.swift`, else
/// the first Swift file below the directory in path order.
fn entry_file(target: &SwiftTarget, known: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    [&target.name, &target.module]
        .into_iter()
        .map(|stem| target.dir.join(format!("{stem}.{SOURCE_EXTENSION}")))
        .find(|candidate| known.contains(candidate))
        .or_else(|| {
            known
                .range::<Path, _>((Bound::Included(target.dir.as_path()), Bound::Unbounded))
                .take_while(|file| file.starts_with(&target.dir))
                .find(|file| {
                    file.extension().is_some_and(|ext| ext == SOURCE_EXTENSION)
                        && file.file_name().is_none_or(|name| name != MANIFEST_FILE)
                })
                .cloned()
        })
}
