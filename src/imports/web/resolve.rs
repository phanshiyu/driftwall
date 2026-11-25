//! Resolution of TypeScript and JavaScript specifiers against the known repository files.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::alias::{AliasConfig, Candidate};
use super::env::Env;
use super::package::{is_builtin, package_name};
use super::project::Project;
use super::symbols;
use crate::code::{PackageKind, SkipReason};
use crate::imports::syntax::literal::literal_text;
use crate::imports::syntax::paths::join_normalised;
use crate::imports::syntax::resolution::{Files, Reference, Resolution};

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
/// are read from the statement around the specifier, see [`symbols`].
///
/// A specifier that is not a plain literal (`require(name)`, a template with a substitution) names
/// no file and is skipped as dynamic. A literal that is relative or an alias is resolved against
/// the repository. A literal that is neither, and is not a Node builtin or a declared package, is
/// skipped as an undeclared package.
pub fn ecmascript(
    reference: &Reference<'_>,
    importer: &Path,
    files: &Files<'_>,
    env: &Env<'_>,
) -> Vec<Resolution> {
    let Some(literal) = literal_text(reference.node, reference.source) else {
        return vec![Resolution::Skipped(SkipReason::DynamicSpecifier)];
    };
    if is_file_reference(reference.node) {
        return vec![file_reference(literal, importer, files)];
    }
    let resolution = specifier(reference.specifier, importer, files, env);
    vec![resolution.using(symbols::used(reference.node, reference.source))]
}

/// Whether `node`, a captured specifier, is the first argument of a `new` expression: the only
/// `new` form the query captures is `new URL(.., import.meta.url)`.
fn is_file_reference(node: tree_sitter::Node<'_>) -> bool {
    node.parent()
        .and_then(|arguments| arguments.parent())
        .is_some_and(|parent| parent.kind() == "new_expression")
}

/// Resolves the literal `written` of `new URL(written, import.meta.url)` in `importer`: the exact
/// file it names, relative to the importer, with no probing.
fn file_reference(written: &str, importer: &Path, files: &Files<'_>) -> Resolution {
    let path = strip_suffix(written);
    // why: `https:` and `data:` URLs name no repository file.
    if has_scheme(path) {
        return Resolution::Ignored;
    }
    match join_normalised(importer.parent(), path) {
        Some(target) if files.known.contains(&target) => Resolution::resolved(target),
        Some(target) if files.assets.contains(&target) => Resolution::Asset { target },
        Some(target) => {
            let shown = if target.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                target.display().to_string()
            };
            let what = if has_file_at_or_below(&target, files) || target.as_os_str().is_empty() {
                "is a directory"
            } else {
                "has no file in the repository"
            };
            Resolution::SkippedWith {
                reason: SkipReason::FileReferenceMissing,
                message: format!("`{shown}` {what}"),
            }
        }
        None => Resolution::SkippedWith {
            reason: SkipReason::FileReferenceMissing,
            message: format!("`{written}` leaves the repository"),
        },
    }
}

/// Whether `path` starts with a URL scheme: a letter, then letters, digits, `+`, `-` or `.`, then `:`.
fn has_scheme(path: &str) -> bool {
    path.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// Resolves the literal `written` as the specifier of an import in `importer`: the suffix, the
/// relative or bare resolution and the query kind of the module documentation, with no symbols.
pub fn specifier(written: &str, importer: &Path, files: &Files<'_>, env: &Env<'_>) -> Resolution {
    let stripped = strip_suffix(written);
    let resolution = if is_relative(stripped) {
        resolve(stripped, importer, files)
    } else {
        resolve_bare(stripped, importer, files, env)
    };
    with_query_kind(written, resolution)
}

/// `resolution` for a specifier written as `specifier`, with a source file target turned into an
/// embed when the query asks for its text or URL: a `raw`, `url` or `inline` parameter and no
/// `worker` or `sharedworker` one. Anything else is returned unchanged.
pub(super) fn with_query_kind(specifier: &str, resolution: Resolution) -> Resolution {
    let query = specifier
        .split_once('?')
        .map_or("", |(_, query)| query.split('#').next().unwrap_or_default());
    let names: Vec<&str> = query
        .split('&')
        .map(|parameter| parameter.split('=').next().unwrap_or_default())
        .collect();
    let loads_data = names
        .iter()
        .any(|name| matches!(*name, "raw" | "url" | "inline"))
        && !names
            .iter()
            .any(|name| matches!(*name, "worker" | "sharedworker"));
    match resolution {
        Resolution::Resolved { target, .. } if loads_data => Resolution::Embedded { target },
        other => other,
    }
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
pub fn resolve(specifier: &str, importer: &Path, files: &Files<'_>) -> Resolution {
    if !is_relative(specifier) {
        return Resolution::Ignored;
    }
    join_normalised(importer.parent(), specifier)
        .and_then(|base| locate(&base, specifier.ends_with('/'), files))
        .map_or(Resolution::Unresolved, Found::resolution)
}

/// Looks `base` up like [`resolve`] does after joining: the file itself, then the probes. `None`
/// when nothing is found. `directory_only` is a candidate written with a trailing `/`.
pub(super) fn find(base: &Path, directory_only: bool, files: &Files<'_>) -> Option<Resolution> {
    locate(base, directory_only, files).map(Found::resolution)
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
fn locate(base: &Path, directory_only: bool, files: &Files<'_>) -> Option<Found> {
    if !directory_only {
        if files.known.contains(base) {
            return Some(Found::Source(base.to_path_buf()));
        }
        if files.assets.contains(base) {
            return Some(Found::Asset(base.to_path_buf()));
        }
    }
    probe(base, directory_only, files.known).map(Found::Source)
}

/// Resolves a non-relative `specifier` written in `importer`.
///
/// Through the alias configuration that governs `importer`, each candidate path is probed like a
/// relative import: exact, appended extensions, the JavaScript to TypeScript mapping and directory
/// index files. The first that names a known file wins. Then a workspace package of that name, or
/// for a `#` specifier the `imports` of the nearest `package.json`, gets its turn: it resolves, or
/// ignores the specifier when it finds no file. A specifier that a `paths` pattern matches is an
/// alias and is ignored when no file backs it. A virtual module is ignored. Anything else is a package
/// use: stdlib for a Node builtin, third party otherwise, which is also skipped as undeclared when no
/// `package.json` at or above the importer declares it.
fn resolve_bare(specifier: &str, importer: &Path, files: &Files<'_>, env: &Env<'_>) -> Resolution {
    let config = governing(env, importer);
    if let Some(found) = config.and_then(|config| first_found(&config.candidates(specifier), files))
    {
        return found.resolution();
    }
    if let Some(resolution) = vite_alias(specifier, importer, files, env) {
        return resolution;
    }
    let linked = if specifier.starts_with('#') {
        env.workspaces.imports_candidates(specifier, importer)
    } else {
        env.workspaces.candidates(specifier)
    };
    if let Some(candidates) = linked {
        return first_found(&candidates, files).map_or(Resolution::Ignored, Found::resolution);
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
    if is_virtual(specifier) {
        return Resolution::Ignored;
    }
    if is_builtin(specifier) {
        let module = specifier.strip_prefix("node:").unwrap_or(specifier);
        let module = module.split('/').next().unwrap_or(module);
        return Resolution::Package {
            name: format!("node:{module}"),
            kind: PackageKind::Stdlib,
        };
    }
    let name = package_name(specifier);
    match (is_package_name(name), declared) {
        (true, true) => Resolution::Package {
            name: name.to_owned(),
            kind: PackageKind::ThirdParty,
        },
        (true, false) => Resolution::UndeclaredPackage {
            name: name.to_owned(),
            message: None,
        },
        // why: a URL, an absolute path or a `#` key names no package, but it is still not followable.
        (false, _) => Resolution::Skipped(SkipReason::UndeclaredPackage),
    }
}

/// Whether `name`, the package part of a bare specifier, can be an npm package name: it starts with
/// a letter, a digit, `@`, `_` or `$` and holds no `:`.
fn is_package_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|first| first.is_alphanumeric() || matches!(first, '@' | '_' | '$'))
        && !name.contains(':')
}

/// Whether `specifier` names a Vite virtual module, one a build plugin generates: `virtual:` and
/// the `\0` prefix of Rollup. No repository file or package is behind it.
fn is_virtual(specifier: &str) -> bool {
    specifier.starts_with("virtual:") || specifier.starts_with('\0')
}

/// What the Vite `resolve.alias` entries say about `specifier` written in `importer`, or `None`
/// when no entry matches. The configs at or above the importer are tried nearest first and the
/// first entry that matches decides: its target, with the rest of the specifier, resolves like a
/// relative path; when it finds no file the target either exists in the repository (an alias with
/// no file behind it, ignored) or does not (skipped, see [`Resolution::AliasTargetMissing`]).
fn vite_alias(
    specifier: &str,
    importer: &Path,
    files: &Files<'_>,
    env: &Env<'_>,
) -> Option<Resolution> {
    let (alias, rest) = importer.parent()?.ancestors().find_map(|dir| {
        env.vite_aliases
            .get(dir)?
            .iter()
            .find_map(|alias| Some((alias, alias.rest(specifier)?)))
    })?;
    let relative = rest.trim_start_matches('/');
    let Some(base) = join_normalised(Some(&alias.target), relative) else {
        return Some(Resolution::Ignored);
    };
    let found = locate(&base, rest.ends_with('/') && !relative.is_empty(), files);
    Some(match found {
        Some(found) => found.resolution(),
        None if has_file_at_or_below(&alias.target, files) => Resolution::Ignored,
        None => Resolution::AliasTargetMissing {
            alias: alias.key.clone(),
            target: alias.target.clone(),
        },
    })
}

/// Whether a known file, source, excluded or asset, is `path` or lies below it.
fn has_file_at_or_below(path: &Path, files: &Files<'_>) -> bool {
    [files.known, files.assets].iter().any(|set| {
        set.range(path.to_path_buf()..)
            .next()
            .is_some_and(|first| first.starts_with(path))
    })
}

/// The alias configuration that governs `importer`: walking up from its directory, the first
/// directory whose solution-style config references a project that includes `importer`, or that
/// holds a config of its own.
fn governing<'a>(env: &Env<'a>, importer: &Path) -> Option<&'a AliasConfig> {
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
fn first_found(candidates: &[Candidate], files: &Files<'_>) -> Option<Found> {
    candidates
        .iter()
        .find_map(|candidate| locate(&candidate.path, candidate.directory_only, files))
}

fn is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
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
