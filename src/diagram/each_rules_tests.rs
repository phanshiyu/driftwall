//! `%% each` with package rules and denials: parsing, expansion per child, the contract errors and
//! the round trip of "Edge templates".

use super::*;
use crate::contract::{Contract, Denial, EachBody, PackagePattern, PackageRuleKind};

/// `billing` has a controller, a service and a view; `search` has no view; `app-shell` is above
/// `features` and `request` and `ui` below it in an open chain.
const FIXTURE: &str = "flowchart LR
  subgraph app
    app-shell
  end
  subgraph features
    subgraph billing
      billing-controller
      billing-service
      billing-view
    end
    subgraph search
      search-controller
      search-service
    end
  end
  subgraph shared
    request
    ui
  end
  %% order open: app > features > shared
";

fn with(extra: &str) -> String {
    format!("{FIXTURE}{extra}")
}

fn error(extra: &str) -> DiagramError {
    parse(&with(extra)).unwrap_err()
}

fn reason(error: &DiagramError) -> String {
    match error {
        DiagramError::MalformedRule { reason, .. }
        | DiagramError::MalformedPackages { reason, .. } => reason.clone(),
        other => panic!("{other:?}"),
    }
}

/// `kind node@line: name|name` of the expanded package rules.
fn expanded(extra: &str) -> Vec<String> {
    parse(&with(extra))
        .unwrap()
        .package_rules
        .iter()
        .filter(|rule| rule.template.is_some())
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

/// `from -> to@line` of every denial, expansions included.
fn denials(extra: &str) -> Vec<String> {
    parse(&with(extra))
        .unwrap()
        .rules
        .deny
        .iter()
        .map(|denial| format!("{} -> {}@{}", denial.from, denial.to, denial.line))
        .collect()
}

const EACH_LINE: u32 = 21;

#[test]
fn packages_expand_for_the_node_of_the_role_in_every_child() {
    assert_eq!(
        expanded("  %% each features: packages service: sqlx, tokio*\n"),
        [
            format!("packages billing-service@{EACH_LINE}: sqlx|tokio*"),
            format!("packages search-service@{EACH_LINE}: sqlx|tokio*"),
        ]
    );
}

#[test]
fn forbid_packages_expand_and_a_child_without_the_role_is_skipped() {
    assert_eq!(
        expanded("  %% each features: forbid-packages view: lit, node:*\n"),
        [format!(
            "forbid-packages billing-view@{EACH_LINE}: lit|node:*"
        )]
    );
}

#[test]
fn an_allow_list_may_be_empty_and_a_deny_list_may_not() {
    assert_eq!(
        expanded("  %% each features: packages view:\n"),
        [format!("packages billing-view@{EACH_LINE}: ")]
    );
    let message = reason(&error("  %% each features: forbid-packages view:\n"));
    assert!(message.contains("needs at least one name"), "{message}");
}

#[test]
fn a_package_role_that_resolves_in_no_child_is_an_error_at_the_line() {
    let err = error("  %% each features: packages servce: sqlx\n");
    assert_eq!(err.line(), Some(EACH_LINE));
    assert!(reason(&err).contains("role `servce` resolves in no child of `features`"));
}

#[test]
fn package_names_add_up_with_written_lines_for_the_same_node() {
    let contract = parse(&with(
        "  %% packages billing-service: serde\n  %% each features: packages service: sqlx\n",
    ))
    .unwrap();
    let names: Vec<String> = contract
        .package_rules
        .iter()
        .filter(|rule| rule.node.as_str() == "billing-service")
        .flat_map(|rule| rule.names.iter().map(ToString::to_string))
        .collect();
    assert_eq!(names, ["serde", "sqlx"]);
}

#[test]
fn a_name_listed_by_a_written_line_and_a_template_is_the_duplicate_error() {
    for extra in [
        "  %% forbid-packages billing-view: lit\n  %% each features: forbid-packages view: lit\n",
        "  %% each features: forbid-packages view: lit\n  %% forbid-packages billing-view: lit\n",
    ] {
        let err = error(extra);
        let message = reason(&err);
        assert!(
            message.contains("`lit` for `billing-view` is already listed on line"),
            "{message}"
        );
    }
    let message = reason(&error(
        "  %% packages billing-view: lit\n  %% each features: forbid-packages view: lit\n",
    ));
    assert!(message.contains("both allowed and forbidden"), "{message}");
}

#[test]
fn two_templates_expanding_the_same_name_for_a_node_are_a_duplicate() {
    let err = error(
        "  %% each features: packages service: sqlx\n  %% each features: packages service: sqlx\n",
    );
    assert_eq!(err.line(), Some(EACH_LINE + 1));
    assert!(reason(&err).contains("already listed on line"));
}

#[test]
fn a_name_twice_in_one_template_line_is_an_error() {
    let message = reason(&error("  %% each features: packages service: sqlx, sqlx\n"));
    assert!(message.contains("`sqlx` is listed twice"), "{message}");
}

#[test]
fn deny_with_a_fixed_end_expands_for_each_child_with_the_role() {
    assert_eq!(
        denials("  %% each features: deny view: request\n"),
        [format!("billing-view -> request@{EACH_LINE}")]
    );
    assert_eq!(
        denials("  %% each features: deny controller: request, ui\n"),
        [
            format!("billing-controller -> request@{EACH_LINE}"),
            format!("search-controller -> request@{EACH_LINE}"),
            format!("billing-controller -> ui@{EACH_LINE}"),
            format!("search-controller -> ui@{EACH_LINE}"),
        ]
    );
}

#[test]
fn deny_with_a_role_end_on_the_imported_side_takes_a_fixed_importer() {
    assert_eq!(
        denials("  %% each features: deny app-shell: view\n"),
        [format!("app-shell -> billing-view@{EACH_LINE}")]
    );
}

#[test]
fn an_expanded_denial_is_named_by_its_template_line() {
    let contract = parse(&with("  %% each features: deny view: request\n")).unwrap();
    assert_eq!(
        contract.rules.deny[0].text(),
        "%% each features: deny view: request"
    );
}

#[test]
fn a_deny_end_that_is_neither_a_role_nor_declared_is_an_error() {
    let err = error("  %% each features: deny view: nothing\n");
    assert_eq!(err.line(), Some(EACH_LINE));
    assert!(reason(&err).contains("role `nothing` resolves in no child of `features`"));
}

#[test]
fn a_deny_with_two_fixed_ends_is_an_error() {
    let message = reason(&error("  %% each features: deny app-shell: request\n"));
    assert!(message.contains("joins two declared ids"), "{message}");
    assert!(message.contains("an ordinary `%% deny`"), "{message}");
}

#[test]
fn a_denied_pair_listed_twice_across_a_written_line_and_a_template_is_an_error() {
    let err = error("  %% deny billing-view: request\n  %% each features: deny view: request\n");
    assert_eq!(err.line(), Some(EACH_LINE + 1));
    let message = reason(&err);
    assert!(
        message.contains("`billing-view` to `request` is already denied on line 21"),
        "{message}"
    );
    assert!(message.contains("expanded by"), "{message}");
}

#[test]
fn an_expanded_denial_that_contradicts_a_declared_edge_names_the_child() {
    let err = error("  billing-view --> request\n  %% each features: deny view: request\n");
    assert_eq!(err.line(), Some(EACH_LINE + 1));
    let message = reason(&err);
    assert!(message.contains("billing-view --> request"), "{message}");
    assert!(
        message.contains("(expanded by `%% each features: deny view: request` for `billing`)"),
        "{message}"
    );
}

#[test]
fn an_expanded_denial_that_removes_nothing_is_a_dead_rule_naming_the_child() {
    // why: two nodes of one layer have no open allowance between them, so there is nothing to deny.
    let err = error("  %% each features: deny controller: view\n");
    assert_eq!(err.line(), Some(EACH_LINE));
    let message = reason(&err);
    assert!(
        message.contains("no open chain lets `billing-controller` use `billing-view`"),
        "{message}"
    );
    assert!(message.contains("for `billing`"), "{message}");
}

#[test]
fn a_role_named_deny_in_the_arrow_form_still_parses_as_arrows() {
    let src = "flowchart LR
  subgraph features
    subgraph billing
      billing-deny
      billing-packages
      billing-service
    end
  end
  %% each features: deny --> service, packages -.-> service
";
    let contract = parse(src).unwrap();
    assert!(matches!(contract.rules.each[0].body, EachBody::Arrows(_)));
    assert_eq!(contract.boundary_edges.len(), 2);
    assert_eq!(contract.boundary_edges[0].from.to_string(), "billing-deny");
    assert_eq!(contract.package_rules.len(), 0);
    assert_eq!(contract.rules.deny.len(), 0);
}

#[test]
fn a_role_called_packages_works_with_the_rule_form_too() {
    let src = "flowchart LR
  subgraph features
    subgraph billing
      billing-packages
    end
  end
  %% each features: forbid-packages packages: lit
";
    let contract = parse(src).unwrap();
    assert_eq!(contract.package_rules.len(), 1);
    assert_eq!(contract.package_rules[0].kind, PackageRuleKind::Forbid);
}

#[test]
fn a_malformed_rule_form_is_an_error_at_its_line() {
    for extra in [
        "  %% each features: deny view:\n",
        "  %% each features: deny view: view\n",
        "  %% each features: deny view: request, request\n",
        "  %% each features: packages service: a b\n",
        "  %% each features: packages service: a*b\n",
        "  %% each features: packages service: sqlx,\n",
    ] {
        let err = error(extra);
        assert_eq!(err.line(), Some(EACH_LINE), "{extra}");
    }
}

#[test]
fn template_lines_render_back_and_expansions_do_not() {
    let src = with(
        "  %% packages billing-service: serde\n  %% deny app-shell: ui\n  %% each features: packages service: sqlx\n  %% each features: forbid-packages view: lit, node:*\n  %% each features: deny view: request\n  %% each features: controller --> service\n",
    );
    let contract = parse(&src).unwrap();
    let rendered = render(&contract);
    for line in [
        "  %% each features: packages service: sqlx\n",
        "  %% each features: forbid-packages view: lit, node:*\n",
        "  %% each features: deny view: request\n",
        "  %% each features: controller --> service\n",
        "  %% packages billing-service: serde\n",
        "  %% deny app-shell: ui\n",
    ] {
        assert!(rendered.contains(line), "{line}\n{rendered}");
    }
    assert!(!rendered.contains("packages search-service"), "{rendered}");
    assert!(
        !rendered.contains("forbid-packages billing-view"),
        "{rendered}"
    );
    assert!(!rendered.contains("deny billing-view"), "{rendered}");
    let again = parse(&rendered).unwrap();
    // why: a rendered contract has other lines, so rules compare by what they say.
    assert_eq!(again.package_rules, contract.package_rules);
    let said = |contract: &Contract| -> Vec<String> {
        contract.rules.deny.iter().map(Denial::text).collect()
    };
    assert_eq!(said(&again), said(&contract));
    let bodies = |contract: &Contract| -> Vec<EachBody> {
        contract
            .rules
            .each
            .iter()
            .map(|rule| rule.body.clone())
            .collect()
    };
    assert_eq!(bodies(&again), bodies(&contract));
    assert_eq!(render(&parse(&rendered).unwrap()), rendered);
}
