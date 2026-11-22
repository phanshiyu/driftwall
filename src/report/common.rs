//! Helpers shared by all three renderers, so they agree on spelling and counting.

use std::path::Path;

use crate::model::{EdgeKind, LintReport, Location, Severity, ViolationKind};

/// The kebab-case name of a kind. An exhaustive match so a new variant fails to compile here.
pub(super) const fn kind_str(kind: ViolationKind) -> &'static str {
    match kind {
        ViolationKind::UnmappedFile => "unmapped-file",
        ViolationKind::UnknownNode => "unknown-node",
        ViolationKind::UndeclaredEdge => "undeclared-edge",
        ViolationKind::StaleEdge => "stale-edge",
        ViolationKind::PlannedRealized => "planned-realized",
        ViolationKind::UnresolvedImport => "unresolved-import",
        ViolationKind::PrivateSymbol => "private-symbol",
    }
}

/// The kebab-case name of a severity. Also the SARIF `level`, which uses the same words.
pub(super) const fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// The kebab-case name of an edge kind, as the model serialises it.
pub(super) const fn edge_kind_str(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Implemented => "implemented",
        EdgeKind::Planned => "planned",
    }
}

/// The contract arrow of an edge kind. An edge the contract does not declare (`None`) is drawn
/// solid, because code does use it.
pub(super) const fn arrow(kind: Option<EdgeKind>) -> &'static str {
    match kind {
        Some(EdgeKind::Planned) => "-.->",
        Some(EdgeKind::Implemented) | None => "-->",
    }
}

/// `1 import`, `2 imports`: `noun` with an `s` unless `n` is one.
pub(super) fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// `file:line` with `/` separators.
pub(super) fn site_str(at: &Location) -> String {
    format!("{}:{}", slash_path(&at.file), at.line)
}

/// Counts of error and warning findings, in that order.
pub(super) fn counts(report: &LintReport) -> (usize, usize) {
    report.counts()
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

#[cfg(test)]
mod tests {
    use super::{arrow, edge_kind_str, kind_str, plural, severity_str, site_str};
    use crate::model::{EdgeKind, Location, Severity};

    #[test]
    fn edge_kind_names_match_the_models_serde_names() {
        for kind in [EdgeKind::Implemented, EdgeKind::Planned] {
            let serde_name = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_name, format!("\"{}\"", edge_kind_str(kind)));
        }
    }

    #[test]
    fn arrows_follow_the_contract_syntax() {
        assert_eq!(arrow(Some(EdgeKind::Implemented)), "-->");
        assert_eq!(arrow(Some(EdgeKind::Planned)), "-.->");
        assert_eq!(arrow(None), "-->");
    }

    #[test]
    fn plural_only_leaves_one_alone() {
        assert_eq!(plural(0, "import"), "0 imports");
        assert_eq!(plural(1, "import"), "1 import");
        assert_eq!(plural(2, "import"), "2 imports");
    }

    #[test]
    fn sites_read_file_colon_line() {
        let at = Location {
            file: "src/a.ts".into(),
            line: 7,
        };
        assert_eq!(site_str(&at), "src/a.ts:7");
    }
    use crate::report::fixtures::ALL_KINDS;

    #[test]
    fn labels_match_the_models_serde_names() {
        for kind in ALL_KINDS {
            let serde_name = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_name, format!("\"{}\"", kind_str(kind)));
        }
        for severity in [Severity::Error, Severity::Warning] {
            let serde_name = serde_json::to_string(&severity).unwrap();
            assert_eq!(serde_name, format!("\"{}\"", severity_str(severity)));
        }
    }
}
