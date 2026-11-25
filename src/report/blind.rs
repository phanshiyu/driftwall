//! Renders what the analysis cannot see: the `lint` note, the `brief` block and the per node list.
//!
//! See "Blind spots" in `docs/DESIGN.md` for the exact forms.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::code::SkipReason;
use crate::results::{BlindRef, BlindSpots, BlindSummary, BlindTally};

use super::common::{capped, plural, site_str, skip_reason_str, slash_path};

/// The note `lint` and `init` print when there is something to say, or `None` when every count is
/// zero: `note: 50 files not read (swift 40, sh 10), 3 skipped references; see driftwall brief`.
///
/// Parts, each present only when non-zero and in this order: files not read (extensions by file
/// count, then name, the first six then `+N more`), files excluded (by reason), files ignored by
/// `.driftwallignore`, skipped references and unresolved imports. No trailing newline.
#[must_use]
pub fn render_blind_note(summary: &BlindSummary) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let unread: usize = summary.unread.values().map(|total| total.count).sum();
    if unread > 0 {
        let listed: Vec<String> = unread_by_count(summary)
            .into_iter()
            .map(|(key, total)| format!("{key} {}", total.count))
            .collect();
        parts.push(format!(
            "{} not read ({})",
            plural(unread, "file"),
            capped(listed.iter().map(String::as_str))
        ));
    }
    let excluded: usize = summary.excluded.values().sum();
    if excluded > 0 {
        let reasons: Vec<String> = summary
            .excluded
            .iter()
            .map(|(reason, count)| format!("{} {count}", reason.as_str()))
            .collect();
        parts.push(format!(
            "{} excluded ({})",
            plural(excluded, "file"),
            reasons.join(", ")
        ));
    }
    if summary.ignored > 0 {
        parts.push(format!(
            "{} ignored by .driftwallignore",
            plural(summary.ignored, "file")
        ));
    }
    let skipped: usize = summary.skipped.values().sum();
    if skipped > 0 {
        parts.push(plural(skipped, "skipped reference"));
    }
    if summary.unresolved > 0 {
        parts.push(plural(summary.unresolved, "unresolved import"));
    }
    (!parts.is_empty()).then(|| format!("note: {}; see driftwall brief", parts.join(", ")))
}

/// The unread keys with their totals, most files first and then by name.
fn unread_by_count(summary: &BlindSummary) -> Vec<(&String, &crate::results::UnreadTotal)> {
    let mut entries: Vec<_> = summary.unread.iter().collect();
    entries.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(b.0)));
    entries
}

/// How many skipped references of one reason the `brief` text lists before it says `+N more`.
const MAX_SKIPPED_PER_REASON: usize = 5;

/// How many unmapped items the `brief` text names before it says `+N more`.
const MAX_UNMAPPED_LISTED: usize = 20;

/// The block that ends the `brief` text: one line per category with a non-zero count, or the one
/// line `blind spots: none`. `unmapped` counts the items no node owns, over all categories, and
/// `unowned` is those items: the first [`MAX_UNMAPPED_LISTED`] follow the count, indented, then
/// `+N more`.
pub(super) fn summary_block(
    summary: &BlindSummary,
    unmapped: &BlindTally,
    unowned: &BlindSpots,
    skipped: &BTreeMap<SkipReason, Vec<BlindRef>>,
) -> String {
    if summary.is_empty() {
        return "blind spots: none\n".to_owned();
    }
    let mut lines = Vec::new();
    if !summary.unread.is_empty() {
        let listed: Vec<String> = unread_by_count(summary)
            .into_iter()
            .map(|(key, total)| {
                format!(
                    "{key} {} ({})",
                    plural(total.count, "file"),
                    plural(total.lines, "line")
                )
            })
            .collect();
        lines.push(format!("not read: {}", listed.join(", ")));
    }
    if !summary.excluded.is_empty() {
        let listed: Vec<String> = summary
            .excluded
            .iter()
            .map(|(reason, count)| format!("{} {count}", reason.as_str()))
            .collect();
        lines.push(format!("excluded: {}", listed.join(", ")));
    }
    if summary.ignored > 0 {
        lines.push(format!("ignored by .driftwallignore: {}", summary.ignored));
    }
    if !summary.skipped.is_empty() {
        let listed: Vec<String> = summary
            .skipped
            .iter()
            .map(|(reason, count)| format!("{} {count}", skip_reason_str(*reason)))
            .collect();
        lines.push(format!("skipped references: {}", listed.join(", ")));
        for (reason, refs) in skipped.iter().filter(|(_, refs)| !refs.is_empty()) {
            lines.push(format!("  {}:", skip_reason_str(*reason)));
            for reference in refs.iter().take(MAX_SKIPPED_PER_REASON) {
                lines.push(format!("    {}", skipped_entry(reference)));
            }
            if refs.len() > MAX_SKIPPED_PER_REASON {
                lines.push(format!(
                    "    +{} more, see --format json",
                    refs.len() - MAX_SKIPPED_PER_REASON
                ));
            }
        }
    }
    if summary.unresolved > 0 {
        lines.push(format!("unresolved imports: {}", summary.unresolved));
    }
    if unmapped.total() > 0 {
        lines.push(format!("unmapped: {}", unmapped.total()));
        let items = item_lines(unowned);
        lines.extend(
            items
                .iter()
                .take(MAX_UNMAPPED_LISTED)
                .map(|item| format!("  {item}")),
        );
        if items.len() > MAX_UNMAPPED_LISTED {
            lines.push(format!("  +{} more", items.len() - MAX_UNMAPPED_LISTED));
        }
    }
    block(&lines)
}

/// ` blind: 3 not read, 1 excluded, 2 ignored, 4 skipped, 1 unresolved` for a node line (only the
/// non-zero parts), or the empty string when every count is zero.
pub(super) fn node_suffix(tally: &BlindTally) -> String {
    let parts: Vec<String> = [
        (tally.unread, "not read"),
        (tally.excluded, "excluded"),
        (tally.ignored, "ignored"),
        (tally.skipped, "skipped"),
        (tally.unresolved, "unresolved"),
    ]
    .into_iter()
    .filter(|&(count, _)| count > 0)
    .map(|(count, name)| format!("{count} {name}"))
    .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" blind: {}", parts.join(", "))
    }
}

/// The `blind spots:` block of `neighbors`: one line per item, or `blind spots: none`.
pub(super) fn items_block(spots: &BlindSpots) -> String {
    let lines = item_lines(spots);
    if lines.is_empty() {
        return "blind spots: none\n".to_owned();
    }
    block(&lines)
}

/// One line per item of `spots`: not read, excluded, ignored, skipped, then unresolved.
fn item_lines(spots: &BlindSpots) -> Vec<String> {
    let mut lines = Vec::new();
    for (key, group) in &spots.unread {
        for file in &group.files {
            lines.push(format!(
                "not read: {} ({key}, {})",
                slash_path(&file.file),
                plural(file.lines.unwrap_or(0), "line")
            ));
        }
    }
    for (reason, files) in &spots.excluded {
        for file in files {
            lines.push(format!(
                "excluded: {} ({})",
                slash_path(&file.file),
                reason.as_str()
            ));
        }
    }
    for file in &spots.ignored {
        lines.push(format!("ignored: {}", slash_path(&file.file)));
    }
    for (reason, refs) in &spots.skipped {
        for reference in refs {
            lines.push(skipped_line(reference, skip_reason_str(*reason)));
        }
    }
    for reference in &spots.unresolved {
        lines.push(format!(
            "unresolved: {} {}",
            site_str(&reference.at),
            reference.specifier
        ));
    }
    lines
}

/// `blind spots:` followed by the lines, two spaces indented.
fn block(lines: &[String]) -> String {
    let mut out = String::from("blind spots:\n");
    for line in lines {
        // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
        let _ = writeln!(out, "  {line}");
    }
    out
}

fn skipped_line(reference: &BlindRef, reason: &str) -> String {
    format!(
        "skipped: {} {} ({reason}){}",
        site_str(&reference.at),
        reference.specifier,
        message_suffix(reference)
    )
}

/// `file:line specifier - message`, a skipped reference without its reason.
fn skipped_entry(reference: &BlindRef) -> String {
    format!(
        "{} {}{}",
        site_str(&reference.at),
        reference.specifier,
        message_suffix(reference)
    )
}

fn message_suffix(reference: &BlindRef) -> String {
    reference
        .message
        .as_ref()
        .map_or_else(String::new, |message| format!(" - {message}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::code::{Exclusion, Location, SkipReason};
    use crate::contract::NodeId;
    use crate::results::{BlindFile, UnreadGroup, UnreadTotal};

    fn summary(unread: &[(&str, usize, usize)]) -> BlindSummary {
        BlindSummary {
            unread: unread
                .iter()
                .map(|&(key, count, lines)| (key.to_owned(), UnreadTotal { count, lines }))
                .collect(),
            ..BlindSummary::default()
        }
    }

    #[test]
    fn nothing_blind_has_no_note() {
        assert_eq!(render_blind_note(&BlindSummary::default()), None);
    }

    #[test]
    fn the_note_orders_extensions_by_count_then_name() {
        let found = summary(&[("sh", 10, 1), ("swift", 40, 1), ("c", 10, 1)]);
        assert_eq!(
            render_blind_note(&found).as_deref(),
            Some("note: 60 files not read (swift 40, c 10, sh 10); see driftwall brief")
        );
    }

    #[test]
    fn the_note_cuts_extensions_after_six_and_counts_the_rest() {
        let found = summary(&[
            ("a", 9, 1),
            ("b", 8, 1),
            ("c", 7, 1),
            ("d", 6, 1),
            ("e", 5, 1),
            ("f", 4, 1),
            ("g", 3, 1),
            ("h", 2, 1),
        ]);
        assert_eq!(
            render_blind_note(&found).as_deref(),
            Some(
                "note: 44 files not read (a 9, b 8, c 7, d 6, e 5, f 4 +2 more); see driftwall brief"
            )
        );
    }

    #[test]
    fn the_note_names_every_part_in_order_and_is_singular_for_one() {
        let found = BlindSummary {
            excluded: [(Exclusion::Generated, 1)].into(),
            ignored: 1,
            skipped: [(SkipReason::EmbedNonSource, 1)].into(),
            unresolved: 1,
            ..summary(&[("swift", 1, 3)])
        };
        assert_eq!(
            render_blind_note(&found).as_deref(),
            Some(
                "note: 1 file not read (swift 1), 1 file excluded (generated 1), 1 file ignored by .driftwallignore, 1 skipped reference, 1 unresolved import; see driftwall brief"
            )
        );
    }

    #[test]
    fn the_summary_block_has_one_line_per_non_zero_category_and_omits_the_rest() {
        let found = BlindSummary {
            skipped: [
                (SkipReason::UndeclaredPackage, 5),
                (SkipReason::DynamicSpecifier, 1),
            ]
            .into(),
            ..summary(&[("sh", 1, 1), ("swift", 40, 1200)])
        };
        let unmapped = BlindTally {
            unread: 2,
            skipped: 1,
            ..BlindTally::default()
        };
        assert_eq!(
            summary_block(&found, &unmapped, &BlindSpots::default(), &BTreeMap::new()),
            "\
blind spots:
  not read: swift 40 files (1200 lines), sh 1 file (1 line)
  skipped references: undeclared-package 5, dynamic-specifier 1
  unmapped: 3
"
        );
    }

    #[test]
    fn an_empty_summary_is_the_single_line_none_whatever_the_tally() {
        assert_eq!(
            summary_block(
                &BlindSummary::default(),
                &BlindTally::default(),
                &BlindSpots::default(),
                &BTreeMap::new()
            ),
            "blind spots: none\n"
        );
    }

    #[test]
    fn a_node_suffix_lists_only_the_non_zero_counts_in_a_fixed_order() {
        let tally = BlindTally {
            unread: 3,
            excluded: 1,
            ignored: 2,
            skipped: 4,
            unresolved: 1,
        };
        assert_eq!(
            node_suffix(&tally),
            " blind: 3 not read, 1 excluded, 2 ignored, 4 skipped, 1 unresolved"
        );
        let only = BlindTally {
            skipped: 2,
            ..BlindTally::default()
        };
        assert_eq!(node_suffix(&only), " blind: 2 skipped");
        assert_eq!(node_suffix(&BlindTally::default()), "");
    }

    #[test]
    fn the_item_block_prints_one_line_per_item_with_the_skip_message() {
        let file = |path: &str, lines| BlindFile {
            file: PathBuf::from(path),
            node: Some(NodeId::new("a").unwrap()),
            lines,
        };
        let reference = |message: Option<&str>| BlindRef {
            at: Location {
                file: PathBuf::from("a/x.py"),
                line: 7,
            },
            specifier: "tool".to_owned(),
            node: None,
            message: message.map(str::to_owned),
        };
        let spots = BlindSpots {
            unread: [(
                "swift".to_owned(),
                UnreadGroup {
                    files: vec![file("a/V.swift", Some(1))],
                },
            )]
            .into(),
            excluded: [(Exclusion::Minified, vec![file("a/x.min.js", None)])].into(),
            ignored: vec![file("a/old.ts", None)],
            skipped: [(
                SkipReason::UnreachableModule,
                vec![reference(Some("matches x/tool.py"))],
            )]
            .into(),
            unresolved: vec![reference(None)],
        };
        assert_eq!(
            items_block(&spots),
            "\
blind spots:
  not read: a/V.swift (swift, 1 line)
  excluded: a/x.min.js (minified)
  ignored: a/old.ts
  skipped: a/x.py:7 tool (unreachable-module) - matches x/tool.py
  unresolved: a/x.py:7 tool
"
        );
        assert_eq!(items_block(&BlindSpots::default()), "blind spots: none\n");
    }
}
