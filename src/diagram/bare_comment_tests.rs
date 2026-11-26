//! Bare `%%` lines before the header, which Mermaid cannot draw.

use super::parse;

fn bare(src: &str) -> Vec<u32> {
    parse(src).unwrap().bare_comments
}

#[test]
fn a_bare_line_before_the_header_is_recorded_with_its_line() {
    assert_eq!(bare("%% top\n%%\nflowchart LR\n  a\n  b\n  a --> b\n"), [2]);
}

#[test]
fn an_indented_bare_line_and_a_bom_line_are_recorded() {
    assert_eq!(bare("  %%\n\t%%\nflowchart LR\n  a\n"), [1, 2]);
    assert_eq!(bare("\u{feff}%%\nflowchart LR\n  a\n"), [1]);
}

#[test]
fn a_comment_with_text_or_spaces_after_it_is_not_bare() {
    assert_eq!(bare("%% text\nflowchart LR\n  a\n").len(), 0);
    assert_eq!(bare("%%   \nflowchart LR\n  a\n").len(), 0);
}

#[test]
fn a_crlf_line_is_not_bare_because_mermaid_strips_it() {
    assert_eq!(bare("%%\r\nflowchart LR\r\n  a\r\n").len(), 0);
}

#[test]
fn a_bare_line_after_the_header_is_not_recorded() {
    assert_eq!(bare("flowchart LR\n  a\n%%\n  %%\n").len(), 0);
}
