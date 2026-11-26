//! Workspace packages and package `imports`, read from the `package.json` files of a repository.
//!
//! The IO shell is [`discover`]. Everything it returns is plain data that [`Workspaces`] turns into
//! candidate paths for a specifier; the caller probes them. See "Workspace packages" and "Package
//! `imports`" in `docs/DESIGN.md`. A `package.json` that cannot be read or parsed is skipped
//! silently, like a malformed `tsconfig.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::alias::Candidate;
use super::package::package_name;
use super::subpath::Subpaths;
use super::tsconfig::{Output, output_of};
use crate::code::Language;
use crate::reading::syntax::glob;
use crate::reading::syntax::paths::directories;
use crate::reading::syntax::paths::join_normalised;

const MANIFEST_FILE: &str = "package.json";

/// The fields probed, in this order, for the root of a package that has no usable `exports`.
const ENTRY_FIELDS: [&str; 3] = ["module", "main", "types"];

/// The tables of a `package.json` that list dependencies.
const DEPENDENCY_TABLES: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// One workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    /// Where the build output of the member lies and where its sources are, from its tsconfig.
    output: Option<Output>,
    /// Repository-relative directory of the member.
    dir: PathBuf,
    /// Its `exports`, when it has a usable one.
    exports: Option<Subpaths>,
    /// The text of `module`, `main` and `types`, in that order, for those that are strings.
    fields: Vec<String>,
}

/// The workspace members of a repository by package name, and the `imports` maps of its packages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspaces {
    members: BTreeMap<String, Member>,
    imports: BTreeMap<PathBuf, Subpaths>,
    /// For each directory with a `package.json`, the dependencies it writes as a link to a
    /// directory or to another package (`file:`, `link:`, `npm:`, `workspace:`), by key.
    links: BTreeMap<PathBuf, BTreeMap<String, Link>>,
}

/// What a dependency key is linked to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Link {
    /// A directory (`file:../a`, `link:../a`, `workspace:../a`), with its package when the
    /// directory holds a readable `package.json` next to source files.
    Dir(PathBuf, Option<Member>),
    /// A package by name (`npm:@x/a@*`, `workspace:@x/a@*`). Only a `workspace:` one is a claim
    /// that the name is a repository member.
    Named { name: String, workspace: bool },
}

/// What a bare specifier that names a workspace package leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// The paths the specifier may name, in the order to probe them, and what they belong to.
    Paths {
        /// The candidates.
        candidates: Vec<Candidate>,
        /// The package, for a message: `workspace package `@x/a` (`packages/a`)`.
        what: String,
    },
    /// The package is declared as a member or a link, and there is nothing to probe.
    Missing(String),
}

impl Workspaces {
    /// What a bare `specifier` written in `importer` names: the workspace member of that name, or
    /// the package or directory a dependency of that name is linked to. `None` when it names none,
    /// so the specifier is an ordinary package.
    #[must_use]
    pub fn entry(&self, specifier: &str, importer: &Path) -> Option<Entry> {
        let name = package_name(specifier);
        let rest = specifier.get(name.len()..).unwrap_or_default();
        let paths = |member: &Member| Entry::Paths {
            candidates: candidates_of(member, rest),
            what: format!("workspace package `{name}` (`{}`)", member.dir.display()),
        };
        let linked = importer
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .find_map(|dir| self.links.get(dir)?.get(name));
        match linked {
            Some(Link::Dir(_, Some(member))) => Some(paths(member)),
            Some(Link::Dir(dir, None)) => Some(Entry::Missing(format!(
                "`{name}` is a dependency linked to `{}`, which holds no package.json with source files in the repository",
                dir.display()
            ))),
            Some(Link::Named {
                name: target,
                workspace,
            }) => match self.members.get(target) {
                Some(member) => Some(paths(member)),
                None if *workspace => Some(Entry::Missing(format!(
                    "`{name}` is a `workspace:` dependency on `{target}`, which is no workspace member of the repository"
                ))),
                None => None,
            },
            None => self.members.get(name).map(paths),
        }
    }

    /// The paths a `#` `specifier` may name through the `imports` of the nearest `package.json` at
    /// or above `importer` that has one. `None` when there is none or none of its keys matches.
    #[must_use]
    pub fn imports_candidates(&self, specifier: &str, importer: &Path) -> Option<Vec<Candidate>> {
        let (dir, map) = importer
            .parent()?
            .ancestors()
            .find_map(|dir| Some((dir, self.imports.get(dir)?)))?;
        let paths = map.lookup(specifier)?;
        Some(paths.iter().filter_map(|path| inside(dir, path)).collect())
    }
}

/// The paths `rest` (what follows the package name in a specifier) may name inside `member`, in
/// the order to probe them. Each path of the member's own files that is a build output is followed
/// by the source path its tsconfig maps it to.
fn candidates_of(member: &Member, rest: &str) -> Vec<Candidate> {
    let subpath = format!(".{rest}");
    let mut found = Vec::new();
    if let Some(paths) = member
        .exports
        .as_ref()
        .and_then(|exports| exports.lookup(&subpath))
    {
        found.extend(paths.iter().filter_map(|path| inside(&member.dir, path)));
    }
    if rest.is_empty() {
        found.extend(
            member
                .fields
                .iter()
                .filter_map(|field| inside(&member.dir, field)),
        );
        found.push(Candidate {
            path: member.dir.clone(),
            directory_only: true,
        });
    } else if let Some(path) = rest.strip_prefix('/') {
        found.extend(inside(&member.dir, path));
    }
    let Some(output) = &member.output else {
        return found;
    };
    found
        .into_iter()
        .flat_map(|candidate| {
            let sources = sources_of(output, &candidate);
            std::iter::once(candidate).chain(sources)
        })
        .collect()
}

/// The source paths of the build output `candidate`: its place below `outDir` moved below each
/// root, with a declaration file (`index.d.ts`) turned into the file it was made from.
fn sources_of(output: &Output, candidate: &Candidate) -> Vec<Candidate> {
    let Ok(below) = candidate.path.strip_prefix(&output.out) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for root in &output.roots {
        let base = root.join(below);
        let name = base
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // why: a plain `.js` output needs no entry here, since probing already maps it to `.ts`.
        let stems: [(&str, &[&str]); 5] = [
            (".d.ts", &[".ts", ".tsx"]),
            (".d.mts", &[".mts"]),
            (".d.cts", &[".cts"]),
            (".mjs", &[".mts", ".ts", ".tsx"]),
            (".cjs", &[".cts", ".ts", ".tsx"]),
        ];
        let made = stems.iter().find_map(|(declaration, sources)| {
            name.strip_suffix(declaration).map(|stem| (stem, *sources))
        });
        match made {
            Some((stem, sources)) if !candidate.directory_only => {
                for extension in sources {
                    found.push(Candidate {
                        path: base.with_file_name(format!("{stem}{extension}")),
                        directory_only: false,
                    });
                }
            }
            _ => found.push(Candidate {
                path: base,
                directory_only: candidate.directory_only,
            }),
        }
    }
    found
}

/// `text`, a path relative to `dir`, as a candidate. `None` when it would leave `dir`.
fn inside(dir: &Path, text: &str) -> Option<Candidate> {
    let path = join_normalised(Some(dir), text)?;
    path.starts_with(dir).then(|| Candidate {
        path,
        directory_only: text.ends_with('/'),
    })
}

/// Reads the workspace members and package `imports` of the repository under `root`.
///
/// Only the directories at or above a TypeScript or JavaScript file in `files` are looked at, so a
/// member without such a file is not found and ignored trees are never consulted.
#[must_use]
pub fn discover(root: &Path, files: &BTreeMap<PathBuf, Language>) -> Workspaces {
    let sources = files
        .iter()
        .filter(|(_, language)| language.is_ecmascript())
        .map(|(path, _)| path.as_path());
    let dirs = directories(sources);
    let manifests: BTreeMap<&PathBuf, Map<String, Value>> = dirs
        .iter()
        .filter_map(|dir| Some((dir, read_manifest(&root.join(dir).join(MANIFEST_FILE))?)))
        .collect();

    let imports = manifests
        .iter()
        .filter_map(|(dir, manifest)| {
            let map = Subpaths::imports(manifest.get("imports")?)?;
            Some(((*dir).clone(), map))
        })
        .collect();

    let read = |path: &Path| fs::read_to_string(root.join(path)).ok();
    let mut members = BTreeMap::new();
    for (base, manifest) in &manifests {
        let patterns = workspace_patterns(manifest.get("workspaces"));
        for dir in matched_dirs(base, &patterns, &dirs) {
            let Some(member_manifest) = manifests.get(&dir) else {
                continue;
            };
            let Some(name) = member_manifest
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            members
                .entry(name.to_owned())
                .or_insert_with(|| member_of(dir, member_manifest, &read));
        }
    }
    let links = manifests
        .iter()
        .filter_map(|(dir, manifest)| {
            let found = links_of(dir, manifest, &manifests, &read);
            (!found.is_empty()).then(|| ((*dir).clone(), found))
        })
        .collect();
    Workspaces {
        members,
        imports,
        links,
    }
}

/// The dependencies of the manifest in `dir` that are written as a link, by key. The first table
/// that lists a key decides it.
fn links_of(
    dir: &Path,
    manifest: &Map<String, Value>,
    manifests: &BTreeMap<&PathBuf, Map<String, Value>>,
    read: &dyn Fn(&Path) -> Option<String>,
) -> BTreeMap<String, Link> {
    let mut found = BTreeMap::new();
    for table in DEPENDENCY_TABLES {
        let Some(Value::Object(entries)) = manifest.get(table) else {
            continue;
        };
        for (key, value) in entries {
            let link = value
                .as_str()
                .and_then(|text| link_of(key, text, dir, manifests, read));
            if let Some(link) = link {
                found.entry(key.clone()).or_insert(link);
            }
        }
    }
    found
}

/// The link the dependency `key` with version text `text` makes, or `None` for an ordinary
/// version, a tarball, and a link to the package of the same name (which resolves by name anyway).
fn link_of(
    key: &str,
    text: &str,
    dir: &Path,
    manifests: &BTreeMap<&PathBuf, Map<String, Value>>,
    read: &dyn Fn(&Path) -> Option<String>,
) -> Option<Link> {
    let by_dir = |path: &str| {
        let target = join_normalised(Some(dir), path)?;
        let member = manifests
            .get(&target)
            .map(|manifest| member_of(target.clone(), manifest, read));
        Some(Link::Dir(target, member))
    };
    if let Some(path) = text
        .strip_prefix("file:")
        .or_else(|| text.strip_prefix("link:"))
    {
        let tarball = [".tgz", ".tar.gz", ".tar"]
            .iter()
            .any(|extension| path.ends_with(extension));
        return if tarball {
            None
        } else {
            by_dir(path).or_else(|| Some(Link::Dir(PathBuf::from(path), None)))
        };
    }
    let (spec, workspace) = match (text.strip_prefix("npm:"), text.strip_prefix("workspace:")) {
        (Some(spec), _) => (spec, false),
        (None, Some(spec)) => (spec, true),
        (None, None) => return None,
    };
    if workspace && spec.starts_with(['.', '/']) {
        return by_dir(spec).or_else(|| Some(Link::Dir(PathBuf::from(spec), None)));
    }
    let name = match (named(spec), workspace) {
        (Some(name), _) => name,
        // why: `npm:react` names the package whole; `workspace:*` is a range for the key's own name.
        (None, false) => spec,
        (None, true) => return None,
    };
    (name != key).then(|| Link::Named {
        name: name.to_owned(),
        workspace,
    })
}

/// The package name of `name@range` or `@scope/name@range`, or `@scope/name` alone. `None` for a
/// version range with no name, such as `*`, `^1.2.0` or `1.0.0`.
fn named(spec: &str) -> Option<&str> {
    let at = spec
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '@')
        .map(|(index, _)| index);
    at.map_or_else(
        || spec.starts_with('@').then_some(spec),
        |index| spec.get(..index),
    )
}

fn member_of(
    dir: PathBuf,
    manifest: &Map<String, Value>,
    read: &dyn Fn(&Path) -> Option<String>,
) -> Member {
    Member {
        output: output_of(&dir, read),
        dir,
        exports: manifest.get("exports").and_then(Subpaths::exports),
        fields: ENTRY_FIELDS
            .iter()
            .filter_map(|field| manifest.get(*field)?.as_str())
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

fn read_manifest(path: &Path) -> Option<Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    match serde_json::from_str(text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// The patterns of a `workspaces` field: an array of strings, or an object whose `packages` is one.
fn workspace_patterns(value: Option<&Value>) -> Vec<&str> {
    let list = match value {
        Some(Value::Array(list)) => Some(list),
        Some(Value::Object(map)) => match map.get("packages") {
            Some(Value::Array(list)) => Some(list),
            _ => None,
        },
        _ => None,
    };
    list.into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// The directories of `dirs` strictly below `base` that `patterns` select, in order: a pattern
/// adds the directories it matches and a `!` pattern removes them.
fn matched_dirs(base: &Path, patterns: &[&str], dirs: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let mut chosen = BTreeSet::new();
    for pattern in patterns {
        let (removes, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, *pattern), |rest| (true, rest));
        for dir in dirs {
            let Ok(relative) = dir.strip_prefix(base) else {
                continue;
            };
            if relative.as_os_str().is_empty() || !glob::matches(pattern, relative) {
                continue;
            }
            if removes {
                chosen.remove(dir);
            } else {
                chosen.insert(dir.clone());
            }
        }
    }
    chosen
}
