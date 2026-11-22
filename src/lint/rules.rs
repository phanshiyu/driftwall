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
        | ViolationKind::UndeclaredEdge => Severity::Error,
        ViolationKind::StaleEdge
        | ViolationKind::PlannedRealized
        | ViolationKind::UnresolvedImport => Severity::Warning,
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
        ViolationKind::PlannedRealized => "change `-.->` to `-->` in the contract",
        ViolationKind::UnresolvedImport => "fix the import path or add the missing file",
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
