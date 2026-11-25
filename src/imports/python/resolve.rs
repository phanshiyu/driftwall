//! Resolution of Python module references against the known repository files.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use super::call::module_of;
use super::layout::PythonLayout;
use super::stdlib::is_stdlib;
use crate::imports::syntax::literal::literal_text;
use crate::imports::syntax::resolution::{Files, Reference, Resolution};
use crate::model::{PackageKind, SkipReason, Symbol};

/// Callees whose second argument is the path of a Python file to load.
const FILE_LOADERS: [&str; 2] = ["SourceFileLoader", "spec_from_file_location"];

/// The table entry. `reference.specifier` is the module as written (`a.b`, `.a`, `..`) and
/// `reference.names` the names a `from` statement imports from it. See "Python resolution" in
/// `docs/DESIGN.md` for the rules.
///
/// With names, each name resolves on its own: the submodule `<module>/<name>` first, then the
/// module itself. Without names (`import a.b`, `from a import *`) only the module resolves.
///
/// The symbols used follow what each name resolved to. A name that is a submodule is a module
/// binding and uses no symbol; a name found inside the module uses that name. `import a.b` is a
/// module binding too. Only `from a import *` uses [`Symbol::All`].
///
/// An absolute module is looked up under the import roots of the importer. One that no root
/// resolves is unresolved when its top-level name is a first-party module or regular package under a root,
/// unreachable when the name matches a repository path no root reaches, and otherwise a third
/// party or standard library module, which is a package use. A name that is only a directory without
/// `__init__.py` is a namespace portion an installed package beats: skipped as undeclared. A relative module that matches nothing is
/// unresolved.
///
/// A call to `importlib.import_module(..)` or `__import__(..)` with a plain string literal
/// resolves like `import <literal>`, see [`module_of`]; any other call is skipped as dynamic.
pub fn resolve(
    reference: &Reference<'_>,
    importer: &Path,
    files: &Files<'_>,
    layout: &PythonLayout,
) -> Vec<Resolution> {
    let env = Env {
        known: files.known,
        python: layout,
    };
    let env = &env;
    // why: the first argument of `importlib.import_module(..)` or `__import__(..)` is not a statement's module path.
    if reference
        .node
        .parent()
        .is_some_and(|parent| parent.kind() == "argument_list")
    {
        return call(reference, importer, env);
    }
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
    let star = is_star(reference);
    if dots == 0 {
        return absolute(&module, &names, star, importer, env);
    }
    let Some(dir) = relative_root(importer, dots) else {
        return vec![Resolution::Unresolved];
    };
    let base = join(&dir, &module);
    names
        .iter()
        .map(|name| {
            let hit = first_known(
                &candidates(&base, name, !module.is_empty(), star),
                env.known,
            );
            hit.map_or(Resolution::Unresolved, |hit| found(hit, importer))
        })
        .collect()
}

/// What the resolver reads: the repository files and the layout of the Python code.
struct Env<'a> {
    known: &'a BTreeSet<PathBuf>,
    python: &'a PythonLayout,
}

/// Resolves the call around `reference`: like `import <literal>` when the call names its module
/// with a plain literal, dynamic otherwise.
fn call(reference: &Reference<'_>, importer: &Path, env: &Env<'_>) -> Vec<Resolution> {
    if loads_file(reference) {
        return vec![load_path(reference, importer, env)];
    }
    let Some(module) = module_of(reference) else {
        return vec![Resolution::Skipped(SkipReason::DynamicSpecifier)];
    };
    let segments: Vec<&str> = module.split('.').collect();
    absolute(&segments, &[Vec::new()], false, importer, env)
}

/// Whether the call around `reference` is a file loader (`SourceFileLoader(..)`,
/// `spec_from_file_location(..)`) and the reference is its path argument.
fn loads_file(reference: &Reference<'_>) -> bool {
    reference
        .node
        .parent()
        .and_then(|list| list.parent())
        .and_then(|call| call.child_by_field_name("function"))
        .and_then(|function| function.child_by_field_name("attribute"))
        .and_then(|name| name.utf8_text(reference.source.as_bytes()).ok())
        .is_some_and(|name| FILE_LOADERS.contains(&name))
}

/// The file a loader call names with a literal path: the repository file at that path, taken
/// relative to the importer's directory first and to the repository root second. Anything else
/// (not a plain literal, a glob or format placeholder, an absolute or `~` path, a path that leaves
/// the repository or names no known file) is dynamic.
fn load_path(reference: &Reference<'_>, importer: &Path, env: &Env<'_>) -> Resolution {
    let dynamic = Resolution::Skipped(SkipReason::DynamicSpecifier);
    let Some(literal) = literal_text(reference.node, reference.source) else {
        return dynamic;
    };
    if literal.is_empty()
        || literal.starts_with(['/', '~'])
        || literal.contains(['*', '?', '[', '{', '%', '$'])
    {
        return dynamic;
    }
    let beside = importer.parent().unwrap_or_else(|| Path::new(""));
    [beside, Path::new("")]
        .iter()
        .filter_map(|base| normalise(&base.join(literal)))
        .find(|path| env.known.contains(path))
        .map_or(dynamic, |target| found((target, None), importer))
}

/// `path` without `.` segments and with `..` applied, or `None` when it leaves the repository.
fn normalise(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// Resolves the absolute `module` (dotted segments) with each of `names` under the import roots
/// of `importer`.
fn absolute(
    module: &[&str],
    names: &[Vec<&str>],
    star: bool,
    importer: &Path,
    env: &Env<'_>,
) -> Vec<Resolution> {
    let roots = env.python.roots(importer);
    names
        .iter()
        .map(|name| {
            let hit = roots.iter().find_map(|root| {
                let candidates = candidates(&join(root, module), name, !module.is_empty(), star);
                first_known(&candidates, env.known)
            });
            hit.map_or_else(
                || miss(module, &roots, importer, env),
                |hit| found(hit, importer),
            )
        })
        .collect()
}

/// What an absolute `module` that no root resolves is: see [`resolve`].
fn miss(module: &[&str], roots: &[PathBuf], importer: &Path, env: &Env<'_>) -> Resolution {
    let Some(first) = module.first() else {
        return Resolution::Ignored;
    };
    if roots
        .iter()
        .any(|root| env.python.is_directory(&join(root, module)))
    {
        return Resolution::Ignored;
    }
    let first_party = roots.iter().any(|root| {
        let base = join(root, &[first]);
        env.known.contains(&base.with_extension("py"))
            || env.known.contains(&base.join("__init__.py"))
    });
    if first_party {
        return Resolution::Unresolved;
    }
    // why: a directory with no `__init__.py` is a namespace portion, which an installed regular package of the same name beats.
    if let Some(root) = roots
        .iter()
        .find(|root| env.python.is_directory(&join(root, &[first])))
    {
        return Resolution::UndeclaredPackage {
            name: (*first).to_owned(),
            message: Some(format!(
                "`{first}` is the namespace directory `{}` (no `__init__.py`) and holds no `{}`; treated as an installed package",
                join(root, &[first]).display(),
                module.join(".")
            )),
        };
    }
    // why: a module nested below directories that hold no Python (`a/c/g/m.py` for `c.g.m`) has no top-level name match.
    let found = env.python.matching(first).map_or_else(
        || env.python.matching_module(module),
        |path| Some(((*first).to_owned(), path)),
    );
    found.map_or_else(
        || Resolution::Package {
            name: (*first).to_owned(),
            kind: if is_stdlib(first) {
                PackageKind::Stdlib
            } else {
                PackageKind::ThirdParty
            },
        },
        |(name, path)| Resolution::SkippedWith {
            reason: SkipReason::UnreachableModule,
            message: format!(
                "`{name}` matches `{}`, which no import root of `{}` reaches",
                path.display(),
                importer.display()
            ),
        },
    )
}

/// The resolution of a hit: the target with the symbol it uses, or ignored when it is the importer
/// itself.
fn found((target, symbol): (PathBuf, Option<Symbol>), importer: &Path) -> Resolution {
    if target == importer {
        Resolution::Ignored
    } else {
        Resolution::resolved(target).using(symbol.into_iter().collect())
    }
}

/// Whether the reference is the module of `from m import *`.
fn is_star(reference: &Reference<'_>) -> bool {
    reference.node.parent().is_some_and(|statement| {
        statement.kind() == "import_from_statement" && {
            let mut cursor = statement.walk();
            statement
                .children(&mut cursor)
                .any(|child| child.kind() == "wildcard_import")
        }
    })
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
/// the submodule as a module file or package (none, a module binding), then the module itself (the
/// name inside it, or none for a module binding, or all of it for a star import). A relative import
/// with no module (`from . import x`) has the package `__init__.py` as its module.
fn candidates(
    base: &Path,
    name: &[&str],
    has_module: bool,
    star: bool,
) -> Vec<(PathBuf, Option<Symbol>)> {
    let inside = name
        .first()
        .map(|name| Symbol::Name((*name).to_owned()))
        .or_else(|| star.then_some(Symbol::All));
    let mut found = Vec::new();
    if !name.is_empty() {
        let sub = join(base, name);
        found.push((sub.with_extension("py"), None));
        found.push((sub.join("__init__.py"), None));
    }
    if has_module {
        found.push((base.with_extension("py"), inside.clone()));
    }
    found.push((base.join("__init__.py"), inside));
    found
}

fn first_known(
    candidates: &[(PathBuf, Option<Symbol>)],
    known: &BTreeSet<PathBuf>,
) -> Option<(PathBuf, Option<Symbol>)> {
    candidates
        .iter()
        .find(|(candidate, _)| known.contains(candidate))
        .cloned()
}
