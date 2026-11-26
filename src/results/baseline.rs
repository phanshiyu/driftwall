//! The debt baseline: parse, write, apply and compare (see "Debt baseline" in `docs/DESIGN.md`).
//!
//! Pure over text and findings. Reading the file and asking git for an old copy is the CLI's job.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::code::Location;

use super::{LintReport, Severity, Violation, ViolationKind};

/// Every kind a baseline can hold, in no particular order.
const BASELINABLE: [ViolationKind; 24] = [
    ViolationKind::UnmappedFile,
    ViolationKind::UnknownNode,
    ViolationKind::UndeclaredEdge,
    ViolationKind::DeniedEdge,
    ViolationKind::StaleEdge,
    ViolationKind::PlannedRealized,
    ViolationKind::TestOnlyEdge,
    ViolationKind::RedundantEdge,
    ViolationKind::UnresolvedImport,
    ViolationKind::PrivateSymbol,
    ViolationKind::StaleMap,
    ViolationKind::IgnoredTagFiles,
    ViolationKind::UpwardEdge,
    ViolationKind::OverBudget,
    ViolationKind::ProductionImportsTest,
    ViolationKind::ForbiddenPackage,
    ViolationKind::StalePackage,
    ViolationKind::StaleFacade,
    ViolationKind::FacadeWithoutExports,
    ViolationKind::BareComment,
    ViolationKind::UndrawableId,
    ViolationKind::UndrawableLabel,
    ViolationKind::IsolationBreach,
    ViolationKind::StaleIsolation,
];

/// The header of a written baseline, fixed text so writing is idempotent.
const HEADER: &str = "\
# driftwall debt baseline. Each line is one kind of existing finding: <kind> <count> <key>.
# CI fails when a key has more findings than recorded, or a new key appears.
# Fixing an entry never fails CI; `lint --strict` then asks to shrink this file with
# `driftwall lint --write-baseline <this file>`.
";

impl ViolationKind {
    /// The kebab-case name, as JSON and the baseline file spell it. Exhaustive, so a new kind
    /// fails to compile here.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::UnmappedFile => "unmapped-file",
            Self::UnknownNode => "unknown-node",
            Self::UndeclaredEdge => "undeclared-edge",
            Self::DeniedEdge => "denied-edge",
            Self::StaleEdge => "stale-edge",
            Self::PlannedRealized => "planned-realized",
            Self::TestOnlyEdge => "test-only-edge",
            Self::RedundantEdge => "redundant-edge",
            Self::UnresolvedImport => "unresolved-import",
            Self::PrivateSymbol => "private-symbol",
            Self::StaleMap => "stale-map",
            Self::IgnoredTagFiles => "ignored-tag-files",
            Self::UpwardEdge => "upward-edge",
            Self::OverBudget => "over-budget",
            Self::ProductionImportsTest => "production-imports-test",
            Self::ForbiddenPackage => "forbidden-package",
            Self::StalePackage => "stale-package",
            Self::StaleFacade => "stale-facade",
            Self::FacadeWithoutExports => "facade-without-exports",
            Self::BareComment => "bare-comment",
            Self::UndrawableId => "undrawable-id",
            Self::UndrawableLabel => "undrawable-label",
            Self::IsolationBreach => "isolation-breach",
            Self::StaleIsolation => "stale-isolation",
            Self::StaleBaseline => "stale-baseline",
        }
    }
}

/// `path` with `/` separators, as a baseline key spells paths.
#[must_use]
pub fn path_key(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// A baseline file line that cannot be read. Every variant names its 1-indexed `line`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BaselineError {
    /// The line is not `<kind> <count> <key>`.
    #[error("line {line}: expected `<kind> <count> <key>`")]
    Malformed {
        /// The line of the file.
        line: u32,
    },
    /// The kind is not one a baseline can hold.
    #[error("line {line}: `{kind}` is not a kind a baseline can hold")]
    UnknownKind {
        /// The line of the file.
        line: u32,
        /// The kind as written.
        kind: String,
    },
    /// The count is not a positive integer.
    #[error("line {line}: `{count}` is not a positive count")]
    BadCount {
        /// The line of the file.
        line: u32,
        /// The count as written.
        count: String,
    },
    /// The entry has a kind and a count but no key.
    #[error("line {line}: the entry has no key")]
    NoKey {
        /// The line of the file.
        line: u32,
    },
    /// The kind and key were listed before.
    #[error("line {line}: the kind and key are already listed")]
    Duplicate {
        /// The line of the file.
        line: u32,
    },
}

impl BaselineError {
    /// The 1-indexed line of the file the error is about.
    #[must_use]
    pub const fn line(&self) -> u32 {
        match self {
            Self::Malformed { line }
            | Self::UnknownKind { line, .. }
            | Self::BadCount { line, .. }
            | Self::NoKey { line }
            | Self::Duplicate { line } => *line,
        }
    }
}

/// One recorded entry: how many findings of its key are accepted, and where the file says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    count: usize,
    line: u32,
}

/// The recorded debt: findings accepted per `(kind, key)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Baseline {
    entries: BTreeMap<(String, String), Entry>,
}

/// What applying a baseline did, for the renderers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineOutcome {
    /// The baseline file as given, for output.
    pub file: PathBuf,
    /// The findings the baseline accepted, which no longer count.
    pub suppressed: Vec<Violation>,
    /// How many `stale-baseline` warnings the report gained.
    pub stale: usize,
}

/// A report after a baseline, with what the baseline did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The findings that remain, with the `stale-baseline` warnings, sorted.
    pub report: LintReport,
    /// What was suppressed and how much of the file is stale.
    pub outcome: BaselineOutcome,
}

/// An entry that is new or grew between two baselines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DebtAdded {
    /// The kind name.
    pub kind: String,
    /// The key.
    pub key: String,
    /// The count before, 0 for a new entry.
    pub old: usize,
    /// The count now.
    pub new: usize,
}

impl Baseline {
    /// Parses a baseline file.
    ///
    /// # Errors
    /// The first malformed line: an unknown kind, a count that is not a positive integer, a
    /// missing key, or a kind and key listed twice.
    pub fn parse(text: &str) -> Result<Self, BaselineError> {
        // why: editors on Windows save a UTF-8 BOM, which would make the first line an unknown kind.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut entries = BTreeMap::new();
        for (index, raw) in text.lines().enumerate() {
            let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let mut parts = trimmed.splitn(3, ' ');
            let kind = parts.next().unwrap_or_default();
            let count = parts.next().ok_or(BaselineError::Malformed { line })?;
            let key = parts.next().map(str::trim).unwrap_or_default();
            if !BASELINABLE.iter().any(|known| known.name() == kind) {
                return Err(BaselineError::UnknownKind {
                    line,
                    kind: kind.to_owned(),
                });
            }
            let count = count
                .parse::<usize>()
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| BaselineError::BadCount {
                    line,
                    count: count.to_owned(),
                })?;
            if key.is_empty() {
                return Err(BaselineError::NoKey { line });
            }
            let previous = entries.insert((kind.to_owned(), key.to_owned()), Entry { count, line });
            if previous.is_some() {
                return Err(BaselineError::Duplicate { line });
            }
        }
        Ok(Self { entries })
    }

    /// The baseline that accepts exactly the findings of `report`. Findings without a key are
    /// not recorded.
    #[must_use]
    pub fn from_report(report: &LintReport) -> Self {
        let mut entries: BTreeMap<(String, String), Entry> = BTreeMap::new();
        for found in report.violations.iter().filter(|v| !v.key.is_empty()) {
            let slot = entries
                .entry((found.kind.name().to_owned(), found.key.clone()))
                .or_insert(Entry { count: 0, line: 0 });
            slot.count += 1;
        }
        Self { entries }
    }

    /// The file text: the header, then one entry per line sorted by kind then key.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = HEADER.to_owned();
        for ((kind, key), entry) in &self.entries {
            // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
            let _ = writeln!(out, "{kind} {} {key}", entry.count);
        }
        out
    }

    /// How many entries the baseline has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the baseline has no entry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The findings the baseline accepts, summed over its entries.
    #[must_use]
    pub fn total(&self) -> usize {
        self.entries.values().map(|entry| entry.count).sum()
    }

    /// Suppresses the findings the baseline accepts and warns about entries with room to spare.
    ///
    /// `file` is the baseline path as the user gave it, for the warnings' location and hint.
    #[must_use]
    pub fn apply(&self, report: LintReport, file: &Path) -> Applied {
        let mut found: BTreeMap<(String, String), usize> = BTreeMap::new();
        for violation in report.violations.iter().filter(|v| !v.key.is_empty()) {
            *found
                .entry((violation.kind.name().to_owned(), violation.key.clone()))
                .or_default() += 1;
        }
        let mut kept = Vec::new();
        let mut suppressed = Vec::new();
        for mut violation in report.violations {
            let slot = (violation.kind.name().to_owned(), violation.key.clone());
            let seen = found.get(&slot).copied().unwrap_or(0);
            let allowed = self.entries.get(&slot).map_or(0, |entry| entry.count);
            if violation.key.is_empty() || (allowed == 0 && seen > 0) {
                kept.push(violation);
            } else if seen <= allowed {
                suppressed.push(violation);
            } else {
                // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
                let _ = write!(
                    violation.message,
                    "; the baseline allows {allowed}, found {seen}"
                );
                kept.push(violation);
            }
        }
        let stale: Vec<Violation> = self
            .entries
            .iter()
            .filter_map(|(slot, entry)| {
                let seen = found.get(slot).copied().unwrap_or(0);
                (seen < entry.count).then(|| stale_entry(slot, *entry, seen, file))
            })
            .collect();
        let outcome = BaselineOutcome {
            file: file.to_path_buf(),
            suppressed,
            stale: stale.len(),
        };
        kept.extend(stale);
        kept.sort();
        Applied {
            report: LintReport { violations: kept },
            outcome,
        }
    }

    /// The entries of `self` that are new or larger than in `old`, sorted by kind then key.
    #[must_use]
    pub fn added_since(&self, old: &Self) -> Vec<DebtAdded> {
        self.entries
            .iter()
            .filter_map(|((kind, key), entry)| {
                let before = old
                    .entries
                    .get(&(kind.clone(), key.clone()))
                    .map_or(0, |e| e.count);
                (entry.count > before).then(|| DebtAdded {
                    kind: kind.clone(),
                    key: key.clone(),
                    old: before,
                    new: entry.count,
                })
            })
            .collect()
    }
}

/// The `stale-baseline` warning for an entry that records more findings than `seen`.
fn stale_entry(slot: &(String, String), entry: Entry, seen: usize, file: &Path) -> Violation {
    let (kind, key) = slot;
    let message = if seen == 0 {
        format!(
            "baseline entry `{kind} {key}` records {} but no finding has that key now",
            entry.count
        )
    } else {
        format!(
            "baseline entry `{kind} {key}` records {} but only {seen} remain",
            entry.count
        )
    };
    Violation {
        kind: ViolationKind::StaleBaseline,
        severity: Severity::Warning,
        at: Some(Location {
            file: file.to_path_buf(),
            line: entry.line,
        }),
        message,
        hint: Some(format!(
            "run `driftwall lint --write-baseline {}` to shrink the baseline",
            path_key(file)
        )),
        test_only: false,
        key: String::new(),
        site: crate::results::SiteDetail::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_utf8_bom_is_ignored() {
        let plain = Baseline::parse("stale-edge 1 a -> b\n").unwrap();
        let bom = Baseline::parse("\u{feff}stale-edge 1 a -> b\n").unwrap();
        assert_eq!(bom, plain);
    }

    fn found(kind: ViolationKind, key: &str) -> Violation {
        Violation {
            kind,
            severity: Severity::Error,
            at: None,
            message: "m".to_owned(),
            hint: None,
            test_only: false,
            key: key.to_owned(),
            site: crate::results::SiteDetail::default(),
        }
    }

    fn report(items: &[(ViolationKind, &str)]) -> LintReport {
        LintReport {
            violations: items.iter().map(|(k, key)| found(*k, key)).collect(),
        }
    }

    #[test]
    fn parse_skips_comments_and_blank_lines_and_keeps_spaces_in_keys() {
        let text = "# c\n\nundeclared-edge 2 a --> b (test)\n";
        let baseline = Baseline::parse(text).unwrap();
        assert_eq!(baseline.len(), 1);
        assert_eq!(baseline.total(), 2);
        let again = Baseline::parse(&baseline.render()).unwrap();
        assert_eq!(again.render(), baseline.render());
    }

    #[test]
    fn malformed_lines_name_their_line() {
        for (text, line) in [
            ("# x\nbogus 1 k\n", 2),
            ("undeclared-edge x k\n", 1),
            ("undeclared-edge 0 k\n", 1),
            ("undeclared-edge 1\n", 1),
            ("undeclared-edge\n", 1),
            ("stale-baseline 1 k\n", 1),
            ("stale-map 1 g\nstale-map 2 g\n", 2),
        ] {
            let error = Baseline::parse(text).unwrap_err();
            assert_eq!(error.line(), line, "{text:?}");
        }
    }

    #[test]
    fn writing_is_sorted_counted_and_idempotent() {
        let all = report(&[
            (ViolationKind::UnmappedFile, "b.rs"),
            (ViolationKind::UndeclaredEdge, "x --> y"),
            (ViolationKind::UndeclaredEdge, "x --> y"),
            (ViolationKind::UnmappedFile, "a.rs"),
        ]);
        let text = Baseline::from_report(&all).render();
        assert!(
            text.ends_with(
                "undeclared-edge 2 x --> y\nunmapped-file 1 a.rs\nunmapped-file 1 b.rs\n"
            )
        );
        assert_eq!(Baseline::parse(&text).unwrap().render(), text);
    }

    #[test]
    fn keyless_findings_are_never_recorded() {
        let all = report(&[(ViolationKind::UnmappedFile, "")]);
        assert!(Baseline::from_report(&all).is_empty());
    }

    #[test]
    fn keyed_marks_a_test_site() {
        let mut v = found(ViolationKind::UndeclaredEdge, "");
        v.test_only = true;
        assert_eq!(v.keyed("a --> b").key, "a --> b (test)");
    }

    #[test]
    fn findings_within_the_record_are_suppressed() {
        let all = report(&[
            (ViolationKind::UndeclaredEdge, "a --> b"),
            (ViolationKind::UndeclaredEdge, "a --> b"),
        ]);
        let baseline = Baseline::from_report(&all);
        let applied = baseline.apply(all, Path::new("debt.txt"));
        assert_eq!(applied.report.violations.len(), 0);
        assert_eq!(applied.outcome.suppressed.len(), 2);
        assert_eq!(applied.outcome.stale, 0);
    }

    #[test]
    fn a_key_over_its_count_reports_every_finding() {
        let baseline = Baseline::parse("undeclared-edge 1 a --> b\n").unwrap();
        let now = report(&[
            (ViolationKind::UndeclaredEdge, "a --> b"),
            (ViolationKind::UndeclaredEdge, "a --> b"),
        ]);
        let applied = baseline.apply(now, Path::new("debt.txt"));
        assert_eq!(applied.report.violations.len(), 2);
        assert!(
            applied.report.violations[0]
                .message
                .contains("the baseline allows 1, found 2")
        );
        assert_eq!(applied.outcome.suppressed.len(), 0);
    }

    #[test]
    fn a_new_key_is_reported_and_keyless_findings_pass_through() {
        let baseline = Baseline::default();
        let now = report(&[
            (ViolationKind::UnmappedFile, "z.rs"),
            (ViolationKind::StaleMap, ""),
        ]);
        let applied = baseline.apply(now, Path::new("debt.txt"));
        assert_eq!(applied.report.violations.len(), 2);
        assert!(!applied.report.violations[0].message.contains("allows"));
    }

    #[test]
    fn unused_room_is_a_stale_warning_at_the_entry_line() {
        let baseline = Baseline::parse("# h\nunmapped-file 2 a.rs\nstale-map 1 g\n").unwrap();
        let now = report(&[(ViolationKind::UnmappedFile, "a.rs")]);
        let applied = baseline.apply(now, Path::new("debt.txt"));
        assert_eq!(applied.outcome.stale, 2);
        let warnings: Vec<_> = applied
            .report
            .violations
            .iter()
            .filter(|v| v.kind == ViolationKind::StaleBaseline)
            .collect();
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].severity, Severity::Warning);
        let at = warnings[0].at.as_ref().unwrap();
        assert_eq!((at.file.as_path(), at.line), (Path::new("debt.txt"), 2));
        assert!(
            warnings[0]
                .hint
                .as_ref()
                .unwrap()
                .contains("--write-baseline debt.txt")
        );
    }

    #[test]
    fn added_since_lists_new_and_grown_entries_only() {
        let old = Baseline::parse("stale-map 2 g\nunmapped-file 1 a\nstale-facade 1 f\n").unwrap();
        let new =
            Baseline::parse("stale-map 3 g\nunmapped-file 1 a\nstale-package 1 n e\n").unwrap();
        let added = new.added_since(&old);
        let shown: Vec<_> = added
            .iter()
            .map(|d| (d.kind.as_str(), d.old, d.new))
            .collect();
        assert_eq!(shown, [("stale-map", 2, 3), ("stale-package", 0, 1)]);
    }

    #[test]
    fn every_kind_has_a_unique_kebab_name() {
        let mut names: Vec<_> = BASELINABLE.iter().map(|k| k.name()).collect();
        names.push(ViolationKind::StaleBaseline.name());
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
