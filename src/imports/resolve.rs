//! Resolution of TypeScript and JavaScript specifiers against the known repository files.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::extract::literal_text;
use super::package::{is_builtin, package_name};
use super::{
    AliasConfig, Candidate, Environment, Project, Reference, Resolution, ecmascript_symbols,
};
use crate::model::SkipReason;

/// Extensions tried, in order, for extension probing and directory index files. Declaration files
/// come after the TypeScript files of their kind. `svelte` is last so that no specifier that
/// resolves without it changes meaning; neither TypeScript nor Vite appends it itself.
const SOURCE_EXTENSIONS: [&str; 12] = [
    "ts", "tsx", "d.ts", "mts", "cts", "d.mts", "d.cts", "js", "jsx", "mjs", "cjs", "svelte",
];

/// The TypeScript ESM convention: a specifier written with a JavaScript extension refers to the
/// TypeScript file that compiles to it.
const JS_TO_TS: [(&str, &[&str]); 4] = [
    ("js", &["ts", "tsx", "d.ts"]),
    ("jsx", &["tsx"]),
    ("mjs", &["mts", "d.mts"]),
    ("cjs", &["cts", "d.cts"]),
];

/// The table entry: an ECMAScript match carries one specifier and no names. The symbols it uses
/// are read from the statement around the specifier, see [`ecmascript_symbols`].
///
/// A specifier that is not a plain literal (`require(name)`, a template with a substitution) names
/// no file and is skipped as dynamic. A literal that is relative or an alias is resolved against
/// the repository. A literal that is neither, and is not a Node builtin or a declared package, is
/// skipped as an undeclared package.
pub fn ecmascript(
    reference: &Reference<'_>,
    importer: &Path,
    env: &Environment<'_>,
) -> Vec<Resolution> {
    if literal_text(reference.node, reference.source).is_none() {
        return vec![Resolution::Skipped(SkipReason::DynamicSpecifier)];
    }
    let specifier = strip_suffix(reference.specifier);
    let resolution = if is_relative(specifier) {
        resolve(specifier, importer, env)
    } else {
        resolve_bare(specifier, importer, env)
    };
    vec![resolution.using(ecmascript_symbols::used(reference.node, reference.source))]
}

/// The specifier without a bundler query or fragment: everything from the first `?`, or from the
/// first `#` that is not the first character (a leading `#` marks a package `imports` key).
pub(super) fn strip_suffix(specifier: &str) -> &str {
    let end = specifier
        .char_indices()
        .find(|&(index, c)| c == '?' || (c == '#' && index > 0))
        .map_or(specifier.len(), |(index, _)| index);
    specifier.get(..end).unwrap_or(specifier)
}

/// Resolves the relative `specifier`, written in `importer`, to a source file or an asset. A
/// specifier that is not relative is ignored here; see [`resolve_bare`]. Whatever its extension, a
/// relative specifier that names no file is unresolved.
pub fn resolve(specifier: &str, importer: &Path, env: &Environment<'_>) -> Resolution {
    if !is_relative(specifier) {
        return Resolution::Ignored;
    }
    join_normalised(importer.parent(), specifier)
        .and_then(|base| locate(&base, specifier.ends_with('/'), env))
        .map_or(Resolution::Unresolved, Found::resolution)
}

/// Looks `base` up like [`resolve`] does after joining: the file itself, then the probes. `None`
/// when nothing is found. `directory_only` is a candidate written with a trailing `/`.
pub(super) fn find(base: &Path, directory_only: bool, env: &Environment<'_>) -> Option<Resolution> {
    locate(base, directory_only, env).map(Found::resolution)
}

/// A repository file a candidate path names.
enum Found {
    /// A source file.
    Source(PathBuf),
    /// A file that is not source.
    Asset(PathBuf),
}

impl Found {
    fn resolution(self) -> Resolution {
        match self {
            Self::Source(target) => Resolution::resolved(target),
            Self::Asset(target) => Resolution::Asset { target },
        }
    }
}

/// Looks `base` up as a relative import would: the file itself, source before asset, then the
/// probes of [`probe`]. A path written with a trailing `/` names a directory only.
fn locate(base: &Path, directory_only: bool, env: &Environment<'_>) -> Option<Found> {
    if !directory_only {
        if env.known.contains(base) {
            return Some(Found::Source(base.to_path_buf()));
        }
        if env.assets.contains(base) {
            return Some(Found::Asset(base.to_path_buf()));
        }
    }
    probe(base, directory_only, env.known).map(Found::Source)
}

/// Resolves a non-relative `specifier` written in `importer`.
///
/// Through the alias configuration that governs `importer`, each candidate path is probed like a
/// relative import: exact, appended extensions, the JavaScript to TypeScript mapping and directory
/// index files. The first that names a known file wins. Then a workspace package of that name, or
/// for a `#` specifier the `imports` of the nearest `package.json`, gets its turn: it resolves, or
/// ignores the specifier when it finds no file. A specifier that a `paths` pattern matches is an
/// alias and is ignored when no file backs it. Anything else is a package: ignored when it is a
/// Node builtin or declared by a `package.json` at or above the importer, skipped otherwise.
fn resolve_bare(specifier: &str, importer: &Path, env: &Environment<'_>) -> Resolution {
    let config = governing(env, importer);
    if let Some(found) = config.and_then(|config| first_found(&config.candidates(specifier), env)) {
        return found.resolution();
    }
    let linked = if specifier.starts_with('#') {
        env.workspaces.imports_candidates(specifier, importer)
    } else {
        env.workspaces.candidates(specifier)
    };
    if let Some(candidates) = linked {
        return first_found(&candidates, env).map_or(Resolution::Ignored, Found::resolution);
    }
    if config.is_some_and(|config| config.matches(specifier)) {
        return Resolution::Ignored;
    }
    let declared = importer.parent().is_some_and(|dir| {
        dir.ancestors().any(|ancestor| {
            env.packages
                .get(ancestor)
                .is_some_and(|names| names.contains(package_name(specifier)))
        })
    });
    if is_builtin(specifier) || declared {
        Resolution::Ignored
    } else {
        Resolution::Skipped(SkipReason::UndeclaredPackage)
    }
}

/// The alias configuration that governs `importer`: walking up from its directory, the first
/// directory whose solution-style config references a project that includes `importer`, or that
/// holds a config of its own.
fn governing<'a>(env: &Environment<'a>, importer: &Path) -> Option<&'a AliasConfig> {
    importer.parent()?.ancestors().find_map(|dir| {
        let referenced = env
            .projects
            .get(dir)
            .and_then(|projects| projects.iter().find(|project| project.includes(importer)))
            .map(Project::config);
        referenced.or_else(|| env.aliases.get(dir))
    })
}

/// The first of `candidates`, in order, that names a repository file.
fn first_found(candidates: &[Candidate], env: &Environment<'_>) -> Option<Found> {
    candidates
        .iter()
        .find_map(|candidate| locate(&candidate.path, candidate.directory_only, env))
}

fn is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
}

/// Joins `specifier` onto `dir` and folds `.` and `..` lexically. `None` means the path leaves the
/// repository root.
pub(super) fn join_normalised(dir: Option<&Path>, specifier: &str) -> Option<PathBuf> {
    let mut joined = PathBuf::new();
    for component in dir.into_iter().flat_map(Path::components) {
        if let Component::Normal(part) = component {
            joined.push(part);
        }
    }
    for component in Path::new(specifier).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !joined.pop() {
                    return None;
                }
            }
            Component::Normal(part) => joined.push(part),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(joined)
}

fn probe(base: &Path, directory_only: bool, known: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    let as_file = if directory_only {
        Vec::new()
    } else {
        file_candidates(base)
    };
    let as_index = SOURCE_EXTENSIONS
        .iter()
        .map(|extension| base.join("index").with_extension(extension));
    as_file
        .into_iter()
        .chain(as_index)
        .find(|candidate| known.contains(candidate))
}

fn file_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![base.to_path_buf()];
    candidates.extend(
        SOURCE_EXTENSIONS
            .iter()
            .map(|extension| append_extension(base, extension)),
    );
    let written = base.extension().and_then(|extension| extension.to_str());
    if let Some((_, replacements)) = JS_TO_TS.iter().find(|(js, _)| Some(*js) == written) {
        candidates.extend(
            replacements
                .iter()
                .map(|extension| base.with_extension(extension)),
        );
    }
    candidates
}

/// Appends rather than replaces, so `user.service` probes `user.service.ts`.
fn append_extension(base: &Path, extension: &str) -> PathBuf {
    let mut name = OsString::from(base.as_os_str());
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}
