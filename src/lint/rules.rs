//! The rule table: severity and hint per [`ViolationKind`].
//!
//! Every function here matches exhaustively on the kind with no wildcard arm, so adding a kind
//! to `results` fails to compile until it has a severity and a hint.

use crate::code::{Location, ResolvedImport};
use crate::results::{Severity, Violation, ViolationKind};

/// How serious a finding of `kind` is.
pub(super) const fn severity(kind: ViolationKind) -> Severity {
    match kind {
        ViolationKind::UnmappedFile
        | ViolationKind::UnknownNode
        | ViolationKind::UndeclaredEdge
        | ViolationKind::DeniedEdge
        | ViolationKind::PrivateSymbol
        | ViolationKind::UpwardEdge
        | ViolationKind::IsolationBreach
        | ViolationKind::ProductionImportsTest
        | ViolationKind::ForbiddenPackage => Severity::Error,
        ViolationKind::StaleEdge
        | ViolationKind::PlannedRealized
        | ViolationKind::TestOnlyEdge
        | ViolationKind::RedundantEdge
        | ViolationKind::UnresolvedImport
        | ViolationKind::StaleMap
        | ViolationKind::IgnoredTagFiles
        | ViolationKind::OverBudget
        | ViolationKind::StalePackage
        | ViolationKind::StaleIsolation
        | ViolationKind::StaleBaseline
        | ViolationKind::StaleFacade
        | ViolationKind::FacadeWithoutExports
        | ViolationKind::BareComment
        | ViolationKind::UndrawableId
        | ViolationKind::UndrawableLabel => Severity::Warning,
    }
}

/// The suggested action for a finding of `kind`.
pub(super) const fn hint(kind: ViolationKind) -> &'static str {
    match kind {
        ViolationKind::UnmappedFile => {
            "add a `.arch` tag in this directory or an ignore rule in `.driftwallignore`"
        }
        ViolationKind::UnknownNode => {
            "fix the tag to name a declared node; declare a new node in the contract only if the user approved it"
        }
        ViolationKind::UndeclaredEdge => {
            "remove the import, or ask the user to approve adding the edge to the contract"
        }
        ViolationKind::DeniedEdge => {
            "remove the import, or ask the user to approve removing the denial"
        }
        ViolationKind::StaleEdge => {
            "remove the edge from the contract, or restore the dependency in code"
        }
        ViolationKind::PlannedRealized => {
            "change `-.->` to `-->` in the contract (`-.->|test|` to `-->|test|`)"
        }
        ViolationKind::TestOnlyEdge => "declare the edge `-->|test|` in the contract",
        ViolationKind::RedundantEdge => {
            "remove the node edge from the contract: the boundary edge already allows it"
        }
        ViolationKind::UnresolvedImport => "fix the import path or add the missing file",
        ViolationKind::StaleMap => {
            "fix the glob, or remove the `%% map` entry if the code it described is gone"
        }
        ViolationKind::IgnoredTagFiles => {
            "delete the `.arch` files: the contract's `%% map` lines decide ownership"
        }
        ViolationKind::ProductionImportsTest => {
            "move what is shared out of the test file into production code, or remove the import; the `%% tests` line is the user's to change"
        }
        ViolationKind::OverBudget => {
            "reduce the node (for `interface`, narrow it behind fewer entry points), or ask the user to raise its `%% budget` line in the contract"
        }
        ViolationKind::UpwardEdge => {
            "remove the import, or ask the user to approve changing the `%% order` line"
        }
        ViolationKind::IsolationBreach => {
            "remove the import, or move the shared code to a node outside the isolated boundary, or ask the user to approve changing the `%% isolated` line"
        }
        ViolationKind::StaleIsolation => {
            "remove the boundary from the `%% isolated` line, or give it a second child"
        }
        ViolationKind::ForbiddenPackage => {
            "remove the import, or ask the user to approve changing the node's `%% packages` or `%% forbid-packages` line in the contract"
        }
        ViolationKind::StalePackage => {
            "remove the entry from the `%% packages` line, or restore the use in code"
        }
        ViolationKind::StaleFacade => {
            "remove the node from the `%% facade` line, or restore the re-exports in code"
        }
        ViolationKind::FacadeWithoutExports => {
            "list the names other nodes may use with `%% exports <node>: ...`, or ask the user to approve dropping the facade"
        }
        ViolationKind::BareComment => {
            "replace the bare `%%` with a blank line or `%% <text>`, so Mermaid can draw the contract"
        }
        ViolationKind::UndrawableId => {
            "rename the node or boundary in the contract and in every `%% map`, `%% exports` and other directive that names it; `driftwall init` never writes such an id"
        }
        ViolationKind::UndrawableLabel => {
            "fix the label (no brackets, braces, parentheses or stray quotes in an edge label; no empty label) or the `%%{` comment, so Mermaid can draw the contract"
        }
        ViolationKind::StaleBaseline => {
            "run `driftwall lint --write-baseline <file>` to shrink the baseline"
        }
        ViolationKind::PrivateSymbol => {
            "import a name the node exports, or ask the user to approve extending its `%% exports` line in the contract"
        }
    }
}

/// Builds a finding of `kind` with the table's severity and hint.
pub(super) fn violation(kind: ViolationKind, at: Option<Location>, message: String) -> Violation {
    Violation {
        kind,
        severity: severity(kind),
        at,
        message,
        hint: Some(hint(kind).to_owned()),
        test_only: false,
        key: String::new(),
        site: crate::results::SiteDetail::default(),
    }
}

/// Builds a finding of `kind` at the import site `import`, marked when the site is test code.
pub(super) fn site_violation(
    kind: ViolationKind,
    import: &ResolvedImport,
    message: String,
) -> Violation {
    Violation {
        test_only: import.test_only,
        ..violation(kind, Some(import.at.clone()), message).targeting(&import.target)
    }
}
