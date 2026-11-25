//! `%% deny` in the parser and the renderer: what parses, and every contract error.

use super::*;
use crate::model::{Denial, Endpoint, NodeId};

/// Lines 1 to 14: three layers, `ui` with two nodes, an open chain on line 15.
const LAYERS: &str = "flowchart LR
  subgraph ui[\"Ui\"]
    ui-a
    ui-b
  end
  subgraph shared[\"Shared\"]
    sh
  end
  subgraph core[\"Core\"]
    co
    co-b
  end
  loose
  %% order open: ui > shared > core
";

const CHAIN_LINE: u32 = 14;

fn with(extra: &str) -> String {
    format!("{LAYERS}{extra}")
}

fn error(extra: &str) -> DiagramError {
    parse(&with(extra)).unwrap_err()
}

fn node(id: &str) -> Endpoint {
    Endpoint::Node(NodeId::new(id).unwrap())
}

fn boundary(id: &str) -> Endpoint {
    Endpoint::Boundary(id.to_owned())
}

fn denial(line: u32, from: Endpoint, to: Endpoint) -> Denial {
    Denial { line, from, to }
}

#[test]
fn a_deny_line_is_one_denial_per_target_with_its_line() {
    let contract = parse(&with("  %% deny ui: core, shared\n  %% deny ui-a: co-b\n")).unwrap();
    assert_eq!(
        contract.rules.deny,
        [
            denial(CHAIN_LINE + 1, boundary("ui"), boundary("core")),
            denial(CHAIN_LINE + 1, boundary("ui"), boundary("shared")),
            denial(CHAIN_LINE + 2, node("ui-a"), node("co-b")),
        ]
    );
    assert!(!contract.rules.is_empty());
}

#[test]
fn a_deny_directive_may_stand_anywhere_and_ids_are_checked_after_the_whole_file() {
    let src = "%% deny ui: core\nflowchart LR\n  subgraph ui[\"Ui\"]\n    a\n  end\n  subgraph core[\"Core\"]\n    c\n  end\n  %% order open: ui > core\n";
    // why: a directive above the header is read like any other comment line before it.
    let contract = parse(src).unwrap();
    assert_eq!(contract.rules.deny.len(), 1);
    assert_eq!(contract.rules.deny[0].line, 1);
}

#[test]
fn only_a_comment_whose_first_word_is_deny_is_a_directive() {
    for plain in [
        "%% denylist of old modules\n",
        "%% denying access\n",
        "%% deny: ui core\n",
        "%% why deny ui: core\n",
        "%%deny-all\n",
    ] {
        let contract = parse(&with(plain)).unwrap();
        assert!(contract.rules.deny.is_empty(), "{plain}");
    }
}

#[test]
fn a_malformed_deny_line_is_an_error_on_its_line() {
    for bad in [
        "%% deny\n",
        "%% deny ui\n",
        "%% deny ui core\n",
        "%% deny ui:\n",
        "%% deny ui: ,\n",
        "%% deny ui: core,\n",
        "%% deny : core\n",
        "%% deny ui: co re\n",
    ] {
        let found = error(bad);
        assert_eq!(found.line(), Some(CHAIN_LINE + 1), "{bad}: {found}");
    }
}

#[test]
fn an_unknown_id_is_an_error_on_the_line_naming_it() {
    for (bad, id) in [
        ("%% deny ghost: core\n", "ghost"),
        ("%% deny ui: ghost\n", "ghost"),
        ("%% deny ui: core, ghost\n", "ghost"),
    ] {
        let found = error(bad);
        assert_eq!(found.line(), Some(CHAIN_LINE + 1), "{bad}");
        assert!(found.to_string().contains(&format!("`{id}`")), "{found}");
    }
}

#[test]
fn the_same_pair_twice_names_the_first_line() {
    let in_one_line = error("%% deny ui: core, core\n");
    assert_eq!(in_one_line.line(), Some(CHAIN_LINE + 1));
    assert!(
        in_one_line
            .to_string()
            .contains("already denied on line 15"),
        "{in_one_line}"
    );
    let across = error("%% deny ui: core\n%% deny ui: shared\n%% deny ui: core\n");
    assert_eq!(across.line(), Some(CHAIN_LINE + 3));
    assert!(
        across.to_string().contains("already denied on line 15"),
        "{across}"
    );
    // why: a different pair with the same source is not a repeat.
    assert!(parse(&with("%% deny ui: core\n%% deny ui: shared\n")).is_ok());
}

#[test]
fn a_declared_edge_of_any_kind_that_a_denial_covers_is_a_contract_error() {
    for edge in [
        "ui-a --> co",
        "ui-a -.-> co",
        "ui-a -->|test| co",
        "ui-a -->|type| co",
        "ui-a -->|type, test| co",
        "ui --> co",
        "ui-a --> core",
        "ui --> core",
    ] {
        let found = error(&format!("{edge}\n%% deny ui: core\n"));
        assert_eq!(found.line(), Some(CHAIN_LINE + 2), "{edge}");
        let message = found.to_string();
        assert!(message.contains(edge), "{message}");
        assert!(message.contains("line 15"), "{message}");
    }
}

#[test]
fn an_each_expansion_that_a_denial_covers_is_a_contract_error() {
    let src = "flowchart LR
  subgraph features[\"Features\"]
    subgraph billing[\"Billing\"]
      billing-controller
      billing-service
    end
    subgraph search[\"Search\"]
      search-controller
      search-service
    end
  end
  %% each features: controller --> service
  %% deny billing-controller: billing-service
";
    let found = parse(src).unwrap_err();
    assert_eq!(found.line(), Some(13));
    assert!(
        found
            .to_string()
            .contains("each features: controller --> service"),
        "{found}"
    );
    assert!(found.to_string().contains("line 12"), "{found}");
}

#[test]
fn a_denial_that_removes_nothing_is_a_contract_error_saying_why() {
    // why: upward, inside one boundary, against a node no chain lists, and a node against itself.
    for dead in [
        "%% deny core: ui\n",
        "%% deny ui-a: ui-b\n",
        "%% deny loose: co\n",
        "%% deny ui: loose\n",
        "%% deny ui-a: ui-a\n",
    ] {
        let found = error(dead);
        assert_eq!(found.line(), Some(CHAIN_LINE + 1), "{dead}");
        assert!(found.to_string().contains("no open chain lets"), "{found}");
    }
    let found = error("%% deny core: ui\n");
    assert!(
        found
            .to_string()
            .contains("no open chain lets `core` use `ui`"),
        "{found}"
    );
}

#[test]
fn a_plain_chain_allows_nothing_so_a_denial_under_it_is_dead() {
    let src = LAYERS.replace("order open:", "order:") + "%% deny ui: core\n";
    let found = parse(&src).unwrap_err();
    assert!(found.to_string().contains("no open chain lets"), "{found}");
}

#[test]
fn a_denial_of_a_narrowed_pair_removes_nothing() {
    // why: `ui-a --> sh` narrows `ui` towards `shared`, so only the drawn arrow is allowed there.
    let narrowed = with("ui-a --> sh\n%% deny ui-b: sh\n");
    let found = parse(&narrowed).unwrap_err();
    assert!(found.to_string().contains("no open chain lets"), "{found}");
    // why: `core` is a different layer pair, still wide open.
    assert!(parse(&with("ui-a --> sh\n%% deny ui-b: co\n")).is_ok());
}

#[test]
fn each_target_of_a_line_is_judged_on_its_own() {
    // why: `ui` to `loose` removes nothing, and the allowed `core` next to it does not save it.
    let found = error("%% deny ui: loose, core\n");
    assert!(found.to_string().contains("`ui` use `loose`"), "{found}");
}

#[test]
fn deny_lines_survive_a_render_round_trip() {
    let contract = parse(&with("%% deny ui: core, shared\n%% deny ui-a: co-b\n")).unwrap();
    let rendered = render(&contract);
    assert!(
        rendered.contains("  %% deny ui: core\n  %% deny ui: shared\n  %% deny ui-a: co-b\n"),
        "{rendered}"
    );
    // why: a rendered file has one pair per line, so its lines are the ones it parses back to.
    let canonical = parse(&rendered).unwrap();
    assert_eq!(canonical.rules.deny.len(), 3);
    assert_eq!(parse(&render(&canonical)).as_ref(), Ok(&canonical));
    let pairs: Vec<(String, String)> = canonical
        .rules
        .deny
        .iter()
        .map(|d| (d.from.to_string(), d.to.to_string()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("ui".to_owned(), "core".to_owned()),
            ("ui".to_owned(), "shared".to_owned()),
            ("ui-a".to_owned(), "co-b".to_owned()),
        ]
    );
}
