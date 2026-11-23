//! Helpers shared by all three renderers, so they agree on spelling and counting.

use std::path::Path;

use crate::model::{EdgeKind, LintReport, Location, Severity, SkipReason, ViolationKind};

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

/// The kebab-case name of a skip reason, as the model serialises it. Exhaustive like [`kind_str`].
pub(super) const fn skip_reason_str(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::UndeclaredPackage => "undeclared-package",
        SkipReason::DynamicSpecifier => "dynamic-specifier",
        SkipReason::EmbedNonSource => "embed-non-source",
        SkipReason::UnreachableModule => "unreachable-module",
        SkipReason::UnresolvedRootPath => "unresolved-root-path",
        SkipReason::UndeclaredOwner => "undeclared-owner",
        SkipReason::AliasTargetMissing => "alias-target-missing",
        SkipReason::FileReferenceMissing => "file-reference-missing",
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

/// How many names a one-line list shows before it says `+N more`.
pub(super) const MAX_LISTED: usize = 6;

/// `a, b, c`, or the first [`MAX_LISTED`] names followed by ` +N more` when there are more.
pub(super) fn capped<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> String {
    let total = names.len();
    let shown: Vec<&str> = names.take(MAX_LISTED).collect();
    let listed = shown.join(", ");
    if total > MAX_LISTED {
        format!("{listed} +{} more", total - MAX_LISTED)
    } else {
        listed
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
    use super::{
        arrow, capped, edge_kind_str, kind_str, plural, severity_str, site_str, skip_reason_str,
    };
    use crate::model::{EdgeKind, Location, Severity, SkipReason};

    #[test]
    fn edge_kind_names_match_the_models_serde_names() {
        for kind in [EdgeKind::Implemented, EdgeKind::Planned] {
            let serde_name = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_name, format!("\"{}\"", edge_kind_str(kind)));
        }
    }

    #[test]
    fn skip_reason_names_match_the_models_serde_names() {
        for reason in [
            SkipReason::UndeclaredPackage,
            SkipReason::DynamicSpecifier,
            SkipReason::EmbedNonSource,
            SkipReason::UnreachableModule,
            SkipReason::UnresolvedRootPath,
            SkipReason::UndeclaredOwner,
            SkipReason::AliasTargetMissing,
            SkipReason::FileReferenceMissing,
        ] {
            let serde_name = serde_json::to_string(&reason).unwrap();
            assert_eq!(serde_name, format!("\"{}\"", skip_reason_str(reason)));
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
    fn capped_lists_show_six_names_then_count_the_rest() {
        let list = |n: usize| {
            let names: Vec<String> = (0..n).map(|i| format!("n{i}")).collect();
            capped(names.iter().map(String::as_str))
        };
        assert_eq!(list(0), "");
        assert_eq!(list(1), "n0");
        assert_eq!(list(6), "n0, n1, n2, n3, n4, n5");
        assert_eq!(list(7), "n0, n1, n2, n3, n4, n5 +1 more");
        assert_eq!(list(10), "n0, n1, n2, n3, n4, n5 +4 more");
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
