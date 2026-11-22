//! How a command ended, and the exit code each ending maps to (see docs/DESIGN.md).

use driftwall::model::{ContractDiff, LintReport};
use driftwall::ownership::Explanation;

/// The three ways a driftwall command can end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to act on.
    Clean,
    /// The tool worked and found something. A failing lint is not an error.
    Findings,
    /// The tool could not do its job: bad contract, IO failure, bad arguments.
    ToolError,
}

impl Outcome {
    /// The outcome of linting: findings when any error was reported, or any finding at all
    /// under `strict`.
    #[must_use]
    pub fn of_lint(report: &LintReport, strict: bool) -> Self {
        if report.failed(strict) {
            Self::Findings
        } else {
            Self::Clean
        }
    }

    /// The outcome of `where`: a file nothing owns is a finding.
    #[must_use]
    pub const fn of_where(explanation: &Explanation) -> Self {
        if explanation.node.is_some() {
            Self::Clean
        } else {
            Self::Findings
        }
    }

    /// The outcome of `diff`: a changed contract is a finding only when `exit_code` asks for it.
    #[must_use]
    pub const fn of_diff(diff: &ContractDiff, exit_code: bool) -> Self {
        if exit_code && !diff.is_empty() {
            Self::Findings
        } else {
            Self::Clean
        }
    }

    /// The process exit code: `0` clean, `1` findings, `2` tool error.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Clean => 0,
            Self::Findings => 1,
            Self::ToolError => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use driftwall::model::{NodeId, Severity, Violation, ViolationKind};

    use super::*;

    fn report(severities: &[Severity]) -> LintReport {
        LintReport {
            violations: severities
                .iter()
                .map(|&severity| Violation {
                    kind: ViolationKind::StaleEdge,
                    severity,
                    at: None,
                    message: String::new(),
                    hint: None,
                })
                .collect(),
        }
    }

    #[test]
    fn exit_codes_follow_the_design_doc() {
        assert_eq!(Outcome::Clean.code(), 0);
        assert_eq!(Outcome::Findings.code(), 1);
        assert_eq!(Outcome::ToolError.code(), 2);
    }

    #[test]
    fn empty_report_is_clean_with_or_without_strict() {
        let empty = report(&[]);
        assert_eq!(Outcome::of_lint(&empty, false), Outcome::Clean);
        assert_eq!(Outcome::of_lint(&empty, true), Outcome::Clean);
    }

    #[test]
    fn errors_fail_with_or_without_strict() {
        let errors = report(&[Severity::Error, Severity::Error]);
        assert_eq!(Outcome::of_lint(&errors, false), Outcome::Findings);
        assert_eq!(Outcome::of_lint(&errors, true), Outcome::Findings);
    }

    #[test]
    fn warnings_only_pass_unless_strict() {
        let warnings = report(&[Severity::Warning]);
        assert_eq!(Outcome::of_lint(&warnings, false), Outcome::Clean);
        assert_eq!(Outcome::of_lint(&warnings, true), Outcome::Findings);
    }

    #[test]
    fn a_mapped_file_is_clean_and_an_unmapped_one_is_a_finding() {
        let mapped = Explanation {
            node: Some(NodeId::new("lint").unwrap()),
            arch_file: Some("src/lint/.arch".into()),
        };
        let unmapped = Explanation {
            node: None,
            arch_file: None,
        };
        assert_eq!(Outcome::of_where(&mapped), Outcome::Clean);
        assert_eq!(Outcome::of_where(&unmapped), Outcome::Findings);
    }

    #[test]
    fn a_changed_contract_is_a_finding_only_with_exit_code() {
        let changed = ContractDiff {
            added_nodes: vec![NodeId::new("a").unwrap()],
            ..ContractDiff::default()
        };
        assert_eq!(Outcome::of_diff(&changed, false), Outcome::Clean);
        assert_eq!(Outcome::of_diff(&changed, true), Outcome::Findings);
    }

    #[test]
    fn an_unchanged_contract_is_always_clean() {
        let same = ContractDiff::default();
        assert_eq!(Outcome::of_diff(&same, false), Outcome::Clean);
        assert_eq!(Outcome::of_diff(&same, true), Outcome::Clean);
    }

    #[test]
    fn one_error_among_warnings_fails() {
        let mixed = report(&[Severity::Warning, Severity::Error]);
        assert_eq!(Outcome::of_lint(&mixed, false), Outcome::Findings);
    }
}
