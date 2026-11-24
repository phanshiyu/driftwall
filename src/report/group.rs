//! Grouping of findings for the text output (see "Grouped findings" in `docs/DESIGN.md`).
//!
//! Findings that share a kind and a baseline key are one problem seen at several import sites, so
//! the text prints them once with the sites listed. JSON and SARIF never go through here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::model::{LintReport, Violation, ViolationKind};

use super::common::{kind_str, plural, severity_str, slash_path};

/// The separator `Baseline::apply` puts before its note, which a group shows once.
const BASELINE_NOTE: &str = "; the baseline allows ";

/// How many blocks were printed and whether any of them held more than one finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Printed {
    pub blocks: usize,
    pub grouped: bool,
}

/// Writes every finding of `report` to `out`, a group of findings that share a key as one block.
pub(super) fn write_blocks(out: &mut String, report: &LintReport) -> Printed {
    let mut sites: BTreeMap<(ViolationKind, &str), Vec<&Violation>> = BTreeMap::new();
    for found in report.violations.iter().filter(|v| groupable(v)) {
        sites
            .entry((found.kind, &found.key))
            .or_default()
            .push(found);
    }
    let mut done: BTreeSet<(ViolationKind, &str)> = BTreeSet::new();
    let mut printed = Printed {
        blocks: 0,
        grouped: false,
    };
    for found in &report.violations {
        let slot = (found.kind, found.key.as_str());
        match sites.get_mut(&slot) {
            Some(members) if groupable(found) && members.len() > 1 => {
                if done.insert(slot) {
                    members.sort_by_key(|v| v.at.as_ref().map(|at| (at.file.clone(), at.line)));
                    write_group(out, members);
                    printed.blocks += 1;
                    printed.grouped = true;
                }
            }
            _ => {
                write_single(out, found);
                printed.blocks += 1;
            }
        }
    }
    printed
}

/// Whether a finding can join a group: it has a key and a place.
const fn groupable(found: &Violation) -> bool {
    !found.key.is_empty() && found.at.is_some()
}

/// One finding on its own, as `lint` always printed it.
fn write_single(out: &mut String, found: &Violation) {
    write_head(out, found, &found.message);
    write_hint(out, found);
}

/// `file:line: error[kind] (test code): message`, the start of every block.
fn write_head(out: &mut String, found: &Violation, message: &str) {
    // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
    if let Some(at) = &found.at {
        let _ = write!(out, "{}:{}: ", slash_path(&at.file), at.line);
    }
    let mark = if found.test_only { " (test code)" } else { "" };
    let _ = writeln!(
        out,
        "{}[{}]{mark}: {message}",
        severity_str(found.severity),
        kind_str(found.kind)
    );
}

fn write_hint(out: &mut String, found: &Violation) {
    if let Some(hint) = &found.hint {
        // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
        let _ = writeln!(out, "  hint: {hint}");
    }
}

/// A group: the header, one `at` line per site, and the hint once. `members` is sorted by place
/// and has at least two findings.
fn write_group(out: &mut String, members: &[&Violation]) {
    let Some(&first) = members.first() else {
        return;
    };
    write_head(out, first, &header_message(first, members.len()));
    for site in members {
        // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
        if let Some(at) = &site.at {
            let _ = write!(out, "  at {}:{}", slash_path(&at.file), at.line);
        }
        let detail: Vec<String> = [
            site.site.target.as_deref().map(slash_path),
            site.site.symbol.as_ref().map(|name| format!("`{name}`")),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !detail.is_empty() {
            let _ = write!(out, " ({})", detail.join(", "));
        }
        out.push('\n');
    }
    write_hint(out, first);
}

/// The first site's message with what is specific to one site, the target file, replaced by
/// `in N places`. The baseline note stays once, at the end.
fn header_message(first: &Violation, count: usize) -> String {
    let places = format!("in {count} places");
    let (body, note) = first
        .message
        .rfind(BASELINE_NOTE)
        .map_or((first.message.as_str(), ""), |at| {
            first.message.split_at(at)
        });
    let general = generalise(first, body, &places).unwrap_or_else(|| {
        // why: a message the generaliser does not know still has to read as a group.
        format!("{body} ({})", plural(count, "place"))
    });
    format!("{general}{note}")
}

/// `body` with the target of `first` rewritten to `places`, per kind; `None` when the kind has no
/// rewrite or the message does not have the shape the rewrite expects.
fn generalise(first: &Violation, body: &str, places: &str) -> Option<String> {
    let target = first.site.target.as_deref().map(slash_path);
    let rewrite =
        |from: String, to: String| body.contains(&from).then(|| body.replacen(&from, &to, 1));
    match (first.kind, target) {
        (ViolationKind::UndeclaredEdge | ViolationKind::PrivateSymbol, Some(t)) => {
            rewrite(format!(" ({t}) "), format!(" {places} "))
        }
        (ViolationKind::UpwardEdge, Some(t)) => rewrite(format!(", {t}) "), format!(") {places} ")),
        (ViolationKind::IsolationBreach, Some(t)) => {
            rewrite(format!(", {t}),"), format!(") {places},"))
        }
        (ViolationKind::ProductionImportsTest, Some(t)) => {
            rewrite(format!("`{t}`,"), format!("`{t}` {places},"))
        }
        (ViolationKind::ForbiddenPackage, _) => {
            let key = first.key.strip_suffix(" (test)").unwrap_or(&first.key);
            let package = key.rsplit(' ').next()?;
            rewrite(
                format!("imports `{package}` "),
                format!("imports `{package}` {places} "),
            )
        }
        _ => None,
    }
}
