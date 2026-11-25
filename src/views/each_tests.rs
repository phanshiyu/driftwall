//! Templates in the contract diff.

use super::contract_diff;

#[test]
fn diff_reports_a_template_as_its_line_and_not_its_expansion() {
    let text = |extra: &str| {
        format!(
            "flowchart LR\n  subgraph f\n    subgraph a\n      a-x\n      a-y\n    end\n  end\n{extra}"
        )
    };
    let diff = contract_diff(&text(""), &text("  %% each f: x --> y\n")).unwrap();
    assert_eq!(diff.rules_added, vec!["%% each f: x --> y".to_owned()]);
    assert!(diff.added_edges.is_empty() && diff.added_boundary_edges.is_empty());
    let diff = contract_diff(&text(""), &text("  a-x --> a-y\n")).unwrap();
    assert!(!diff.is_empty());
}

#[test]
fn diff_reports_package_and_deny_templates_as_their_lines() {
    let text = |extra: &str| {
        format!(
            "flowchart LR\n  subgraph f\n    subgraph a\n      a-x\n      a-y\n    end\n  end\n  subgraph g\n    z\n  end\n  %% order open: f > g\n{extra}"
        )
    };
    let diff = contract_diff(
        &text(""),
        &text("  %% each f: forbid-packages x: lit\n  %% each f: deny y: z\n"),
    )
    .unwrap();
    assert_eq!(
        diff.rules_added,
        vec![
            "%% each f: deny y: z".to_owned(),
            "%% each f: forbid-packages x: lit".to_owned()
        ]
    );
    assert_eq!(diff.rules_removed, Vec::<String>::new());
    let changed = contract_diff(
        &text("  %% each f: forbid-packages x: lit\n"),
        &text("  %% each f: forbid-packages x: axum\n"),
    )
    .unwrap();
    assert_eq!(
        changed.rules_removed,
        vec!["%% each f: forbid-packages x: lit".to_owned()]
    );
    assert_eq!(
        changed.rules_added,
        vec!["%% each f: forbid-packages x: axum".to_owned()]
    );
}
