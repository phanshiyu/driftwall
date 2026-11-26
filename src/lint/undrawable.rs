//! Ids and labels that Mermaid cannot parse, so GitHub and other renderers cannot draw the contract.

use std::path::Path;

use crate::code::Location;
use crate::contract::Contract;
use crate::results::{Violation, ViolationKind};

use super::rules::violation;

/// An `undrawable-id` per node or boundary id Mermaid rejects, keyed by the id, and an
/// `undrawable-label` per label or comment line it rejects, keyed by the line.
pub(super) fn findings(contract: &Contract, contract_file: &Path) -> Vec<Violation> {
    let file = contract_file.display();
    let at = |line| Location {
        file: contract_file.to_path_buf(),
        line,
    };
    let ids = contract.undrawable_ids.iter().map(|(id, line)| {
        violation(
            ViolationKind::UndrawableId,
            Some(at(*line)),
            format!(
                "`{id}` on line {line} of {file} cannot be a Mermaid node id, so GitHub and other Mermaid renderers cannot draw this file; rename it"
            ),
        )
        .keyed(id.clone())
    });
    let labels = contract.undrawable_labels.iter().map(|(line, what)| {
        violation(
            ViolationKind::UndrawableLabel,
            Some(at(*line)),
            format!(
                "line {line} of {file}: {what}, so GitHub and other Mermaid renderers cannot draw this file"
            ),
        )
        .keyed(line.to_string())
    });
    ids.chain(labels).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::parse;
    use crate::results::Severity;

    #[test]
    fn an_id_is_a_warning_keyed_by_the_id_at_its_line() {
        let contract = parse("flowchart LR\n  class\n  a\n  a --> class\n").unwrap();
        let found = findings(&contract, Path::new("architecture.mmd"));
        assert_eq!(found.len(), 1);
        let one = &found[0];
        assert_eq!(one.kind, ViolationKind::UndrawableId);
        assert_eq!(one.severity, Severity::Warning);
        assert_eq!(one.key, "class");
        assert_eq!(one.at.as_ref().map(|at| at.line), Some(2));
    }

    #[test]
    fn a_label_is_a_warning_keyed_by_the_line() {
        let src = "flowchart LR\n  a[\"\"]\n  b\n  a -->|uses (x)| b\n";
        let found = findings(&parse(src).unwrap(), Path::new("architecture.mmd"));
        let keys: Vec<&str> = found.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(keys, ["2", "4"]);
        assert!(
            found
                .iter()
                .all(|v| v.kind == ViolationKind::UndrawableLabel)
        );
        assert!(
            found[1].message.contains("parenthesis"),
            "{}",
            found[1].message
        );
    }

    #[test]
    fn a_drawable_contract_has_no_findings() {
        let src = "%%{init: {}}%%\nflowchart LR\n  End\n  a\n  a -->|type| End\n";
        assert_eq!(
            findings(&parse(src).unwrap(), Path::new("a.mmd")),
            Vec::new()
        );
    }
}
