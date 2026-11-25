//! SARIF 2.1.0 rendering for code-scanning integrations.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::model::{BaselineOutcome, LintReport, Violation};

use super::ReportError;
use super::common::{kind_str, severity_str, slash_path};

const SCHEMA_URI: &str = "https://json.schemastore.org/sarif-2.1.0.json";

/// Renders `report` as a minimal SARIF 2.1.0 log ending in a newline.
///
/// Rules are listed once each, sorted by id, and only for kinds that appear in the report.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when the log cannot be serialised.
pub fn render_sarif(report: &LintReport) -> Result<String, ReportError> {
    sarif(report, None)
}

/// Like [`render_sarif`] for a report a baseline was applied to: the findings the baseline
/// suppressed are results too, each with `"suppressions": [{"kind": "external"}]`.
///
/// # Errors
/// Returns [`ReportError::Serialize`] when the log cannot be serialised.
pub fn render_sarif_baselined(
    report: &LintReport,
    baseline: &BaselineOutcome,
) -> Result<String, ReportError> {
    sarif(report, Some(baseline))
}

fn sarif(report: &LintReport, baseline: Option<&BaselineOutcome>) -> Result<String, ReportError> {
    let mut all: Vec<(&Violation, bool)> = report.violations.iter().map(|v| (v, false)).collect();
    if let Some(outcome) = baseline {
        all.extend(outcome.suppressed.iter().map(|v| (v, true)));
        all.sort();
    }
    let rule_ids: BTreeSet<&str> = all.iter().map(|(v, _)| kind_str(v.kind)).collect();
    let rules: Vec<Value> = rule_ids.into_iter().map(|id| json!({ "id": id })).collect();
    let results: Vec<Value> = all
        .iter()
        .map(|&(v, suppressed)| result(v, suppressed))
        .collect();

    let log = json!({
        "$schema": SCHEMA_URI,
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": { "name": "driftwall", "rules": rules } },
            "results": results,
        }],
    });
    let mut out = serde_json::to_string_pretty(&log)?;
    out.push('\n');
    Ok(out)
}

fn result(v: &Violation, suppressed: bool) -> Value {
    let text = v.hint.as_ref().map_or_else(
        || v.message.clone(),
        |hint| format!("{}\n{hint}", v.message),
    );
    let mut result = json!({
        "ruleId": kind_str(v.kind),
        "level": severity_str(v.severity),
        "message": { "text": text },
    });
    if let (true, Some(map)) = (suppressed, result.as_object_mut()) {
        map.insert("suppressions".to_owned(), json!([{ "kind": "external" }]));
    }
    if let (true, Some(map)) = (v.test_only, result.as_object_mut()) {
        map.insert("properties".to_owned(), json!({ "testOnly": true }));
    }
    if let (Some(at), Some(map)) = (&v.at, result.as_object_mut()) {
        map.insert(
            "locations".to_owned(),
            json!([{
                "physicalLocation": {
                    "artifactLocation": { "uri": slash_path(&at.file) },
                    "region": { "startLine": at.line },
                }
            }]),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Severity, ViolationKind};
    use crate::report::fixtures::{all_kinds, mixed, violation};

    fn parse(report: &LintReport) -> Value {
        serde_json::from_str(&render_sarif(report).unwrap()).unwrap()
    }

    #[test]
    fn envelope_is_sarif_2_1_0() {
        let value = parse(&mixed());
        assert_eq!(value["version"], json!("2.1.0"));
        assert!(value["$schema"].as_str().unwrap().contains("sarif-2.1.0"));
        assert_eq!(value["runs"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["runs"][0]["tool"]["driver"]["name"],
            json!("driftwall")
        );
    }

    #[test]
    fn suppressed_findings_are_results_marked_external() {
        let outcome = BaselineOutcome {
            file: "debt.txt".into(),
            suppressed: mixed().violations[..1].to_vec(),
            stale: 0,
        };
        let rest = LintReport {
            violations: mixed().violations[1..].to_vec(),
        };
        let value: Value =
            serde_json::from_str(&render_sarif_baselined(&rest, &outcome).unwrap()).unwrap();
        let results = value["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 4);
        let marked = results
            .iter()
            .filter(|r| r.get("suppressions").is_some())
            .count();
        assert_eq!(marked, 1);
        assert_eq!(results[0]["suppressions"], json!([{ "kind": "external" }]));
    }

    #[test]
    fn located_result_has_rule_level_message_and_region() {
        let value = parse(&mixed());
        let first = &value["runs"][0]["results"][0];
        assert_eq!(first["ruleId"], json!("unmapped-file"));
        assert_eq!(first["level"], json!("error"));
        assert_eq!(
            first["message"]["text"],
            json!("file has no owning node\nadd a .arch tag")
        );
        let physical = &first["locations"][0]["physicalLocation"];
        assert_eq!(physical["artifactLocation"]["uri"], json!("src/a.rs"));
        assert_eq!(physical["region"]["startLine"], json!(3));
    }

    #[test]
    fn a_test_site_result_carries_a_property_bag_and_others_do_not() {
        let mut report = mixed();
        report.violations[1].test_only = true;
        let value = parse(&report);
        let results = &value["runs"][0]["results"];
        assert_eq!(results[1]["properties"], json!({ "testOnly": true }));
        assert!(results[0].get("properties").is_none());
    }

    #[test]
    fn message_without_hint_is_just_the_message() {
        let value = parse(&mixed());
        let second = &value["runs"][0]["results"][1];
        assert_eq!(second["message"]["text"], json!("lint imports report"));
        assert_eq!(
            second["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            json!("src/nested/b.rs")
        );
    }

    #[test]
    fn unlocated_result_has_no_locations_key_and_warning_level() {
        let value = parse(&mixed());
        let third = value["runs"][0]["results"][2].as_object().unwrap();
        assert!(!third.contains_key("locations"));
        assert_eq!(third["level"], json!("warning"));
        assert_eq!(third["ruleId"], json!("stale-edge"));
    }

    #[test]
    fn rules_list_each_kind_once_sorted() {
        let report = LintReport {
            violations: vec![
                violation(
                    ViolationKind::UnresolvedImport,
                    Severity::Warning,
                    None,
                    "a",
                    None,
                ),
                violation(ViolationKind::StaleEdge, Severity::Warning, None, "b", None),
                violation(
                    ViolationKind::UnresolvedImport,
                    Severity::Warning,
                    None,
                    "c",
                    None,
                ),
                violation(
                    ViolationKind::UnmappedFile,
                    Severity::Error,
                    None,
                    "d",
                    None,
                ),
            ],
        };
        let value = parse(&report);
        let rules = &value["runs"][0]["tool"]["driver"]["rules"];
        assert_eq!(
            rules,
            &json!([
                { "id": "stale-edge" },
                { "id": "unmapped-file" },
                { "id": "unresolved-import" },
            ])
        );
        assert_eq!(value["runs"][0]["results"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn clean_report_has_empty_rules_and_results() {
        let value = parse(&LintReport::default());
        assert_eq!(value["runs"][0]["tool"]["driver"]["rules"], json!([]));
        assert_eq!(value["runs"][0]["results"], json!([]));
    }

    #[test]
    fn every_kind_renders_as_a_rule_and_a_result() {
        let value = parse(&all_kinds());
        let rules = value["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        let ids: Vec<&str> = rules.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "denied-edge",
                "forbidden-package",
                "ignored-tag-files",
                "isolation-breach",
                "over-budget",
                "planned-realized",
                "private-symbol",
                "production-imports-test",
                "redundant-edge",
                "stale-baseline",
                "stale-edge",
                "stale-facade",
                "stale-isolation",
                "stale-map",
                "stale-package",
                "test-only-edge",
                "undeclared-edge",
                "unknown-node",
                "unmapped-file",
                "unresolved-import",
                "upward-edge",
            ]
        );
        assert_eq!(value["runs"][0]["results"].as_array().unwrap().len(), 21);
    }

    #[test]
    fn output_ends_with_newline_and_is_byte_identical_across_runs() {
        let first = render_sarif(&mixed()).unwrap();
        assert!(first.ends_with("}\n"));
        assert_eq!(first, render_sarif(&mixed()).unwrap());
    }
}
