//! JSON rendering, the interface for agents and scripts.

use serde::Serialize;

use crate::model::{BlindSummary, LintReport, Violation};

use super::ReportError;
use super::common::counts;

/// Bumped on any breaking change to the JSON shape.
pub(super) const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Document<'a> {
    version: u32,
    summary: Summary,
    violations: &'a [Violation],
    blind_spots: &'a BlindSummary,
}

#[derive(Serialize)]
struct Summary {
    errors: usize,
    warnings: usize,
}

/// Renders `report` as pretty-printed JSON ending in a newline. `blind_spots` is the last key: the
/// counts of what the scan could not see, which change no finding and no exit code.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when a value cannot be serialised, such as a path that is
/// not valid UTF-8.
pub fn render_json(report: &LintReport, blind_spots: &BlindSummary) -> Result<String, ReportError> {
    let (errors, warnings) = counts(report);
    let document = Document {
        version: SCHEMA_VERSION,
        summary: Summary { errors, warnings },
        violations: &report.violations,
        blind_spots,
    };
    let mut out = serde_json::to_string_pretty(&document)?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::fixtures::{all_kinds, mixed};
    use serde_json::{Value, json};

    fn parse(report: &LintReport) -> Value {
        serde_json::from_str(&render_json(report, &BlindSummary::default()).unwrap()).unwrap()
    }

    #[test]
    fn wrapper_has_version_and_summary() {
        let value = parse(&mixed());
        assert_eq!(value["version"], json!(1));
        assert_eq!(value["summary"], json!({ "errors": 2, "warnings": 2 }));
        assert_eq!(value["violations"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn violation_fields_follow_the_model() {
        let value = parse(&mixed());
        assert_eq!(
            value["violations"][0],
            json!({
                "kind": "unmapped-file",
                "severity": "error",
                "at": { "file": "src/a.rs", "line": 3 },
                "message": "file has no owning node",
                "hint": "add a .arch tag",
            })
        );
    }

    #[test]
    fn missing_location_and_hint_serialise_as_null() {
        let value = parse(&mixed());
        let last = &value["violations"][3];
        assert_eq!(last["kind"], json!("stale-edge"));
        assert_eq!(last["severity"], json!("warning"));
        assert!(last["at"].is_null());
        assert!(last["hint"].is_null());
        assert!(last.as_object().unwrap().contains_key("hint"));
    }

    #[test]
    fn clean_report_has_zero_counts_and_no_violations() {
        let value = parse(&LintReport::default());
        assert_eq!(value["summary"], json!({ "errors": 0, "warnings": 0 }));
        assert_eq!(value["violations"], json!([]));
    }

    #[test]
    fn output_ends_with_newline_and_is_byte_identical_across_runs() {
        let first = render_json(&mixed(), &BlindSummary::default()).unwrap();
        let second = render_json(&mixed(), &BlindSummary::default()).unwrap();
        assert!(first.ends_with("}\n"));
        assert_eq!(first, second);
    }

    #[test]
    fn top_level_keys_are_in_stable_order() {
        let out = render_json(&mixed(), &BlindSummary::default()).unwrap();
        let version = out.find("\"version\"").unwrap();
        let summary = out.find("\"summary\"").unwrap();
        let violations = out.find("\"violations\"").unwrap();
        assert!(version < summary && summary < violations);
    }

    #[test]
    fn blind_spots_are_the_last_key_and_carry_the_summary() {
        let blind = BlindSummary {
            ignored: 2,
            ..BlindSummary::default()
        };
        let text = render_json(&mixed(), &blind).unwrap();
        assert!(text.rfind("\"blind_spots\"") > text.rfind("\"violations\""));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["blind_spots"],
            json!({ "unread": {}, "excluded": {}, "ignored": 2, "skipped": {}, "unresolved": 0 })
        );
    }

    #[test]
    fn every_kind_renders() {
        let value = parse(&all_kinds());
        let kinds: Vec<&str> = value["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["kind"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "unmapped-file",
                "unknown-node",
                "undeclared-edge",
                "stale-edge",
                "planned-realized",
                "test-only-edge",
                "unresolved-import",
                "private-symbol",
                "stale-map",
                "ignored-tag-files",
                "upward-edge",
                "over-budget",
                "production-imports-test",
                "forbidden-package",
                "stale-package",
                "redundant-edge",
            ]
        );
    }
}
