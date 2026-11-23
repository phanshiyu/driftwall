//! The `%% packages` and `%% forbid-packages` directives, see "Package rules" in `docs/DESIGN.md`.

use super::*;
use crate::model::{NodeId, PackagePattern, PackageRuleKind};

fn id(raw: &str) -> NodeId {
    NodeId::new(raw).unwrap()
}

/// `kind node@line: name|name` for every rule.
fn rules(src: &str) -> Vec<String> {
    parse(src)
        .unwrap()
        .package_rules
        .iter()
        .map(|rule| {
            let names: Vec<&str> = rule.names.iter().map(PackagePattern::as_str).collect();
            format!(
                "{} {}@{}: {}",
                rule.kind.directive(),
                rule.node,
                rule.line,
                names.join("|")
            )
        })
        .collect()
}

fn error(src: &str) -> DiagramError {
    parse(src).unwrap_err()
}

#[test]
fn both_directives_declare_rules_with_their_lines() {
    let src = "flowchart LR\na\nb\n%% packages a: sqlx, tokio*\n  %%forbid-packages   b :  node:child_process ,lit\n%% packages a: @scope/*\n";
    assert_eq!(
        rules(src),
        [
            "packages a@4: sqlx|tokio*",
            "forbid-packages b@5: node:child_process|lit",
            "packages a@6: @scope/*",
        ]
    );
}

#[test]
fn an_allow_list_may_be_empty_and_a_deny_list_may_not() {
    assert_eq!(
        rules("flowchart LR\na\n%% packages a:\n"),
        ["packages a@3: "]
    );
    assert_eq!(
        rules("flowchart LR\na\n%% packages a:   \n"),
        ["packages a@3: "]
    );
    for bad in ["%% forbid-packages a:", "%% forbid-packages a:  "] {
        assert!(
            matches!(
                error(&format!("flowchart LR\na\n{bad}\n")),
                DiagramError::MalformedPackages { line: 3, .. }
            ),
            "{bad}"
        );
    }
}

#[test]
fn a_node_may_have_several_lines_of_each_kind() {
    let contract = parse(
        "flowchart LR\na\n%% packages a: x\n%% packages a: y\n%% forbid-packages a: z\n%% forbid-packages a: w\n",
    )
    .unwrap();
    assert_eq!(contract.package_rules.len(), 4);
    assert_eq!(contract.package_rules[2].kind, PackageRuleKind::Forbid);
}

#[test]
fn malformed_lines_are_errors_on_their_line() {
    for bad in [
        "%% packages",
        "%% packages a",
        "%% packages : x",
        "%% packages a: x,",
        "%% packages a: ,x",
        "%% packages a: x,,y",
        "%% packages a: x y",
        "%% packages a: x*y",
        "%% packages a: *x",
        "%% packages a: x**",
        "%% packages are listed below",
        "%% forbid-packages",
        "%% forbid-packages a",
        "%% forbid-packages everything is fine",
    ] {
        let err = error(&format!("flowchart LR\na\n{bad}\n"));
        assert!(
            matches!(err, DiagramError::MalformedPackages { line: 3, .. }),
            "{bad}: {err:?}"
        );
        assert_eq!(err.line(), Some(3), "{bad}");
    }
    assert!(matches!(
        error("flowchart LR\na\n%% packages 1a: x\n"),
        DiagramError::InvalidId { line: 3, .. }
    ));
}

#[test]
fn other_words_stay_plain_comments() {
    let src = "flowchart LR\na\n%% packages-tests a: x\n%% package a: x\n%% packagesx a: x\n%% forbid-package a: x\n%% Packages a: x\n";
    assert_eq!(rules(src), Vec::<String>::new());
}

#[test]
fn an_unknown_node_is_reported_on_its_line_after_the_whole_file() {
    assert_eq!(
        error("flowchart LR\na\n%% packages ghost: x\nb\n"),
        DiagramError::UndeclaredPackagesNode {
            line: 3,
            directive: "packages",
            id: id("ghost")
        }
    );
    assert_eq!(
        error("flowchart LR\nsubgraph g\na\nend\n%% forbid-packages g: x\n"),
        DiagramError::UndeclaredPackagesNode {
            line: 5,
            directive: "forbid-packages",
            id: id("g")
        }
    );
}

#[test]
fn a_name_listed_twice_for_a_node_is_an_error_naming_the_first_line() {
    let err = error("flowchart LR\na\n%% packages a: x\n%% packages a: y, x\n");
    let DiagramError::MalformedPackages { line, reason, .. } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(*line, 4);
    assert!(
        reason.contains("`x` for `a` is already listed on line 3"),
        "{reason}"
    );
    assert!(matches!(
        error("flowchart LR\na\n%% forbid-packages a: x, x\n"),
        DiagramError::MalformedPackages { line: 3, .. }
    ));
}

#[test]
fn the_same_name_in_both_lists_of_a_node_contradicts_but_other_nodes_may_repeat_it() {
    let err = error("flowchart LR\na\n%% packages a: lit\n%% forbid-packages a: lit\n");
    let DiagramError::MalformedPackages { line, reason, .. } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(*line, 4);
    assert!(reason.contains("both allowed and forbidden"), "{reason}");
    assert_eq!(
        rules("flowchart LR\na\nb\n%% packages a: lit\n%% forbid-packages b: lit\n").len(),
        2
    );
    // why: overlapping patterns are no contradiction, the deny-list carves an exception out.
    assert_eq!(
        rules("flowchart LR\na\n%% packages a: lit*\n%% forbid-packages a: lit-ssr\n").len(),
        2
    );
}

#[test]
fn rules_survive_a_render_round_trip_in_order() {
    let src = "flowchart LR\na\nb\n%% packages b:\n%% packages a: sqlx, tokio*\n%% forbid-packages a: node:fs\n";
    let contract = parse(src).unwrap();
    let rendered = render(&contract);
    assert!(rendered.contains("  %% packages b:\n"), "{rendered}");
    assert!(
        rendered.contains("  %% packages a: sqlx, tokio*\n"),
        "{rendered}"
    );
    assert_eq!(parse(&rendered).as_ref(), Ok(&contract));
    assert_eq!(render(&parse(&rendered).unwrap()), rendered);
}
