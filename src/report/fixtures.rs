//! Shared test data for the renderer tests.

use std::path::PathBuf;

use crate::code::Location;
use crate::results::{LintReport, Severity, Violation, ViolationKind};

pub fn violation(
    kind: ViolationKind,
    severity: Severity,
    at: Option<(&str, u32)>,
    message: &str,
    hint: Option<&str>,
) -> Violation {
    Violation {
        kind,
        severity,
        at: at.map(|(file, line)| Location {
            file: PathBuf::from(file),
            line,
        }),
        message: message.to_owned(),
        hint: hint.map(str::to_owned),
        test_only: false,
        key: String::new(),
        site: crate::results::SiteDetail::default(),
    }
}

/// Two errors and two warnings, mixing located/unlocated and hint/no-hint.
pub fn mixed() -> LintReport {
    LintReport {
        violations: vec![
            violation(
                ViolationKind::UnmappedFile,
                Severity::Error,
                Some(("src/a.rs", 3)),
                "file has no owning node",
                Some("add a .arch tag"),
            ),
            violation(
                ViolationKind::UndeclaredEdge,
                Severity::Error,
                Some(("src/nested/b.rs", 10)),
                "lint imports report",
                None,
            ),
            violation(
                ViolationKind::StaleEdge,
                Severity::Warning,
                None,
                "edge cli --> lint has no import",
                Some("make the edge dotted"),
            ),
            violation(
                ViolationKind::StaleEdge,
                Severity::Warning,
                None,
                "edge init --> model has no import",
                None,
            ),
        ],
    }
}

/// Every kind, in declaration order, so the list can be checked against [`ordinal`].
pub const ALL_KINDS: [ViolationKind; 25] = [
    ViolationKind::UnmappedFile,
    ViolationKind::UnknownNode,
    ViolationKind::UndeclaredEdge,
    ViolationKind::StaleEdge,
    ViolationKind::PlannedRealized,
    ViolationKind::TestOnlyEdge,
    ViolationKind::UnresolvedImport,
    ViolationKind::PrivateSymbol,
    ViolationKind::StaleMap,
    ViolationKind::IgnoredTagFiles,
    ViolationKind::UpwardEdge,
    ViolationKind::OverBudget,
    ViolationKind::ProductionImportsTest,
    ViolationKind::ForbiddenPackage,
    ViolationKind::StalePackage,
    ViolationKind::RedundantEdge,
    ViolationKind::StaleFacade,
    ViolationKind::IsolationBreach,
    ViolationKind::StaleIsolation,
    ViolationKind::StaleBaseline,
    ViolationKind::DeniedEdge,
    ViolationKind::FacadeWithoutExports,
    ViolationKind::BareComment,
    ViolationKind::UndrawableId,
    ViolationKind::UndrawableLabel,
];

/// Position of a kind in [`ALL_KINDS`]. Exhaustive, so a new variant fails to compile here and
/// forces `ALL_KINDS` to be updated.
pub const fn ordinal(kind: ViolationKind) -> usize {
    match kind {
        ViolationKind::UnmappedFile => 0,
        ViolationKind::UnknownNode => 1,
        ViolationKind::UndeclaredEdge => 2,
        ViolationKind::StaleEdge => 3,
        ViolationKind::PlannedRealized => 4,
        ViolationKind::TestOnlyEdge => 5,
        ViolationKind::UnresolvedImport => 6,
        ViolationKind::PrivateSymbol => 7,
        ViolationKind::StaleMap => 8,
        ViolationKind::IgnoredTagFiles => 9,
        ViolationKind::UpwardEdge => 10,
        ViolationKind::OverBudget => 11,
        ViolationKind::ProductionImportsTest => 12,
        ViolationKind::ForbiddenPackage => 13,
        ViolationKind::StalePackage => 14,
        ViolationKind::RedundantEdge => 15,
        ViolationKind::StaleFacade => 16,
        ViolationKind::IsolationBreach => 17,
        ViolationKind::StaleIsolation => 18,
        ViolationKind::StaleBaseline => 19,
        ViolationKind::DeniedEdge => 20,
        ViolationKind::FacadeWithoutExports => 21,
        ViolationKind::BareComment => 22,
        ViolationKind::UndrawableId => 23,
        ViolationKind::UndrawableLabel => 24,
    }
}

/// One located violation per kind.
pub fn all_kinds() -> LintReport {
    LintReport {
        violations: ALL_KINDS
            .iter()
            .map(|&kind| {
                violation(
                    kind,
                    Severity::Warning,
                    Some(("src/x.rs", 1)),
                    "message",
                    Some("hint"),
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_kinds_is_ordered_and_complete() {
        for (index, kind) in ALL_KINDS.iter().enumerate() {
            assert_eq!(ordinal(*kind), index);
        }
    }
}
