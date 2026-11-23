//! The `%% facade` directive of "Re-exports": what parses and every contract error.

use super::*;

const NODES: &str = "flowchart LR\n  a\n  b\n  c\n";

fn error(extra: &str) -> DiagramError {
    parse(&format!("{NODES}{extra}")).unwrap_err()
}

#[test]
fn nodes_are_comma_separated_and_may_span_several_lines() {
    let contract = parse(&format!("{NODES}  %% facade a, b\n  %% facade c\n")).unwrap();
    let facades: Vec<(String, u32)> = contract
        .rules
        .facades
        .iter()
        .map(|(node, line)| (node.to_string(), *line))
        .collect();
    assert_eq!(
        facades,
        [
            ("a".to_owned(), 5),
            ("b".to_owned(), 5),
            ("c".to_owned(), 6)
        ]
    );
    assert_eq!(contract.rules.summary().facades, ["a", "b", "c"]);
}

#[test]
fn an_unknown_node_is_an_error_at_its_line() {
    let found = error("  %% facade a, zzz\n").to_string();
    assert!(found.contains("line 5") && found.contains("zzz"), "{found}");
}

#[test]
fn an_empty_or_repeated_list_is_an_error() {
    assert!(error("  %% facade\n").to_string().contains("line 5"));
    let repeated = error("  %% facade a\n  %% facade b, a\n").to_string();
    assert!(
        repeated.contains("line 6") && repeated.contains("line 5"),
        "{repeated}"
    );
}

#[test]
fn a_comment_that_merely_starts_with_the_letters_stays_plain() {
    assert!(parse(&format!("{NODES}  %% facades are fine here\n")).is_ok());
}

#[test]
fn facades_survive_a_render_round_trip_on_one_line() {
    let contract = parse(&format!("{NODES}  %% facade c\n  %% facade a\n")).unwrap();
    let rendered = render(&contract);
    assert!(rendered.contains("  %% facade a, c\n"), "{rendered}");
    let again = parse(&rendered).unwrap();
    assert_eq!(again.rules.summary().facades, ["a", "c"]);
}
