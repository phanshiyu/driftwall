//! Parsing a single file and turning query matches into imports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use super::spec::{Grammar, LanguageSpec, Region};
use super::statement::Flags;
use super::{
    Environment, FileImports, ImportsError, PythonLayout, Reference, Resolution, Workspaces, spec,
};
use crate::model::{
    ImportKind, Language, Location, ResolvedImport, SkipReason, Skipped, Symbol, UnresolvedImport,
};

/// What duplicate imports must agree on to merge: location, target, kind and whether it is test code.
type Merge = (Location, PathBuf, ImportKind, bool);

const SPECIFIER_CAPTURE: &str = "specifier";
const NAME_CAPTURE: &str = "name";

/// Extracts and resolves the imports of one file. Pure: it never touches the filesystem.
///
/// `file` is the repository-relative path of the importing file and `known` holds the
/// repository-relative paths of every repository file. A language with no table entry yields no
/// imports. Syntax errors are tolerated: whatever imports tree-sitter can still see are returned.
/// No library crates are known, so Rust paths that start with a crate name are not followed; use
/// [`extract_in`] to supply them.
///
/// # Errors
/// Returns [`ImportsError::Grammar`] or [`ImportsError::Query`] when a table entry is internally
/// inconsistent.
pub fn extract(
    source: &str,
    language: Language,
    file: &Path,
    known: &BTreeSet<PathBuf>,
) -> Result<FileImports, ImportsError> {
    let crates = BTreeMap::new();
    let aliases = BTreeMap::new();
    let python = PythonLayout::new(known, BTreeMap::new());
    extract_in(
        source,
        language,
        file,
        &Environment {
            known,
            assets: &BTreeSet::new(),
            crates: &crates,
            aliases: &aliases,
            packages: &BTreeMap::new(),
            workspaces: &Workspaces::default(),
            projects: &BTreeMap::new(),
            python: &python,
        },
    )
}

/// [`extract`] with an explicit [`Environment`]: the known repository files and library crates.
///
/// # Errors
/// Returns [`ImportsError::Grammar`] or [`ImportsError::Query`] when a table entry is internally
/// inconsistent.
pub fn extract_in(
    source: &str,
    language: Language,
    file: &Path,
    env: &Environment<'_>,
) -> Result<FileImports, ImportsError> {
    let Some(spec) = spec::spec(language) else {
        return Ok(FileImports::default());
    };
    let mut found = Found::default();
    for region in (spec.scan)(source, file).regions {
        found_in(&region, spec, source, file, env, &mut found)?;
    }
    let Found {
        resolved,
        unresolved,
        skipped,
    } = found;
    let mut resolved: Vec<ResolvedImport> = resolved
        .into_iter()
        .map(|((at, target, kind, test_only), symbols)| ResolvedImport {
            at,
            target,
            symbols,
            kind,
            test_only,
        })
        .collect();
    // why: the map is ordered by its merge key; the documented order puts symbols before kind.
    resolved.sort();
    Ok(FileImports {
        resolved,
        unresolved: unresolved.into_iter().collect(),
        skipped: skipped.into_iter().collect(),
    })
}

/// Parses `region` of `source`, the text of `file`, and files every import it holds into `found`.
fn found_in(
    region: &Region,
    spec: &LanguageSpec,
    source: &str,
    file: &Path,
    env: &Environment<'_>,
    found: &mut Found,
) -> Result<(), ImportsError> {
    let grammar = region.grammar;
    let query = compiled_query(grammar, grammar.query)?;
    let mut parser = Parser::new();
    parser.set_language(&(grammar.language)())?;
    if let Some(range) = region.range {
        // why: a range taken from the text being parsed is always accepted; a refusal means nothing to read.
        if parser.set_included_ranges(&[range]).is_err() {
            return Ok(());
        }
    }
    // why: no timeout or cancellation flag is configured, so a parse only fails on a runtime fault.
    let Some(tree) = parser.parse(source, None) else {
        return Ok(());
    };
    let Some(capture) = query.capture_index_for_name(SPECIFIER_CAPTURE) else {
        return Ok(());
    };

    let name_capture = query.capture_index_for_name(NAME_CAPTURE);
    let bindings = (spec.bindings)(tree.root_node(), source);

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(hit) = matches.next() {
        let Some(node) = hit
            .captures()
            .iter()
            .find(|c| c.index == capture)
            .map(|c| c.node)
        else {
            continue;
        };
        let Some(specifier) = specifier_text(node, source) else {
            continue;
        };
        let names: Vec<&str> = hit
            .captures()
            .iter()
            .filter(|c| Some(c.index) == name_capture)
            .filter_map(|c| c.node.utf8_text(source.as_bytes()).ok())
            .collect();
        let anchor = (spec.anchor)(node);
        let at = Location {
            file: file.to_path_buf(),
            line: line_of(anchor),
        };
        let flags = (spec.flags)(anchor, source, file);
        let reference = Reference {
            specifier,
            names: &names,
            node,
            source,
            bindings: &bindings,
        };
        for resolution in (spec.resolve)(&reference, file, env) {
            found.record(resolution, &at, specifier, flags);
        }
    }
    Ok(())
}

/// What the resolutions of one file add up to.
#[derive(Default)]
struct Found {
    // why: keyed by (location, target, kind, test_only): duplicates merge only when all four agree.
    resolved: BTreeMap<Merge, BTreeSet<Symbol>>,
    unresolved: BTreeSet<UnresolvedImport>,
    skipped: BTreeSet<Skipped>,
}

impl Found {
    /// Files one resolution of the reference written as `specifier` at `at`.
    fn record(&mut self, resolution: Resolution, at: &Location, specifier: &str, flags: Flags) {
        match resolution {
            Resolution::Resolved { target, symbols } => {
                let merge = (at.clone(), target, flags.kind, flags.test_only);
                self.resolved.entry(merge).or_default().extend(symbols);
            }
            Resolution::Unresolved => {
                self.unresolved.insert(UnresolvedImport {
                    at: at.clone(),
                    specifier: specifier.to_owned(),
                });
            }
            Resolution::Embedded { target } => {
                let merge = (at.clone(), target, ImportKind::Embed, flags.test_only);
                self.resolved.entry(merge).or_default();
            }
            Resolution::Asset { target } => {
                let merge = (at.clone(), target, ImportKind::Asset, flags.test_only);
                self.resolved.entry(merge).or_default();
            }
            Resolution::Skipped(reason) => self.skip(at, specifier, reason, None),
            Resolution::Unreachable { message } => {
                self.skip(at, specifier, SkipReason::UnreachableModule, Some(message));
            }
            Resolution::Ignored => {}
        }
    }

    fn skip(
        &mut self,
        at: &Location,
        specifier: &str,
        reason: SkipReason,
        message: Option<String>,
    ) {
        self.skipped.insert(Skipped {
            at: at.clone(),
            specifier: specifier.to_owned(),
            reason,
            message,
        });
    }
}

/// The text of a captured specifier: the contents of a plain literal, else the source text of the
/// node, such as a Python module path or the computed argument of `require(..)`.
fn specifier_text<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    literal_text(node, source).or_else(|| node.utf8_text(source.as_bytes()).ok())
}

/// The contents of `node` when it is a plain literal: a string, a template without
/// substitutions, a Rust string or a Python string (see `python_literal`), written without escapes. `None` for anything else, which is
/// computed or cannot be read without unescaping.
pub(super) fn literal_text<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    if node.kind() == "string" && node.child(0).is_some_and(|c| c.kind() == "string_start") {
        return python_literal(node, source);
    }
    let content = match node.kind() {
        "string" | "template_string" => "string_fragment",
        "string_literal" => "string_content",
        _ => return None,
    };
    match node.named_child_count() {
        0 => Some(""),
        1 => {
            let fragment = node.named_child(0)?;
            if fragment.kind() == content {
                fragment.utf8_text(source.as_bytes()).ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The contents of a Python string literal: no prefix but `r` or `u`, no escape sequence, no
/// interpolation and a single piece of text. Adjacent literals are a different node and never get
/// here. `None` for anything else.
fn python_literal<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    let start = node.child(0)?.utf8_text(source.as_bytes()).ok()?;
    let prefix = start.trim_end_matches(['"', '\'']);
    if !(prefix.is_empty() || prefix.eq_ignore_ascii_case("r") || prefix.eq_ignore_ascii_case("u"))
    {
        return None;
    }
    let mut cursor = node.walk();
    let mut content = None;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "string_start" | "string_end" => {}
            "string_content" if child.named_child_count() == 0 && content.is_none() => {
                content = Some(child);
            }
            _ => return None,
        }
    }
    content.map_or(Some(""), |text| text.utf8_text(source.as_bytes()).ok())
}

/// The text of a string literal made of a single fragment, so escapes and templates never produce
/// a half-read specifier.
pub(super) fn plain_string<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    if node.kind() != "string" || node.named_child_count() != 1 {
        return None;
    }
    let fragment = node.named_child(0)?;
    if fragment.kind() != "string_fragment" {
        return None;
    }
    fragment.utf8_text(source.as_bytes()).ok()
}

fn line_of(node: Node<'_>) -> u32 {
    u32::try_from(node.start_position().row).map_or(u32::MAX, |row| row.saturating_add(1))
}

type QueryCache = Mutex<BTreeMap<&'static str, Arc<Query>>>;

/// Compiles each distinct (grammar, query) pair once for the life of the process. TSX and plain
/// TypeScript share query text but need separate compiled queries.
fn compiled_query(grammar: &Grammar, text: &str) -> Result<Arc<Query>, ImportsError> {
    static CACHE: std::sync::OnceLock<QueryCache> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(QueryCache::default);
    let key = grammar.name;
    let cached = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .cloned();
    if let Some(query) = cached {
        return Ok(query);
    }
    // why: compiled outside the lock; two threads racing here both compile and the later insert wins.
    let query = Arc::new(Query::new(&(grammar.language)(), text)?);
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, Arc::clone(&query));
    Ok(query)
}

#[cfg(test)]
mod cache_key_tests {
    use super::super::spec::{Scan, spec};
    use crate::model::Language;
    use std::path::Path;

    #[test]
    fn grammar_names_are_unique_per_grammar() {
        let ts = spec(Language::TypeScript).unwrap();
        let js = spec(Language::JavaScript).unwrap();
        let name = |scan: Scan| scan.regions[0].grammar.name;
        let names = [
            name((ts.scan)("", Path::new("a.ts"))),
            name((ts.scan)("", Path::new("a.tsx"))),
            name((js.scan)("", Path::new("a.js"))),
        ];
        assert_eq!(names, ["typescript", "tsx", "javascript"]);
    }
}
