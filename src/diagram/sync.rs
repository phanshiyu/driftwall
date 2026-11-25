//! Keeping a Markdown copy of a picture in sync, see "Contract picture" in `docs/DESIGN.md`:
//! `driftwall render --update FILE.md` and `--check FILE.md`.

use super::picture::{PictureError, picture};
use crate::model::{Contract, MarkdownSync, PictureView};

const START: &str = "<!-- driftwall render";
const END: &str = "<!-- driftwall render end -->";

/// What one line of the file is, as far as blocks go.
enum Marker {
    Start(PictureView),
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
    let words: Vec<&str> = args.split_whitespace().collect();
    Some(match words.as_slice() {
        [] => Ok(Marker::Start(PictureView::Full)),
        ["--depth", depth] => depth
            .parse::<u32>()
            .map(|depth| Marker::Start(PictureView::Depth(depth)))
            .map_err(|_| bad()),
        ["--focus", boundary] => Ok(Marker::Start(PictureView::Focus((*boundary).to_owned()))),
        _ => Err(bad()),
    })
}

/// A block that has started: its line, its view and its line ending.
struct Open {
    line: u32,
    view: PictureView,
    eol: &'static str,
}

/// Rewrites the inside of every block of `markdown` with the picture its marker asks for, from
/// `contract` (`source` is the contract file name).
///
/// # Errors
/// A start with no end, an end with no start, a start inside a block, unknown arguments in a
/// marker, no block at all, or a picture that cannot be drawn (depth 0, a focus that is no
/// boundary).
pub fn sync(
    markdown: &str,
    contract: &Contract,
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
            (Some(Marker::Start(_)), Some(block)) => {
                return Err(PictureError::Nested {
                    line,
                    first: block.line,
                });
            }
            (Some(Marker::Start(view)), None) => {
                text.push_str(raw);
                inside.clear();
                let eol = if raw.ends_with("\r\n") { "\r\n" } else { "\n" };
                open = Some(Open { line, view, eol });
            }
            (Some(Marker::End), None) => return Err(PictureError::StrayEnd { line }),
            (Some(Marker::End), Some(block)) => {
                let drawn = picture(contract, &block.view, source)?;
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
