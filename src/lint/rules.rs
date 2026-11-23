//! The rule table: severity and hint per [`ViolationKind`].
//!
//! Every function here matches exhaustively on the kind with no wildcard arm, so adding a kind
//! to the model fails to compile until it has a severity and a hint.

use crate::model::{Location, Severity, Violation, ViolationKind};

/// How serious a finding of `kind` is.
pub(super) const fn severity(kind: ViolationKind) -> Severity {
    match kind {
        ViolationKind::UnmappedFile
        | ViolationKind::UnknownNode
        | ViolationKind::UndeclaredEdge
        | ViolationKind::PrivateSymbol
        | ViolationKind::UpwardEdge => Severity::Error,
        ViolationKind::StaleEdge
        | ViolationKind::PlannedRealized
        | ViolationKind::TestOnlyEdge
        | ViolationKind::UnresolvedImport
        | ViolationKind::StaleMap
        | ViolationKind::IgnoredTagFiles
        | ViolationKind::OverBudget => Severity::Warning,
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
        ViolationKind::StaleEdge => {
            "remove the edge from the contract, or restore the dependency in code"
        }
        ViolationKind::PlannedRealized => {
            "change `-.->` to `-->` in the contract (`-.->|test|` to `-->|test|`)"
        }
        ViolationKind::TestOnlyEdge => "declare the edge `-->|test|` in the contract",
        ViolationKind::UnresolvedImport => "fix the import path or add the missing file",
        ViolationKind::StaleMap => {
            "fix the glob, or remove the `%% map` entry if the code it described is gone"
        }
        ViolationKind::IgnoredTagFiles => {
            "delete the `.arch` files: the contract's `%% map` lines decide ownership"
        }
        ViolationKind::OverBudget => {
            "reduce the node, or ask the user to raise its `%% budget` line in the contract"
        }
        ViolationKind::UpwardEdge => {
            "remove the import or move the code to a node in the same or a lower layer; the `%% order` line is the user's to change"
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
    }
}
