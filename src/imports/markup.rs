//! Svelte files: the script elements of a markup file as regions of TypeScript or JavaScript.
//!
//! The scanner of `model::markup` finds the elements, see "Svelte and HTML" in `docs/DESIGN.md`.
//! A region keeps the positions of the file, so every line is a line of the markup file.

use std::path::Path;

use tree_sitter::{Point, Range};

use super::spec::{Grammar, JAVASCRIPT_GRAMMAR, Region, Scan, TYPESCRIPT_GRAMMAR};
use crate::model::markup::{self, Tag};

/// The scan of a `.svelte` file: every `<script>` element, parsed as TypeScript when its `lang` is
/// `ts` or `typescript` and as JavaScript otherwise.
pub fn svelte(source: &str, _file: &Path) -> Scan {
    let regions = markup::scan(source, true, &["script"])
        .iter()
        .filter_map(|tag| region(tag, language_of(tag)))
        .collect();
    Scan { regions }
}

/// The grammar a script's `lang` attribute asks for.
fn language_of(tag: &Tag) -> &'static Grammar {
    let typescript = tag.attribute("lang").is_some_and(|lang| {
        lang.trim().eq_ignore_ascii_case("ts") || lang.trim().eq_ignore_ascii_case("typescript")
    });
    if typescript {
        &TYPESCRIPT_GRAMMAR
    } else {
        &JAVASCRIPT_GRAMMAR
    }
}

/// The region holding the text of the script element `tag`, or `None` when it has none.
fn region(tag: &Tag, grammar: &'static Grammar) -> Option<Region> {
    let text = tag.text.filter(|text| text.end.byte > text.start.byte)?;
    Some(Region {
        grammar,
        range: Some(Range {
            start_byte: text.start.byte,
            end_byte: text.end.byte,
            start_point: Point::new(text.start.row, text.start.column),
            end_point: Point::new(text.end.row, text.end.column),
        }),
    })
}
