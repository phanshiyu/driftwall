//! `%% order open:` in the parser and the renderer: what parses, and every contract error.

use super::*;

const LAYERS: &str = "flowchart LR
  subgraph entry[\"Entry\"]
    cli
  end
  subgraph engine[\"Engine\"]
    lint
  end
  subgraph shared[\"Shared\"]
    model
  end
";

fn with(extra: &str) -> String {
    format!("{LAYERS}{extra}")
}

fn error(src: &str) -> DiagramError {
    parse(src).unwrap_err()
}

#[test]
fn an_open_order_line_is_a_chain_marked_open_and_renders_back() {
    let contract = parse(&with(
        "  %% order open: entry > engine > shared\n  %% order: entry > engine\n",
    ))
    .unwrap();
    assert!(contract.rules.order[0].open);
    assert!(!contract.rules.order[1].open);
    assert_eq!(
        contract.rules.order[0].boundaries,
        ["entry", "engine", "shared"]
    );
    let rendered = render(&contract);
    assert!(
        rendered.contains("  %% order open: entry > engine > shared\n  %% order: entry > engine\n"),
        "{rendered}"
    );
    let reparsed = parse(&rendered).unwrap();
    let again = reparsed.rules.summary(&reparsed.nodes);
    assert_eq!(again, contract.rules.summary(&contract.nodes));
    let summary = contract.rules.summary(&contract.nodes);
    assert_eq!(summary.order.len(), 2);
    assert_eq!(summary.open_order, [["entry", "engine", "shared"]]);
}

#[test]
fn an_open_order_is_checked_like_a_plain_one() {
    for bad in [
        "%% order open:\n",
        "%% order open: entry\n",
        "%% order open: entry > ghost\n",
        "%% order open: entry > lint\n",
    ] {
        assert_eq!(error(&with(bad)).line(), Some(11), "{bad}");
    }
    // why: open and plain chains must agree on the boundaries they share.
    let clash = with("  %% order open: entry > engine\n  %% order: engine > entry\n");
    assert_eq!(error(&clash).line(), Some(12));
    // why: an upward arrow is a contract error in an open chain too.
    let upward = with("  model --> cli\n  %% order open: entry > shared\n");
    assert!(matches!(error(&upward), DiagramError::UpwardArrow { .. }));
}

#[test]
fn only_the_colon_directly_after_open_makes_the_chain_open() {
    let plain = with("%% order open entry > engine\n");
    assert!(parse(&plain).unwrap().rules.is_empty());
}
