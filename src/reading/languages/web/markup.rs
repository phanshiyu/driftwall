//! Svelte and HTML files: the script elements of a markup file as regions of TypeScript or
//! JavaScript, and the files an HTML page loads through attributes.
//!
//! The scanner in `markup` finds the elements, see "Svelte and HTML" in `docs/DESIGN.md`.
//! A region keeps the positions of the file, so every line is a line of the markup file.

use std::path::{Path, PathBuf};

use tree_sitter::{Point, Range};

use super::env::Env;
use super::package::MANIFEST_FILE;
use super::spec::{JAVASCRIPT_GRAMMAR, TYPESCRIPT_GRAMMAR};
use super::{resolve, symbols};
use crate::code::SkipReason;
use crate::markup::{self, Tag};
use crate::reading::syntax::literal::literal_text;
use crate::reading::syntax::paths;
use crate::reading::syntax::resolution::{Files, Reference, Resolution};
use crate::reading::syntax::spec::{Grammar, Load, Region, Scan};

/// The values of a script's `type` that make it JavaScript, besides no `type` at all.
const SCRIPT_TYPES: [&str; 3] = ["module", "text/javascript", "application/javascript"];

/// The `rel` tokens of a `<link>` whose `href` is a resource the page uses, see "Svelte and HTML".
const ASSET_RELS: &[&str] = &[
    "stylesheet",
    "preload",
    "prefetch",
    "icon",
    "apple-touch-icon",
    "manifest",
];

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
        let (path, asset) = match tag.name.as_str() {
            "script" if is_javascript(&tag) => (tag.attribute("src"), false),
            "link" if has_rel(&tag, &["modulepreload"]) => (tag.attribute("href"), false),
            "link" if has_rel(&tag, ASSET_RELS) => (tag.attribute("href"), true),
            _ => (None, false),
        };
        if let Some(path) = path {
            scan.loads.push(Load {
                specifier: path.trim().to_owned(),
                line: tag.line,
                asset,
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

/// Whether the `rel` list of the `<link>` `tag` holds one of `wanted`.
fn has_rel(tag: &Tag, wanted: &[&str]) -> bool {
    tag.attribute("rel").is_some_and(|rel| {
        rel.split_ascii_whitespace()
            .any(|token| wanted.iter().any(|name| token.eq_ignore_ascii_case(name)))
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
    files: &Files<'_>,
    env: &Env<'_>,
) -> Vec<Resolution> {
    let specifier = reference.specifier;
    if literal_text(reference.node, reference.source).is_none() {
        return resolve::ecmascript(reference, importer, files, env);
    }
    if is_remote(specifier) {
        return vec![Resolution::Ignored];
    }
    if is_root_absolute(specifier) {
        let symbols = symbols::used(reference.node, reference.source);
        let resolution = root_absolute(specifier, importer, files, env);
        return vec![resolve::with_query_kind(specifier, resolution).using(symbols)];
    }
    resolve::ecmascript(reference, importer, files, env)
}

/// Resolves the `src` or `href` of `load`, written in the HTML file `importer`. It is a
/// document-relative URL: `main.js` means `./main.js`. A resource link (see [`Load::asset`]) turns a
/// source file it finds into an asset, and a relative path that finds nothing is ignored when the
/// file is below `public` of the Vite root, where Vite serves it from.
pub fn resolve_load(load: &Load, importer: &Path, files: &Files<'_>, env: &Env<'_>) -> Resolution {
    let path = resolve::strip_suffix(&load.specifier);
    if path.is_empty() || path.starts_with('#') || is_remote(path) {
        return Resolution::Ignored;
    }
    let resolution = if is_root_absolute(path) {
        root_absolute(path, importer, files, env)
    } else {
        let relative =
            if path == "." || path == ".." || path.starts_with("./") || path.starts_with("../") {
                path.to_owned()
            } else {
                format!("./{path}")
            };
        resolve::resolve(&relative, importer, files)
    };
    match resolution {
        Resolution::Resolved { target, .. } if load.asset => Resolution::Asset { target },
        Resolution::Unresolved if load.asset && in_public(path, importer, files, env) => {
            Resolution::Ignored
        }
        other => other,
    }
}

/// Whether the document-relative `path` of the page `importer` names a file below `public` of the
/// Vite root: the path joined onto the directory of the page, with the root taken off and `public`
/// put in its place.
fn in_public(path: &str, importer: &Path, files: &Files<'_>, env: &Env<'_>) -> bool {
    let root = vite_root(importer, files, env);
    let Some(target) = paths::join_normalised(importer.parent(), path) else {
        return false;
    };
    target
        .strip_prefix(&root)
        .is_ok_and(|below| resolve::find(&root.join("public").join(below), false, files).is_some())
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

/// Resolves the root-absolute `specifier` against the Vite root of `importer`. A file Vite serves
/// from `<root>/public` is ignored; a path that finds no file at all is skipped as
/// [`SkipReason::UnresolvedRootPath`], never dropped.
fn root_absolute(specifier: &str, importer: &Path, files: &Files<'_>, env: &Env<'_>) -> Resolution {
    let path = resolve::strip_suffix(specifier);
    let below = path.trim_start_matches('/');
    let root = vite_root(importer, files, env);
    let find = |dir: &Path| {
        paths::join_normalised(Some(dir), below)
            .and_then(|base| resolve::find(&base, path.ends_with('/'), files))
    };
    if let Some(found) = find(&root) {
        return found;
    }
    if find(&root.join("public")).is_some() {
        return Resolution::Ignored;
    }
    let shown = if root.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        root.display().to_string()
    };
    Resolution::SkippedWith {
        reason: SkipReason::UnresolvedRootPath,
        message: format!("`{path}` finds no file below the Vite root `{shown}`"),
    }
}

/// The Vite root of the HTML file `importer`: the root of the nearest Vite config above it, else
/// its nearest ancestor directory that holds a `package.json`, else the repository root. The file's
/// own directory is the first one looked at.
fn vite_root(importer: &Path, files: &Files<'_>, env: &Env<'_>) -> PathBuf {
    let directories = || importer.ancestors().skip(1);
    directories()
        .find_map(|dir| env.vite_roots.get(dir).cloned())
        .or_else(|| {
            directories()
                .find(|dir| files.assets.contains(&dir.join(MANIFEST_FILE)))
                .map(Path::to_path_buf)
        })
        .unwrap_or_default()
}

/// Whether `source`, an HTML file, holds a `<script` element, which makes it a source file.
#[cfg(test)]
pub fn has_script(source: &str) -> bool {
    markup::has_script(source)
}
