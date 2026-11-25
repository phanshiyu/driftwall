//! Discovery of the packages a repository declares, read from its `package.json` files.
//!
//! The IO shell next to [`super::tsconfig`]: it finds the manifests at or above each TypeScript and
//! JavaScript file and reads the names listed in `dependencies`, `devDependencies`,
//! `peerDependencies` and `optionalDependencies`. A name counts as declared when any manifest in
//! the file's own directory or an ancestor directory, up to the repository root, lists it, because
//! Node searches every ancestor `node_modules`. A manifest that cannot be read or parsed, or that
//! lists a dependency table of the wrong type, is skipped silently, like a malformed
//! `tsconfig.json`: the directory has no manifest and the others still count.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::imports::syntax::paths::directories;
use crate::model::Language;

/// The file that declares a package.
pub(super) const MANIFEST_FILE: &str = "package.json";

/// A dependency table: package name to version, which is never read.
type Table = BTreeMap<String, Value>;

/// The dependency tables of a manifest. Every other field is ignored.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    dependencies: Option<Table>,
    dev_dependencies: Option<Table>,
    peer_dependencies: Option<Table>,
    optional_dependencies: Option<Table>,
}

/// Maps each directory that holds a usable `package.json` to the package names it declares,
/// reading from under `root`.
///
/// Only the directories at or above a TypeScript or JavaScript file in `files` are looked at.
#[must_use]
pub fn package_dependencies(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let sources = files
        .iter()
        .filter(|(_, language)| language.is_ecmascript())
        .map(|(path, _)| path.as_path());
    directories(sources)
        .into_iter()
        .filter_map(|dir| {
            let text = fs::read_to_string(root.join(&dir).join(MANIFEST_FILE)).ok()?;
            let names = declared(&text)?;
            Some((dir, names))
        })
        .collect()
}

/// The package names a manifest declares. `None` for text that is not a manifest.
fn declared(text: &str) -> Option<BTreeSet<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let manifest: Manifest = serde_json::from_str(text).ok()?;
    Some(
        [
            manifest.dependencies,
            manifest.dev_dependencies,
            manifest.peer_dependencies,
            manifest.optional_dependencies,
        ]
        .into_iter()
        .flatten()
        .flat_map(Table::into_keys)
        .collect(),
    )
}

/// The package a bare specifier names: the first segment, or the first two for a scoped name, so
/// `@a/b/sub` is `@a/b` and `pkg/sub` is `pkg`.
pub fn package_name(specifier: &str) -> &str {
    let mut segments = specifier.splitn(3, '/');
    let first = segments.next().unwrap_or(specifier);
    if !first.starts_with('@') {
        return first;
    }
    let end = segments
        .next()
        .map_or(specifier.len(), |second| first.len() + 1 + second.len());
    specifier.get(..end).unwrap_or(specifier)
}

/// The Node builtin modules that can be imported without the `node:` prefix. `test`, `sea` and
/// `sqlite` need the prefix, so a bare `test` is a package.
const BUILTINS: &[&str] = &[
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "domain",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

/// Whether `specifier` names a Node builtin module: a `node:` specifier, or one whose first path
/// segment is a builtin name, so `fs` and `fs/promises` both are.
pub fn is_builtin(specifier: &str) -> bool {
    specifier.starts_with("node:") || BUILTINS.contains(&specifier.split('/').next().unwrap_or(""))
}
