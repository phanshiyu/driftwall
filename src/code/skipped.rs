//! References the scan saw and could not follow.

use std::path::PathBuf;

use serde::Serialize;

use super::Location;

/// Why a reference was recorded as skipped instead of resolved. Serialized in kebab-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// A TypeScript or JavaScript package that no alias maps and `package.json` does not declare.
    UndeclaredPackage,
    /// A module reference computed at run time, so it names no file: `import(expr)`,
    /// `require(expr)`, Python `importlib.import_module(..)` and `__import__(..)`. Also the
    /// JavaScript forms that load code in a way driftwall does not follow, with a message saying
    /// which: `import.meta.glob(..)`, `require.resolve(..)`, `createRequire(..)(..)` and a
    /// variable set to `require` or `createRequire(..)`.
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
    /// A bare specifier that names a workspace package, or a dependency linked to a directory or
    /// to a workspace package (`file:`, `link:`, `workspace:`), and finds no source file: its entry
    /// is a build output the package's tsconfig does not map to sources, or the link leads to no
    /// package of the repository.
    WorkspaceEntryMissing,
    /// A source file whose syntax tree has an error or a missing node, such as a conflict marker or
    /// an unterminated string: the imports after the error may be missing. One per file, at the
    /// first error. Recorded by import extraction.
    Unparsed,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
