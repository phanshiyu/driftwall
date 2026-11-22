//! Plain-text rendering for humans.

use std::fmt::Write as _;

use crate::model::LintReport;

use super::common::{counts, kind_str, severity_str, slash_path};

/// Renders `report` as uncoloured text, one block per violation plus a summary line.
#[must_use]
pub fn render_text(report: &LintReport) -> String {
    let mut out = String::new();
    for v in &report.violations {
        // Writing to a String cannot fail.
        if let Some(at) = &v.at {
            let _ = write!(out, "{}:{}: ", slash_path(&at.file), at.line);
        }
        let _ = writeln!(
            out,
            "{}[{}]: {}",
            severity_str(v.severity),
            kind_str(v.kind),
            v.message
        );
        if let Some(hint) = &v.hint {
            let _ = writeln!(out, "  hint: {hint}");
        }
    }
    out.push_str(&summary(report));
    out.push('\n');
    out
}

fn summary(report: &LintReport) -> String {
    if report.violations.is_empty() {
        return "driftwall: clean".to_owned();
    }
    let (errors, warnings) = counts(report);
    format!(
        "driftwall: {}, {}",
        plural(errors, "error"),
        plural(warnings, "warning")
    )
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
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
