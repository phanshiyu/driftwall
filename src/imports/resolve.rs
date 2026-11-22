//! Resolution of TypeScript and JavaScript specifiers against the known repository files.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::extract::literal_text;
use super::package::{is_builtin, package_name};
use super::{Environment, Reference, Resolution, ecmascript_symbols, nearest};
use crate::model::SkipReason;

/// Extensions tried, in order, for extension probing and directory index files.
const SOURCE_EXTENSIONS: [&str; 8] = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// The TypeScript ESM convention: a specifier written with a JavaScript extension refers to the
/// TypeScript file that compiles to it.
const JS_TO_TS: [(&str, &[&str]); 4] = [
    ("js", &["ts", "tsx"]),
    ("jsx", &["tsx"]),
    ("mjs", &["mts"]),
    ("cjs", &["cts"]),
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
    let specifier = reference.specifier;
    let resolution = if is_relative(specifier) {
        resolve(specifier, importer, env.known)
    } else {
        resolve_bare(specifier, importer, env)
    };
    vec![resolution.using(ecmascript_symbols::used(reference.node, reference.source))]
}

/// Resolves the relative `specifier`, written in `importer`, to a file in `known`. A specifier
/// that is not relative is ignored here; see [`resolve_aliased`].
pub fn resolve(specifier: &str, importer: &Path, known: &BTreeSet<PathBuf>) -> Resolution {
    if !is_relative(specifier) {
        return Resolution::Ignored;
    }
    let is_asset = has_non_source_extension(Path::new(specifier));
    let Some(base) = join_normalised(importer.parent(), specifier) else {
        return if is_asset {
            Resolution::Ignored
        } else {
            Resolution::Unresolved
        };
    };
    match probe(&base, specifier.ends_with('/'), known) {
        Some(target) => Resolution::resolved(target),
        None if is_asset => Resolution::Ignored,
        None => Resolution::Unresolved,
    }
}

/// Resolves a non-relative `specifier` written in `importer`.
///
/// Through the alias configuration that governs `importer`, each candidate path is probed like a
/// relative import: exact, appended extensions, the JavaScript to TypeScript mapping and directory
/// index files. The first that names a known file wins. A specifier that a `paths` pattern matches
/// is an alias and is ignored when no file backs it. Anything else is a package: ignored when it
/// is a Node builtin or declared by the nearest `package.json`, skipped otherwise.
fn resolve_bare(specifier: &str, importer: &Path, env: &Environment<'_>) -> Resolution {
    if let Some(config) = nearest(env.aliases, importer) {
        let found = config
            .candidates(specifier)
            .iter()
            .find_map(|candidate| probe(&candidate.path, candidate.directory_only, env.known));
        if let Some(target) = found {
            return Resolution::resolved(target);
        }
        if config.matches(specifier) {
            return Resolution::Ignored;
        }
    }
    let declared = nearest(env.packages, importer)
        .is_some_and(|names| names.contains(package_name(specifier)));
    if is_builtin(specifier) || declared {
        Resolution::Ignored
    } else {
        Resolution::Skipped(SkipReason::UndeclaredPackage)
    }
}

fn is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
}

fn has_non_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| !SOURCE_EXTENSIONS.contains(&extension))
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
