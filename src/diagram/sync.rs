//! Keeping a Markdown copy of a picture in sync, see "Contract picture" in `docs/DESIGN.md`:
//! `driftwall render --update FILE.md` and `--check FILE.md`.

use std::collections::BTreeSet;

use super::picture::{PictureError, picture};
use crate::contract::{Contract, NodeId};
use crate::results::{MarkdownSync, PictureView};

const START: &str = "<!-- driftwall render";
const END: &str = "<!-- driftwall render end -->";

/// What one line of the file is, as far as blocks go.
enum Marker {
    Start {
        view: PictureView,
        short: bool,
        built: bool,
    },
    End,
}

/// Reads a marker line, or `None` for any other line.
fn marker(text: &str, line: u32) -> Option<Result<Marker, PictureError>> {
    let text = text.trim();
    if text == END {
        return Some(Ok(Marker::End));
    }
    let args = text.strip_prefix(START)?;
    if !args.is_empty() && !args.starts_with(char::is_whitespace) {
        return None;
    }
    let args = args.strip_suffix("-->")?.trim();
    let bad = || PictureError::BadMarker {
        line,
        args: args.to_owned(),
    };
    let mut words = args.split_whitespace();
    let mut view = None;
    let mut short = false;
    let mut built = false;
    // why: the flags may come in any order, and each at most once.
    while let Some(word) = words.next() {
        let found = match word {
            "--short-labels" if !short => {
                short = true;
                continue;
            }
            "--built" if !built => {
                built = true;
                continue;
            }
            "--depth" => words
                .next()
                .and_then(|depth| depth.parse::<u32>().ok())
                .map(PictureView::Depth),
            "--focus" => words.next().map(|id| PictureView::Focus(id.to_owned())),
            _ => None,
        };
        match (found, view.is_some()) {
            (Some(found), false) => view = Some(found),
            _ => return Some(Err(bad())),
        }
    }
    Some(Ok(Marker::Start {
        view: view.unwrap_or(PictureView::Full),
        short,
        built,
    }))
}

/// Whether some block of `markdown` has `--built` in its marker, so drawing it needs the
/// repository's ownership. A marker that is malformed does not count: [`sync`] reports it.
#[must_use]
pub fn wants_built(markdown: &str) -> bool {
    markdown.lines().zip(1_u32..).any(|(raw, line)| {
        matches!(
            marker(raw, line),
            Some(Ok(Marker::Start { built: true, .. }))
        )
    })
}

/// A block that has started: its line, its view and its line ending.
struct Open {
    line: u32,
    view: PictureView,
    short: bool,
    built: bool,
    eol: &'static str,
}

/// Rewrites the inside of every block of `markdown` with the picture its marker asks for.
///
/// The pictures are drawn from `contract` (`source` is the contract file name). `owning` is the
/// set of nodes that own a source file, which a block with `--built` needs.
///
/// # Errors
/// A start with no end, an end with no start, a start inside a block, unknown arguments in a
/// marker, no block at all, a block with `--built` when `owning` is `None`, or a picture that
/// cannot be drawn (depth 0, a focus that is no boundary).
pub fn sync(
    markdown: &str,
    contract: &Contract,
    owning: Option<&BTreeSet<NodeId>>,
    source: &str,
) -> Result<MarkdownSync, PictureError> {
    let mut text = String::new();
    let mut stale = Vec::new();
    let mut open: Option<Open> = None;
    let mut inside = String::new();
    let mut blocks = 0_usize;
    for (index, raw) in markdown.split_inclusive('\n').enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let found = marker(raw, line).transpose()?;
        match (found, open.take()) {
            (Some(Marker::Start { .. }), Some(block)) => {
                return Err(PictureError::Nested {
                    line,
                    first: block.line,
                });
            }
            (Some(Marker::Start { view, short, built }), None) => {
                text.push_str(raw);
                inside.clear();
                let eol = if raw.ends_with("\r\n") { "\r\n" } else { "\n" };
                open = Some(Open {
                    line,
                    view,
                    short,
                    built,
                    eol,
                });
            }
            (Some(Marker::End), None) => return Err(PictureError::StrayEnd { line }),
            (Some(Marker::End), Some(block)) => {
                let built = match (block.built, owning) {
                    (true, None) => return Err(PictureError::NeedsOwnership { line: block.line }),
                    (true, Some(_)) => owning,
                    (false, _) => None,
                };
                let drawn = picture(contract, &block.view, block.short, built, source)?;
                let fresh = format!("```mermaid\n{drawn}```\n").replace('\n', block.eol);
                if fresh != inside {
                    stale.push(block.line);
                }
                text.push_str(&fresh);
                text.push_str(raw);
                blocks += 1;
            }
            (None, Some(block)) => {
                inside.push_str(raw);
                open = Some(block);
            }
            (None, None) => text.push_str(raw),
        }
    }
    if let Some(block) = open {
        return Err(PictureError::Unclosed { line: block.line });
    }
    if blocks == 0 {
        return Err(PictureError::NoBlock);
    }
    Ok(MarkdownSync { text, stale })
}
