//! The findings of a lint run.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::code::Location;

/// Kinds of finding the linter can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViolationKind {
    /// A source file has no inherited `.arch` tag.
    UnmappedFile,
    /// A `.arch` tag names a node the diagram does not declare.
    UnknownNode,
    /// Code in node A imports node B and the diagram has no A to B edge.
    UndeclaredEdge,
    /// Code imports a pair that a `%% deny` line forbids.
    DeniedEdge,
    /// A solid diagram edge has no import backing it.
    StaleEdge,
    /// A planned (dotted) edge is now used by code and should be made solid.
    PlannedRealized,
    /// A solid edge is realised only by test-only imports and should be declared `-->|test|`.
    TestOnlyEdge,
    /// A node edge that a boundary edge already covers with a kind at least as wide.
    RedundantEdge,
    /// A relative import could not be resolved to a file.
    UnresolvedImport,
    /// An import uses a symbol outside the public surface its target node declares.
    PrivateSymbol,
    /// A `%% map` glob matches no file.
    StaleMap,
    /// The contract maps files itself, and `.arch` files exist that are not read.
    IgnoredTagFiles,
    /// Code in a lower boundary imports a node in a higher one, against `%% order`.
    UpwardEdge,
    /// A node exceeds a limit of its `%% budget`.
    OverBudget,
    /// Production code imports a test file, with `%% tests: isolated`.
    ProductionImportsTest,
    /// A package use that a `%% forbid-packages` line names, or a third-party one that the
    /// `%% packages` allow-list of its node does not list.
    ForbiddenPackage,
    /// An entry of a `%% packages` line that no package use of its node matches.
    StalePackage,
    /// A node named by `%% facade` owns no file that re-exports anything.
    StaleFacade,
    /// An import goes between two children of a `%% isolated` boundary.
    IsolationBreach,
    /// A `%% isolated` boundary has fewer than two children.
    StaleIsolation,
    /// A debt baseline entry records more findings than exist now, see "Debt baseline".
    StaleBaseline,
}

/// How serious a violation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Fails the lint (exit code 1).
    Error,
    /// Reported but does not fail unless `--strict`.
    Warning,
}

/// One finding, with enough location detail for an agent to act on it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Violation {
    /// What went wrong.
    pub kind: ViolationKind,
    /// How serious it is.
    pub severity: Severity,
    /// Offending location, when the finding has one.
    pub at: Option<Location>,
    /// One-line description.
    pub message: String,
    /// Suggested fix, phrased as an action.
    pub hint: Option<String>,
    /// Whether the finding is at an import site written in test code. Serialized only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub test_only: bool,
    /// What a debt baseline records the finding as, see "Debt baseline" in `docs/DESIGN.md`.
    /// Empty for a finding no baseline can hold. Never serialized.
    #[serde(skip)]
    pub key: String,
    /// What the text output lists for the finding when it groups sites that share a key. Never
    /// serialized: JSON and SARIF stay per site.
    #[serde(skip)]
    pub site: SiteDetail,
}

/// The parts of an import-site finding that differ between the sites of one key.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SiteDetail {
    /// The file the import resolves to, when the finding is about an import of one.
    pub target: Option<PathBuf>,
    /// The symbol the import uses, for a `private-symbol` finding.
    pub symbol: Option<String>,
}

impl Violation {
    /// The finding with the import target it is about, for the grouped text output.
    #[must_use]
    pub fn targeting(mut self, target: &Path) -> Self {
        self.site.target = Some(target.to_path_buf());
        self
    }

    /// The finding with the symbol it is about, for the grouped text output.
    #[must_use]
    pub fn naming(mut self, symbol: impl Into<String>) -> Self {
        self.site.symbol = Some(symbol.into());
        self
    }

    /// The finding with its baseline key. A finding at a test-only site gets ` (test)` appended,
    /// so test debt and production debt are separate entries.
    #[must_use]
    pub fn keyed(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        if self.test_only {
            self.key.push_str(" (test)");
        }
        self
    }
}

/// The complete result of a lint run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LintReport {
    /// Findings, sorted by kind, location, message.
    pub violations: Vec<Violation>,
}

impl LintReport {
    /// Counts of error and warning findings, in that order.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        let errors = self
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count();
        (errors, self.violations.len() - errors)
    }

    /// True when any finding is an [`Severity::Error`], or any finding at all when `strict`.
    #[must_use]
    pub fn failed(&self, strict: bool) -> bool {
        self.violations
            .iter()
            .any(|v| strict || v.severity == Severity::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_split_errors_from_warnings() {
        let violation = |severity| Violation {
            kind: ViolationKind::StaleEdge,
            severity,
            at: None,
            message: String::new(),
            hint: None,
            test_only: false,
            key: String::new(),
            site: crate::results::SiteDetail::default(),
        };
        let report = LintReport {
            violations: vec![
                violation(Severity::Error),
                violation(Severity::Warning),
                violation(Severity::Warning),
            ],
        };
        assert_eq!(report.counts(), (1, 2));
        assert_eq!(LintReport::default().counts(), (0, 0));
    }

    #[test]
    fn warnings_only_fail_when_strict() {
        let report = LintReport {
            violations: vec![Violation {
                kind: ViolationKind::StaleEdge,
                severity: Severity::Warning,
                at: None,
                message: String::new(),
                hint: None,
                test_only: false,
                key: String::new(),
                site: crate::results::SiteDetail::default(),
            }],
        };
        assert!(!report.failed(false));
        assert!(report.failed(true));
    }
}
