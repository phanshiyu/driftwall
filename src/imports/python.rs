//! Resolution of Python module references against the known repository files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Environment, Reference, Resolution};
use crate::model::Symbol;

/// Directory prefixes that absolute imports are tried against, in order: the repository root, then
/// `src/`.
const ABSOLUTE_ROOTS: [&str; 2] = ["", "src"];

/// The table entry. `reference.specifier` is the module as written (`a.b`, `.a`, `..`) and
/// `reference.names` the names a `from` statement imports from it.
///
/// With names, each name resolves on its own: the submodule `<module>/<name>` first, then the
/// module itself. Without names (`import a.b`, `from a import *`) only the module resolves.
///
/// The symbols used follow what each name resolved to. A name that is a submodule file uses the
/// submodule as a whole ([`Symbol::All`]); a name found inside the module uses that name. A
/// statement without names uses the module as a whole.
/// Absolute modules that match no file are third party or standard library and are ignored;
/// relative ones that match nothing are unresolved.
pub fn resolve(
    reference: &Reference<'_>,
    importer: &Path,
    env: &Environment<'_>,
) -> Vec<Resolution> {
    let known = env.known;
    let written: String = reference
        .specifier
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let dots = written.chars().take_while(|&c| c == '.').count();
    let module: Vec<&str> = written
        .trim_start_matches('.')
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect();
    let names: Vec<Vec<&str>> = if reference.names.is_empty() {
        vec![Vec::new()]
    } else {
        reference
            .names
            .iter()
            .map(|name| vec![name.trim()])
            .collect()
    };

    let roots: Vec<PathBuf> = if dots == 0 {
        ABSOLUTE_ROOTS.iter().map(PathBuf::from).collect()
    } else {
        match relative_root(importer, dots) {
            Some(dir) => vec![dir],
            None => return vec![Resolution::Unresolved],
        }
    };
    names
        .iter()
        .map(|name| {
            let hit = roots.iter().find_map(|root| {
                let base = join(root, &module);
                first_known(&candidates(&base, name, !module.is_empty()), known)
            });
            match hit {
                Some((target, _)) if target == importer => Resolution::Ignored,
                Some((target, symbol)) => {
                    Resolution::resolved(target).using(BTreeSet::from([symbol]))
                }
                None if dots == 0 => Resolution::Ignored,
                None => Resolution::Unresolved,
            }
        })
        .collect()
}

/// The directory a relative import counts from: the importer's directory, climbed `dots - 1`
/// levels. `None` means it climbs above the repository root.
fn relative_root(importer: &Path, dots: usize) -> Option<PathBuf> {
    let mut dir = importer.parent()?.to_path_buf();
    for _ in 1..dots {
        if !dir.pop() {
            return None;
        }
    }
    Some(dir)
}

fn join(root: &Path, segments: &[&str]) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(segments);
    path
}

/// Files `base` plus `name` can mean, best first, each with the symbol it makes the import use:
/// the submodule as a module file or package (all of it), then the module itself (the name inside
/// it, or all of it when there is no name). A relative import with no module (`from . import x`)
/// has the package `__init__.py` as its module.
fn candidates(base: &Path, name: &[&str], has_module: bool) -> Vec<(PathBuf, Symbol)> {
    let inside = name
        .first()
        .map_or(Symbol::All, |name| Symbol::Name((*name).to_owned()));
    let mut found = Vec::new();
    if !name.is_empty() {
        let sub = join(base, name);
        found.push((sub.with_extension("py"), Symbol::All));
        found.push((sub.join("__init__.py"), Symbol::All));
    }
    if has_module {
        found.push((base.with_extension("py"), inside.clone()));
    }
    found.push((base.join("__init__.py"), inside));
    found
}

fn first_known(
    candidates: &[(PathBuf, Symbol)],
    known: &BTreeSet<PathBuf>,
) -> Option<(PathBuf, Symbol)> {
    candidates
        .iter()
        .find(|(candidate, _)| known.contains(candidate))
        .cloned()
}
