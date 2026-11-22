//! Resolution of TypeScript and JavaScript specifiers against the known repository files.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::{Reference, Resolution};

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

/// The table entry: an ECMAScript match carries one specifier and no names.
pub fn ecmascript(
    reference: &Reference<'_>,
    importer: &Path,
    known: &BTreeSet<PathBuf>,
) -> Vec<Resolution> {
    vec![resolve(reference.specifier, importer, known)]
}

/// Resolves `specifier`, written in `importer`, to a file in `known`.
///
/// Only relative specifiers are resolved. `tsconfig` `paths` aliases are not supported yet, so
/// bare packages and aliases are ignored.
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
        Some(target) => Resolution::Resolved(target),
        None if is_asset => Resolution::Ignored,
        None => Resolution::Unresolved,
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
fn join_normalised(dir: Option<&Path>, specifier: &str) -> Option<PathBuf> {
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
