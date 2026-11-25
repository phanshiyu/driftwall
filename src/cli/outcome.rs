//! How a command ended, and the exit code each ending maps to (see docs/DESIGN.md).

use driftwall::code::{FileKind, Located};
use driftwall::results::{ContractDiff, LintReport};

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

    /// The outcome of `where`: a source file (or directory) nothing owns is a finding. Assets,
    /// unread, excluded and ignored files are not findings whatever their owner.
    #[must_use]
    pub const fn of_where(located: &Located) -> Self {
        match (&located.kind, located.owner.node()) {
            (FileKind::Source | FileKind::Directory, None) => Self::Findings,
            _ => Self::Clean,
        }
    }

    /// The outcome of `diff`: a changed contract, or debt added to the baseline (`debt` entries),
    /// is a finding only when `exit_code` asks for it.
    #[must_use]
    pub const fn of_diff(diff: &ContractDiff, debt: usize, exit_code: bool) -> Self {
        if exit_code && (!diff.is_empty() || debt > 0) {
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
    use driftwall::code::{Owner, Rule};
    use driftwall::contract::NodeId;
    use driftwall::results::{Severity, Violation, ViolationKind};

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
                    test_only: false,
                    key: String::new(),
                    site: driftwall::results::SiteDetail::default(),
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
        let located = |kind, owned: bool| Located {
            kind,
            owner: if owned {
                Owner::Owned {
                    node: NodeId::new("lint").unwrap(),
                    rule: Rule::Tag {
                        arch_file: "src/lint/.arch".into(),
                    },
                }
            } else {
                Owner::Unowned
            },
        };
        assert_eq!(
            Outcome::of_where(&located(FileKind::Source, true)),
            Outcome::Clean
        );
        assert_eq!(
            Outcome::of_where(&located(FileKind::Source, false)),
            Outcome::Findings
        );
    }

    #[test]
    fn only_an_unmapped_source_file_is_a_finding_for_where() {
        let unowned = |kind| Located {
            kind,
            owner: Owner::Unowned,
        };
        for kind in [
            FileKind::Asset,
            FileKind::Unread("swift".to_owned()),
            FileKind::Excluded(driftwall::code::Exclusion::Vendored),
            FileKind::Ignored,
            FileKind::Folded,
        ] {
            assert_eq!(Outcome::of_where(&unowned(kind)), Outcome::Clean);
        }
    }

    #[test]
    fn a_changed_contract_is_a_finding_only_with_exit_code() {
        let changed = ContractDiff {
            added_nodes: vec![NodeId::new("a").unwrap()],
            ..ContractDiff::default()
        };
        assert_eq!(Outcome::of_diff(&changed, 0, false), Outcome::Clean);
        assert_eq!(Outcome::of_diff(&changed, 0, true), Outcome::Findings);
    }

    #[test]
    fn added_debt_is_a_finding_with_exit_code_even_when_the_contract_is_unchanged() {
        let same = ContractDiff::default();
        assert_eq!(Outcome::of_diff(&same, 1, false), Outcome::Clean);
        assert_eq!(Outcome::of_diff(&same, 1, true), Outcome::Findings);
    }

    #[test]
    fn an_unchanged_contract_is_always_clean() {
        let same = ContractDiff::default();
        assert_eq!(Outcome::of_diff(&same, 0, false), Outcome::Clean);
        assert_eq!(Outcome::of_diff(&same, 0, true), Outcome::Clean);
    }

    #[test]
    fn one_error_among_warnings_fails() {
        let mixed = report(&[Severity::Warning, Severity::Error]);
        assert_eq!(Outcome::of_lint(&mixed, false), Outcome::Findings);
    }
}
