//! `%% each <boundary>: map <dir>/`: parsing, expansion per child, the contract errors and the
//! round trip of "Edge templates".

use super::*;
use crate::contract::{Contract, EachBody, MapRule, MapTarget};

/// `billing` and `search` are children of `features`; `features-glue` sits directly in
/// `features`, so it is not a child.
const FIXTURE: &str = "flowchart LR
  subgraph app
    app-shell
  end
  subgraph features
    features-glue
    subgraph billing
      billing-controller
      billing-service
    end
    subgraph search
      search-service
    end
  end
";

fn with(extra: &str) -> String {
    format!("{FIXTURE}{extra}")
}

fn error(extra: &str) -> DiagramError {
    parse(&with(extra)).unwrap_err()
}

fn reason(error: &DiagramError) -> String {
    match error {
        DiagramError::MalformedRule { reason, .. } => reason.clone(),
        other => panic!("{other:?}"),
    }
}

/// The line of the first line after the fixture.
fn next_line() -> u32 {
    u32::try_from(FIXTURE.lines().count() + 1).unwrap()
}

/// `target@line: glob` of the expanded map rules.
fn expanded(extra: &str) -> Vec<String> {
    parse(&with(extra))
        .unwrap()
        .maps
        .iter()
        .filter(|rule| rule.template.is_some())
        .map(|rule| format!("{}@{}: {}", rule.target, rule.line, rule.globs[0]))
        .collect()
}

#[test]
fn the_map_form_is_stored_as_a_template() {
    let contract = parse(&with(
        "  %% each features: map src/features/{child}/, lib/{child}/\n",
    ))
    .unwrap();
    let [rule] = contract.rules.each.as_slice() else {
        panic!("{:?}", contract.rules.each);
    };
    assert_eq!(rule.boundary, "features");
    assert_eq!(
        rule.body,
        EachBody::Map {
            dirs: vec![
                "src/features/{child}/".to_owned(),
                "lib/{child}/".to_owned()
            ]
        }
    );
    assert_eq!(rule.form(), Some("map"));
    assert_eq!(rule.list(), "map src/features/{child}/, lib/{child}/");
}

#[test]
fn each_child_gets_a_module_form_map_at_the_each_line() {
    let found = expanded("  %% each features: map src/features/{child}/\n");
    let at = next_line();
    assert_eq!(
        found,
        [
            format!("billing-*@{at}: src/features/billing/"),
            format!("search-*@{at}: src/features/search/"),
        ]
    );
}

#[test]
fn every_directory_pattern_expands_for_every_child() {
    let found = expanded("  %% each features: map a/{child}/, b/x-{child}/\n");
    let globs: Vec<&str> = found.iter().filter_map(|f| f.split(": ").nth(1)).collect();
    assert_eq!(
        globs,
        ["a/billing/", "b/x-billing/", "a/search/", "b/x-search/"]
    );
}

#[test]
fn nodes_directly_in_the_boundary_are_not_children() {
    let found = expanded("  %% each features: map src/{child}/\n");
    assert!(found.iter().all(|f| !f.contains("glue")), "{found:?}");
    assert_eq!(found.len(), 2);
}

#[test]
fn expansions_are_ordinary_module_maps_beside_the_written_ones() {
    let contract = parse(&with(
        "  %% map billing-service: src/features/billing/service/\n  %% each features: map src/features/{child}/\n",
    ))
    .unwrap();
    let targets: Vec<(String, bool)> = contract
        .maps
        .iter()
        .map(|rule: &MapRule| (rule.target.to_string(), rule.template.is_some()))
        .collect();
    assert_eq!(
        targets,
        [
            ("billing-service".to_owned(), false),
            ("billing-*".to_owned(), true),
            ("search-*".to_owned(), true),
        ]
    );
    assert!(matches!(contract.maps[1].target, MapTarget::Modules(_)));
}

#[test]
fn a_role_called_map_in_an_arrow_still_parses() {
    let src = "flowchart LR
  subgraph features
    subgraph billing
      billing-map
      billing-view
    end
  end
  %% each features: map --> view
";
    let contract = parse(src).unwrap();
    assert!(matches!(contract.rules.each[0].body, EachBody::Arrows(_)));
    assert_eq!(contract.maps.len(), 0);
}

#[test]
fn a_bad_pattern_is_an_error_at_the_line() {
    for (text, part) in [
        ("map src/features/billing/", "`{child}`"),
        ("map src/{child}", "ends in `/`"),
        ("map src/{kid}/{child}/", "only placeholder"),
        ("map src/{child}/*/", "no wildcard"),
        ("map src/{child}/, src/{child}/", "listed twice"),
        ("map src/{child}/,", "expected"),
        ("map ../{child}/", ".."),
    ] {
        let err = error(&format!("  %% each features: {text}\n"));
        assert!(
            matches!(err, DiagramError::MalformedRule { line, .. } if line == next_line()),
            "{text}: {err:?}"
        );
        assert!(reason(&err).contains(part), "{text}: {}", reason(&err));
    }
}

#[test]
fn a_boundary_without_children_or_a_non_boundary_is_an_error() {
    let err = error("  %% each app: map src/{child}/\n");
    assert!(reason(&err).contains("no child boundary"), "{err:?}");
    let err = error("  %% each nowhere: map src/{child}/\n");
    assert!(matches!(err, DiagramError::MalformedRule { .. }), "{err:?}");
}

#[test]
fn an_expansion_that_repeats_a_written_glob_is_a_duplicate() {
    let err = error(
        "  %% map features-glue: src/features/billing/\n  %% each features: map src/features/{child}/\n",
    );
    assert!(
        matches!(err, DiagramError::DuplicateMapGlob { .. }),
        "{err:?}"
    );
}

#[test]
fn the_template_line_renders_back_and_the_expansions_do_not() {
    let src = with(
        "  %% map billing-service: src/features/billing/service/\n  %% each features: map src/features/{child}/\n",
    );
    let contract = parse(&src).unwrap();
    let rendered = render(&contract);
    assert!(
        rendered.contains("  %% each features: map src/features/{child}/\n"),
        "{rendered}"
    );
    assert!(
        rendered.contains("  %% map billing-service: src/features/billing/service/\n"),
        "{rendered}"
    );
    assert!(!rendered.contains("%% map billing-*"), "{rendered}");
    let again: Contract = parse(&rendered).unwrap();
    assert_eq!(again.maps, contract.maps);
    // why: a rendered contract has other lines, so templates compare by what they say.
    let bodies = |c: &Contract| {
        c.rules
            .each
            .iter()
            .map(|r| r.body.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(bodies(&again), bodies(&contract));
    assert_eq!(render(&again), rendered);
}
