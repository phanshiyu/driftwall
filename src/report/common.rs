//! Helpers shared by all three renderers, so they agree on spelling and counting.

use std::path::Path;

use crate::model::{LintReport, Severity, ViolationKind};

/// The kebab-case name of a kind. An exhaustive match so a new variant fails to compile here.
pub(super) const fn kind_str(kind: ViolationKind) -> &'static str {
    match kind {
        ViolationKind::UnmappedFile => "unmapped-file",
        ViolationKind::UnknownNode => "unknown-node",
        ViolationKind::UndeclaredEdge => "undeclared-edge",
        ViolationKind::StaleEdge => "stale-edge",
        ViolationKind::PlannedRealized => "planned-realized",
        ViolationKind::UnresolvedImport => "unresolved-import",
    }
}

/// The kebab-case name of a severity. Also the SARIF `level`, which uses the same words.
pub(super) const fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// Counts of error and warning findings, in that order.
pub(super) fn counts(report: &LintReport) -> (usize, usize) {
    let errors = report
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .count();
    (errors, report.violations.len() - errors)
}

/// A path with `/` separators regardless of platform.
pub(super) fn slash_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '/' {
        raw.into_owned()
    } else {
        raw.replace(std::path::MAIN_SEPARATOR, "/")
    }
}
