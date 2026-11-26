//! Follows a named import through the modules that only re-export it, see "Re-exports" in
//! `docs/DESIGN.md`.
//!
//! Resolution decides which file an import names. This second step reads the re-export tables of
//! the files that were named: when a file re-exports a name from another repository file, the
//! symbol is attributed to the file that defines it. Each language fills a [`Table`] from syntax
//! alone (the `reexport` module of `rust`, `web` and `python`); the walk here is shared.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use tree_sitter::{Parser, Tree};

use super::environment::Environment;
use super::{FileImports, spec};
use crate::code::{ImportKind, Language, Location, ResolvedImport, Symbol};
use crate::reading::languages::{python, rust, web};
use crate::reading::syntax::table::Table;

/// The name a default export has.
const DEFAULT: &str = "default";

/// The files owned by a facade node, with that node's name, see "Facades" in "Re-exports". A file
/// not in the map is in no facade node.
pub type Facades = BTreeMap<PathBuf, String>;

/// The tables of the files that may be named by an import, by repository-relative path.
pub type Tables = BTreeMap<PathBuf, Table>;

/// The table of `source`, the text of `file`, when it may hold a re-export. `None` when a cheap
/// text check rules re-exports out, or the language has none.
pub fn facade(
    source: &str,
    language: Language,
    file: &Path,
    env: &Environment<'_>,
) -> Option<Table> {
    let maybe = match language {
        Language::Rust => {
            (source.contains("pub") && source.contains("use")) || source.contains("type")
        }
        Language::TypeScript | Language::JavaScript => {
            source.contains("type") || (source.contains("export") && source.contains("from"))
        }
        Language::Python => {
            file.file_name().is_some_and(|name| name == "__init__.py") && source.contains("import")
        }
        _ => false,
    };
    maybe.then(|| table(source, language, file, env))
}

/// The table of `source`, the text of `file`, read in full.
pub fn table(source: &str, language: Language, file: &Path, env: &Environment<'_>) -> Table {
    let Some(tree) = parse(source, language, file) else {
        return Table::default();
    };
    match language {
        Language::Rust => rust::reexport_table(&tree, source, file, &env.files, &env.rust),
        Language::TypeScript | Language::JavaScript => {
            web::reexport_table(&tree, source, file, &env.files, &env.web)
        }
        Language::Python => python::reexport_table(&tree, source, file, &env.files, env.python),
        _ => Table::default(),
    }
}

fn parse(source: &str, language: Language, file: &Path) -> Option<Tree> {
    let spec = spec::spec(language);
    let scan = (spec.scan)(source, file);
    let region = scan.regions.first()?;
    let mut parser = Parser::new();
    parser.set_language(&(region.grammar.language)()).ok()?;
    parser.parse(source, None)
}

/// Adds the tables of the files some table globs, which may have no re-export of their own, until
/// every star source has one. A source that is not a readable source file gets none.
pub fn complete(
    tables: &mut Tables,
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
    env: &Environment<'_>,
) {
    loop {
        let missing: BTreeSet<PathBuf> = tables
            .values()
            .flat_map(|table| &table.stars)
            .filter(|source| !tables.contains_key(*source) && files.contains_key(*source))
            .cloned()
            .collect();
        if missing.is_empty() {
            return;
        }
        let missing: Vec<PathBuf> = missing.into_iter().collect();
        let found: Vec<(PathBuf, Table)> = missing
            .par_iter()
            .map(|path| {
                let table = files
                    .get(path)
                    .and_then(|&language| {
                        let source = std::fs::read_to_string(root.join(path)).ok()?;
                        Some(table(&source, language, path, env))
                    })
                    .unwrap_or_default();
                (path.clone(), table)
            })
            .collect();
        tables.extend(found);
    }
}

/// The file that defines `name` of `file` and the symbol it has there, or `None` when the name
/// stays on `file`: it is defined there, nothing re-exports it, it cannot be decided or the walk
/// cycles. A name that re-exports a module as a whole leads to that module's file and
/// [`Symbol::All`], because the item used inside the module is not recorded.
fn follow(
    tables: &Tables,
    file: &Path,
    name: &str,
    stops: &dyn Fn(&Path) -> bool,
) -> Option<(PathBuf, Symbol)> {
    let start = (file.to_path_buf(), name.to_owned());
    let mut current = start.clone();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current.clone()) {
            return None;
        }
        // why: a facade is where the importer's dependency ends, the walk goes no further.
        if stops(&current.0) {
            break;
        }
        let Some(table) = tables.get(&current.0) else {
            break;
        };
        if table.local.contains(&current.1) {
            break;
        }
        if let Some(next) = table.names.get(&current.1) {
            current = next.clone();
        } else if let Some(module) = table.modules.get(&current.1) {
            return Some((module.clone(), Symbol::All));
        } else if let Some(source) = single_source(tables, table, &current.1) {
            current.0 = source;
        } else {
            break;
        }
    }
    (current != start).then_some((current.0, Symbol::Name(current.1)))
}

/// The one glob source of `table` that can export `name`, when exactly one can and the others
/// surely cannot.
fn single_source(tables: &Tables, table: &Table, name: &str) -> Option<PathBuf> {
    if name == DEFAULT {
        return None;
    }
    let mut candidates = Vec::new();
    for source in &table.stars {
        match can_export(tables, source, name, &mut BTreeSet::new()) {
            Can::Yes => candidates.push((source, true)),
            Can::Unknown => candidates.push((source, false)),
            Can::No => {}
        }
    }
    match candidates.as_slice() {
        [(source, true)] => Some((*source).clone()),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Can {
    Yes,
    No,
    Unknown,
}

/// Whether `file` can export `name`: it defines it, re-exports it, or a glob of its own can. A
/// file with no table is not known to export it or not.
fn can_export(
    tables: &Tables,
    file: &Path,
    name: &str,
    seen: &mut BTreeSet<(PathBuf, String)>,
) -> Can {
    let Some(table) = tables.get(file) else {
        return Can::Unknown;
    };
    if !seen.insert((file.to_path_buf(), name.to_owned())) {
        return Can::No;
    }
    if table.all.as_ref().is_some_and(|all| !all.contains(name))
        || (table.public_only && name.starts_with('_'))
    {
        return Can::No;
    }
    if table.local.contains(name)
        || table.names.contains_key(name)
        || table.modules.contains_key(name)
        || table.inline.contains_key(name)
    {
        return Can::Yes;
    }
    if name == DEFAULT {
        return Can::No;
    }
    let mut result = Can::No;
    for source in &table.stars {
        match can_export(tables, source, name, seen) {
            Can::Yes => return Can::Yes,
            Can::Unknown => result = Can::Unknown,
            Can::No => {}
        }
    }
    result
}

/// Whether `table` re-exports no name, namespace or glob.
fn forwards_nothing(table: &Table) -> bool {
    table.names.is_empty()
        && table.stars.is_empty()
        && table.modules.is_empty()
        && table.inline.is_empty()
}

/// Attributes every named symbol of every import in `imports` to the file that defines it.
pub fn apply(imports: &mut BTreeMap<PathBuf, FileImports>, tables: &Tables, facades: &Facades) {
    for (file, found) in imports.iter_mut() {
        found.reexports = tables
            .get(file)
            .is_some_and(|table| !forwards_nothing(table));
    }
    if tables.values().all(forwards_nothing) {
        return;
    }
    for (file, found) in imports.iter_mut() {
        let home = facades.get(file);
        // why: inside a facade node nothing changes, every other importer stops at the facade.
        let stops = |at: &Path| facades.get(at).is_some_and(|node| Some(node) != home);
        let own = tables.get(file).map(|table| &table.lines);
        let mut merged: BTreeMap<(Location, PathBuf, ImportKind, bool), BTreeSet<Symbol>> =
            BTreeMap::new();
        for import in std::mem::take(&mut found.resolved) {
            let kept = matches!(import.kind, ImportKind::Embed | ImportKind::Asset)
                || own.is_some_and(|lines| lines.contains(&import.at.line));
            let groups = if kept {
                None
            } else {
                regroup(&import, file, tables, &stops)
            };
            let Some(groups) = groups else {
                let key = (import.at, import.target, import.kind, import.test_only);
                merged.entry(key).or_default().extend(import.symbols);
                continue;
            };
            for (target, symbols) in groups {
                let key = (import.at.clone(), target, import.kind, import.test_only);
                merged.entry(key).or_default().extend(symbols);
            }
        }
        found.resolved = merged
            .into_iter()
            .map(|((at, target, kind, test_only), symbols)| ResolvedImport {
                at,
                target,
                symbols,
                kind,
                test_only,
                via: None,
            })
            .collect();
        // why: the documented order puts symbols before kind, which the merge key does not.
        found.resolved.sort();
    }
}

/// The symbols of `import` grouped by the file that defines them, or `None` when nothing moves.
fn regroup(
    import: &ResolvedImport,
    importer: &Path,
    tables: &Tables,
    stops: &dyn Fn(&Path) -> bool,
) -> Option<BTreeMap<PathBuf, BTreeSet<Symbol>>> {
    let mut groups: BTreeMap<PathBuf, BTreeSet<Symbol>> = BTreeMap::new();
    let mut moved = false;
    for symbol in &import.symbols {
        if *symbol == Symbol::All
            && let Some((files, keeps)) = through_glob(&import.target, tables, stops)
        {
            moved = true;
            let own = keeps.then(|| import.target.clone());
            for file in files.into_iter().chain(own).filter(|file| file != importer) {
                groups.entry(file).or_default().insert(Symbol::All);
            }
            continue;
        }
        if let Symbol::Name(name) = symbol
            && let Some(files) = through_inline(&import.target, name, tables, stops)
        {
            moved = true;
            for file in files.iter().filter(|file| file.as_path() != importer) {
                groups.entry(file.clone()).or_default().insert(Symbol::All);
            }
            continue;
        }
        let followed = match symbol {
            Symbol::Name(name) => follow(tables, &import.target, name, stops),
            Symbol::All => None,
        };
        match followed {
            // why: a name that leads back to the importer is the importer's own item, not an import.
            Some((target, _)) if target == importer => moved = true,
            Some((target, symbol)) => {
                moved = true;
                groups.entry(target).or_default().insert(symbol);
            }
            None => {
                groups
                    .entry(import.target.clone())
                    .or_default()
                    .insert(symbol.clone());
            }
        }
    }
    moved.then_some(groups)
}

/// Where a Rust glob import of `file` (`use crate::c::*;`) reaches, when `file` re-exports other
/// repository files: the files its re-exports lead to, each followed to its definer, and whether
/// `file` stays too because it defines items of its own. Which of the names the importer uses is
/// not recorded, so each re-exported file is used as a whole. `None` for another language (a
/// glob stays on the module there), a facade, and a file that re-exports nothing.
fn through_glob(
    file: &Path,
    tables: &Tables,
    stops: &dyn Fn(&Path) -> bool,
) -> Option<(BTreeSet<PathBuf>, bool)> {
    if file.extension().is_none_or(|ext| ext != "rs") || stops(file) {
        return None;
    }
    let table = tables.get(file)?;
    let mut files: BTreeSet<PathBuf> = table.modules.values().cloned().collect();
    files.extend(table.stars.iter().cloned());
    files.extend(table.inline.values().flatten().cloned());
    for (name, (source, _)) in &table.names {
        let followed =
            follow(tables, file, name, stops).map_or_else(|| source.clone(), |(at, _)| at);
        files.insert(followed);
    }
    files.remove(file);
    (!files.is_empty()).then_some((files, !table.local.is_empty()))
}

/// The files the inline module `name` of `file` re-exports from (`pub mod r { pub use ..; }`),
/// each used as a whole because the item used inside the module is not recorded. `None` when
/// `file` has no such module, or is a facade.
fn through_inline<'t>(
    file: &Path,
    name: &str,
    tables: &'t Tables,
    stops: &dyn Fn(&Path) -> bool,
) -> Option<&'t BTreeSet<PathBuf>> {
    if stops(file) {
        return None;
    }
    tables.get(file)?.inline.get(name)
}
