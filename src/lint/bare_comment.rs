//! Bare `%%` lines before the header, which Mermaid cannot draw.

use std::path::Path;

use crate::code::Location;
use crate::contract::Contract;
use crate::results::{Violation, ViolationKind};

use super::rules::violation;

/// A `bare-comment` per line before the `flowchart` header that is exactly `%%`: Mermaid removes a
/// comment only when text follows `%%`, so GitHub and other renderers fail on the file.
pub(super) fn findings(contract: &Contract, contract_file: &Path) -> Vec<Violation> {
    let file = contract_file.display();
    contract
        .bare_comments
        .iter()
        .map(|&line| {
            violation(
                ViolationKind::BareComment,
                Some(Location {
                    file: contract_file.to_path_buf(),
                    line,
                }),
                format!(
                    "line {line} of {file} is a bare `%%` before the `flowchart` header: Mermaid removes a comment only when text follows `%%`, so GitHub and other Mermaid renderers cannot draw this file; use a blank line or `%% <text>`"
                ),
            )
            .keyed(line.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::parse;
    use crate::results::Severity;

    #[test]
    fn each_bare_line_is_a_warning_keyed_by_its_line() {
        let contract = parse("%% top\n%%\n%%\nflowchart LR\n  a\n  b\n  a --> b\n").unwrap();
        let found = findings(&contract, Path::new("architecture.mmd"));
        let keys: Vec<&str> = found.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(keys, ["2", "3"]);
        let one = &found[0];
        assert_eq!(one.kind, ViolationKind::BareComment);
        assert_eq!(one.severity, Severity::Warning);
        assert_eq!(one.at.as_ref().map(|at| at.line), Some(2));
        assert!(
            one.message.contains("line 2 of architecture.mmd"),
            "{}",
            one.message
        );
    }

    #[test]
    fn a_clean_header_has_no_findings() {
        let contract = parse("%% top\nflowchart LR\n  a\n  b\n  a --> b\n").unwrap();
        assert_eq!(findings(&contract, Path::new("a.mmd")).len(), 0);
    }
}
