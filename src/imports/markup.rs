//! Svelte and HTML files: the script elements of a markup file as regions of TypeScript or
//! JavaScript, and the files an HTML page loads through attributes.
//!
//! The scanner in `markup` finds the elements, see "Svelte and HTML" in `docs/DESIGN.md`.
//! A region keeps the positions of the file, so every line is a line of the markup file.

use std::path::{Path, PathBuf};

use tree_sitter::{Point, Range};

use super::extract::literal_text;
use super::package::MANIFEST_FILE;
use super::spec::{Grammar, JAVASCRIPT_GRAMMAR, Load, Region, Scan, TYPESCRIPT_GRAMMAR};
use super::{Environment, Reference, Resolution, ecmascript_symbols, resolve};
use crate::markup::{self, Tag};

/// The values of a script's `type` that make it JavaScript, besides no `type` at all.
const SCRIPT_TYPES: [&str; 3] = ["module", "text/javascript", "application/javascript"];

/// The scan of a `.svelte` file: every `<script>` element, parsed as TypeScript when its `lang` is
/// `ts` or `typescript` and as JavaScript otherwise.
pub fn svelte(source: &str, _file: &Path) -> Scan {
    let regions = markup::scan(source, true, &["script"])
        .iter()
        .filter_map(|tag| region(tag, language_of(tag)))
        .collect();
    Scan {
        regions,
        loads: Vec::new(),
    }
}

/// The scan of an HTML file: the inline scripts of JavaScript type as JavaScript regions, and the
/// `src` of a script and the `href` of a module preload as loads.
pub fn html(source: &str, _file: &Path) -> Scan {
    let mut scan = Scan {
        regions: Vec::new(),
        loads: Vec::new(),
    };
    for tag in markup::scan(source, false, &["script", "link"]) {
        let path = match tag.name.as_str() {
            "script" if is_javascript(&tag) => tag.attribute("src"),
            "link" if is_module_preload(&tag) => tag.attribute("href"),
            _ => None,
        };
        if let Some(path) = path {
            scan.loads.push(Load {
                specifier: path.trim().to_owned(),
                line: tag.line,
            });
        } else if tag.name == "script" && is_javascript(&tag) {
            scan.regions.extend(region(&tag, &JAVASCRIPT_GRAMMAR));
        }
    }
    scan
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

/// Whether the `type` of the script `tag` is absent, empty or one of [`SCRIPT_TYPES`].
fn is_javascript(tag: &Tag) -> bool {
    tag.attribute("type").is_none_or(|kind| {
        let kind = kind.trim();
        kind.is_empty()
            || SCRIPT_TYPES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(kind))
    })
}

/// Whether the `rel` list of the `<link>` `tag` holds `modulepreload`.
fn is_module_preload(tag: &Tag) -> bool {
    tag.attribute("rel").is_some_and(|rel| {
        rel.split_ascii_whitespace()
            .any(|token| token.eq_ignore_ascii_case("modulepreload"))
    })
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

/// The table entry for an import inside an HTML script: TypeScript and JavaScript resolution, except
/// that a remote URL is ignored and a root-absolute specifier is a path below the Vite root.
pub fn resolve_inline(
    reference: &Reference<'_>,
    importer: &Path,
    env: &Environment<'_>,
) -> Vec<Resolution> {
    let specifier = reference.specifier;
    if literal_text(reference.node, reference.source).is_none() {
        return resolve::ecmascript(reference, importer, env);
    }
    if is_remote(specifier) {
        return vec![Resolution::Ignored];
    }
    if is_root_absolute(specifier) {
        let symbols = ecmascript_symbols::used(reference.node, reference.source);
        return vec![root_absolute(specifier, importer, env).using(symbols)];
    }
    resolve::ecmascript(reference, importer, env)
}

/// Resolves the `src` or `href` written in the HTML file `importer`. It is a document-relative
/// URL: `main.js` means `./main.js`.
pub fn resolve_load(specifier: &str, importer: &Path, env: &Environment<'_>) -> Resolution {
    let path = resolve::strip_suffix(specifier.trim());
    if path.is_empty() || path.starts_with('#') || is_remote(path) {
        return Resolution::Ignored;
    }
    if is_root_absolute(path) {
        return root_absolute(path, importer, env);
    }
    let relative =
        if path == "." || path == ".." || path.starts_with("./") || path.starts_with("../") {
            path.to_owned()
        } else {
            format!("./{path}")
        };
    resolve::resolve(&relative, importer, env)
}

/// Whether `specifier` is a URL on another origin: it starts with `//`, or with a scheme (a
/// letter, then letters, digits, `+`, `-` or `.`, then `:`).
fn is_remote(specifier: &str) -> bool {
    if specifier.starts_with("//") {
        return true;
    }
    let Some((scheme, _)) = specifier.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Whether `specifier` starts with a single `/`.
fn is_root_absolute(specifier: &str) -> bool {
    specifier.starts_with('/') && !specifier.starts_with("//")
}

/// Resolves the root-absolute `specifier` against the Vite root of `importer`. A path that finds no
/// file is ignored: it may be served from `public` or by the dev server.
fn root_absolute(specifier: &str, importer: &Path, env: &Environment<'_>) -> Resolution {
    let path = resolve::strip_suffix(specifier);
    resolve::join_normalised(
        Some(&vite_root(importer, env)),
        path.trim_start_matches('/'),
    )
    .and_then(|base| resolve::find(&base, path.ends_with('/'), env))
    .unwrap_or(Resolution::Ignored)
}

/// The Vite project root of the HTML file `importer`: its nearest ancestor directory, its own
/// directory first, that holds a `package.json`, else the repository root.
fn vite_root(importer: &Path, env: &Environment<'_>) -> PathBuf {
    importer
        .ancestors()
        .skip(1)
        .find(|dir| env.assets.contains(&dir.join(MANIFEST_FILE)))
        .map(Path::to_path_buf)
        .unwrap_or_default()
}
