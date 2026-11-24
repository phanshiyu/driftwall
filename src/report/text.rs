//! Plain-text rendering for humans.

use std::fmt::Write as _;

use crate::model::{BaselineOutcome, LintReport};

use super::common::{counts, plural};
use super::group::{Printed, write_blocks};

/// Renders `report` as uncoloured text, one block per violation plus a summary line.
#[must_use]
pub fn render_text(report: &LintReport) -> String {
    text(report, None)
}

/// Like [`render_text`] for a report a baseline was applied to: the summary line says how many
/// findings the baseline suppressed.
#[must_use]
pub fn render_text_baselined(report: &LintReport, baseline: &BaselineOutcome) -> String {
    text(report, Some(baseline.suppressed.len()))
}

fn text(report: &LintReport, baselined: Option<usize>) -> String {
    let mut out = String::new();
    let printed = write_blocks(&mut out, report);
    out.push_str(&summary(report, printed, baselined));
    out.push('\n');
    out
}

fn summary(report: &LintReport, printed: Printed, baselined: Option<usize>) -> String {
    let mut line = if report.violations.is_empty() {
        "driftwall: clean".to_owned()
    } else {
        let (errors, warnings) = counts(report);
        format!(
            "driftwall: {}, {}",
            plural(errors, "error"),
            plural(warnings, "warning")
        )
    };
    // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
    if printed.grouped {
        let _ = write!(line, " ({})", plural(printed.blocks, "group"));
    }
    if let Some(count) = baselined {
        let _ = write!(line, ", {count} baselined");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Severity, ViolationKind};
    use crate::report::fixtures::{all_kinds, mixed, violation};

    #[test]
    fn mixed_report_renders_exact_text() {
        let expected = "\
src/a.rs:3: error[unmapped-file]: file has no owning node
  hint: add a .arch tag
src/nested/b.rs:10: error[undeclared-edge]: lint imports report
warning[stale-edge]: edge cli --> lint has no import
  hint: make the edge dotted
warning[stale-edge]: edge init --> model has no import
driftwall: 2 errors, 2 warnings
";
        assert_eq!(render_text(&mixed()), expected);
    }

    #[test]
    fn a_finding_at_a_test_site_says_so_after_its_kind() {
        let mut found = violation(
            ViolationKind::UndeclaredEdge,
            Severity::Error,
            Some(("src/a.test.ts", 3)),
            "a imports b",
            None,
        );
        found.test_only = true;
        let report = LintReport {
            violations: vec![found],
        };
        let expected = "\
src/a.test.ts:3: error[undeclared-edge] (test code): a imports b
driftwall: 1 error, 0 warnings
";
        assert_eq!(render_text(&report), expected);
    }

    #[test]
    fn a_baseline_adds_its_count_to_the_summary() {
        let outcome = BaselineOutcome {
            file: "debt.txt".into(),
            suppressed: mixed().violations,
            stale: 0,
        };
        let clean = render_text_baselined(&LintReport::default(), &outcome);
        assert_eq!(clean, "driftwall: clean, 4 baselined\n");
        let one = LintReport {
            violations: mixed().violations[..1].to_vec(),
        };
        assert!(
            render_text_baselined(&one, &outcome).ends_with("1 error, 0 warnings, 4 baselined\n")
        );
    }

    #[test]
    fn clean_report_says_clean() {
        assert_eq!(render_text(&LintReport::default()), "driftwall: clean\n");
    }

    #[test]
    fn singular_error_with_plural_warnings() {
        let report = LintReport {
            violations: vec![
                violation(
                    ViolationKind::UnknownNode,
                    Severity::Error,
                    Some(("src/a/.arch", 2)),
                    "unknown node `zzz`",
                    None,
                ),
                violation(
                    ViolationKind::UnresolvedImport,
                    Severity::Warning,
                    Some(("src/a.ts", 1)),
                    "cannot resolve ./x",
                    None,
                ),
                violation(
                    ViolationKind::UnresolvedImport,
                    Severity::Warning,
                    Some(("src/a.ts", 2)),
                    "cannot resolve ./y",
                    None,
                ),
            ],
        };
        let expected = "\
src/a/.arch:2: error[unknown-node]: unknown node `zzz`
src/a.ts:1: warning[unresolved-import]: cannot resolve ./x
src/a.ts:2: warning[unresolved-import]: cannot resolve ./y
driftwall: 1 error, 2 warnings
";
        assert_eq!(render_text(&report), expected);
    }

    #[test]
    fn singular_warning_with_zero_errors() {
        let report = LintReport {
            violations: vec![violation(
                ViolationKind::PlannedRealized,
                Severity::Warning,
                None,
                "promote cli -.-> lint",
                None,
            )],
        };
        let expected = "\
warning[planned-realized]: promote cli -.-> lint
driftwall: 0 errors, 1 warning
";
        assert_eq!(render_text(&report), expected);
    }

    #[test]
    fn every_kind_renders_with_its_kebab_name() {
        let text = render_text(&all_kinds());
        for name in [
            "unmapped-file",
            "unknown-node",
            "undeclared-edge",
            "stale-edge",
            "planned-realized",
            "unresolved-import",
            "private-symbol",
        ] {
            assert!(
                text.contains(&format!("warning[{name}]: message")),
                "{name}"
            );
        }
    }

    #[test]
    fn output_is_deterministic() {
        assert_eq!(render_text(&mixed()), render_text(&mixed()));
    }
}
