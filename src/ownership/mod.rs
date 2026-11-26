//! Walks the repository and maps every file to its owning node, by the contract's `%% map` lines
//! or, in a repository without them, by `.arch` tag files.

mod arch;
mod census;
mod error;
mod exclude;
mod folded;
mod language;
mod locate;
mod lookup;
mod mapping;
mod mechanism;
mod module;

#[cfg(test)]
mod asset_tests;
#[cfg(test)]
mod census_tests;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead, Read};
use std::path::{Component, Path, PathBuf};

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub use census::{SHEBANG, line_count};
pub use error::{ArchProblem, OwnershipError};
pub use exclude::{HEADER_LINES, announces_generated, exclusion_of};
pub use folded::{declares_nothing, is_entry_file, is_foldable_in, trivial_files};
pub use language::{language_in, language_of};
pub use lookup::owner_in;
pub use mapping::MapContext;
pub use mechanism::{Mechanism, OwnerRule};
pub use module::{folds, is_test_file, module_name, peers_in};

pub use crate::code::{DirTag, Scope, Tag};
use crate::code::{Exclusion, Location, MapSite, Owner, Ownership, Rule};
use crate::contract::{ModelError, NodeId};

/// Name of the file that tags a directory with a node.
const ARCH_FILE: &str = ".arch";
/// Extra gitignore-syntax exclusions, honored in addition to `.gitignore`.
const IGNORE_FILE: &str = ".driftwallignore";
/// What the content of a `.git` file starts with when it points at a repository elsewhere.
const GITDIR_PREFIX: &str = "gitdir:";

/// One `.arch` file's decision about its directory.
struct Decision {
    tag: Tag,
    scope: Scope,
    arch_file: PathBuf,
    line: u32,
    /// The non-test modules directly in the directory, for a `node: *` tag (see [`peers_in`]).
    peers: BTreeSet<String>,
}

impl Decision {
    /// Whether this decision, made in the directory `dir`, applies to `file`: always for a file
    /// directly in `dir`, and for one below it only when the scope is the whole subtree.
    fn covers(&self, dir: &Path, file: &Path) -> bool {
        self.scope == Scope::Subtree || file.parent() == Some(dir)
    }

    /// What this decision, made in the directory `dir` of the `.arch` file, says about `file`.
    /// `trivial` is whether `file` is an entry file that declares nothing.
    fn verdict(&self, dir: &Path, file: &Path, trivial: bool) -> Verdict {
        let derived = matches!(self.tag, Tag::Modules(_));
        let decided = if trivial && derived && folds(dir, file, &self.peers) {
            Decided::Folded
        } else {
            Decided::Node(self.node(dir, file))
        };
        Verdict {
            decided,
            at: self.location(),
            derived,
            site: None,
        }
    }

    /// The node this decision names for `file`, or why a `node: *` name is not one.
    fn node(&self, dir: &Path, file: &Path) -> Result<NodeId, ModelError> {
        match &self.tag {
            Tag::Node(id) => Ok(id.clone()),
            Tag::Modules(prefix) => {
                prefix.derive(&module_name(dir, file, &self.peers).unwrap_or_default())
            }
        }
    }

    /// The node `file` gets from this decision, where `dir` is the directory of the `.arch` file.
    fn node_for(&self, dir: &Path, file: &Path) -> Result<NodeId, OwnershipError> {
        self.node(dir, file)
            .map_err(|source| OwnershipError::InvalidModule {
                at: self.location(),
                file: file.to_path_buf(),
                source,
            })
    }

    fn location(&self) -> Location {
        Location {
            file: self.arch_file.clone(),
            line: self.line,
        }
    }
}

/// Scans `root` and maps every source file to the node named by its nearest `.arch` file.
///
/// A `node: *` file names each source file by its position (see [`module_name`]). Its tags are one
/// entry per distinct derived node, all located at the `node: *` line, so the linter can check
/// each derived name against the contract.
///
/// Vendored, minified and generated files (see [`exclusion_of`] and [`announces_generated`]) are
/// not source: they appear in
/// [`Ownership::excluded`] and nowhere else (and [`Ownership::excluded_owners`], which names the
/// node of each one a tag covers), unless the root `.driftwallignore` whitelists them with a
/// negated pattern.
///
/// Every other walked file is an asset, owned by the same tag rules and listed in
/// [`Ownership::asset_owners`] or [`Ownership::asset_unmapped`]. Assets never appear in
/// [`Ownership::owners`], [`Ownership::unmapped`], [`Ownership::tags`] or [`Ownership::excluded`].
///
/// The census of what is not read (see "Blind spots" in `docs/DESIGN.md`) is [`Ownership::unread`]
/// and [`Ownership::ignored`]. It never changes the maps above.
///
/// Honors `.gitignore` (with or without a `.git` directory) and `.driftwallignore`, and never walks
/// into a directory below the root that holds its own `.git` (a nested repository or worktree). Machine
/// specific ignore sources (global git config, `.git/info/exclude`, parent directories) are
/// deliberately not consulted so the result depends only on the contents of `root`.
///
/// # Errors
/// Fails when the tree cannot be walked, a file cannot be read, a `.arch` file is invalid, or a
/// `node: *` derives a name that is not a valid node id.
fn scan(root: &Path) -> Result<Ownership, OwnershipError> {
    let Discovered {
        sources,
        assets,
        arch_files,
        excluded,
        unread,
        ignored,
    } = discover(root)?;

    // why: an entry file that declares nothing must not name a module, so peers ignore it.
    let trivial = trivial_files(root, sources.iter().map(PathBuf::as_path));
    let mut decisions: BTreeMap<PathBuf, Decision> = BTreeMap::new();
    let mut tags = Vec::new();
    for arch_file in arch_files {
        let parsed = read_tag(root, &arch_file)?;
        let location = Location {
            file: arch_file.clone(),
            line: parsed.line,
        };
        if matches!(parsed.tag, Tag::Node(_)) {
            tags.push((location, parsed.raw));
        }
        let dir = arch_file
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        let peers = match parsed.tag {
            Tag::Modules(_) => peers_without(&dir, &sources, &trivial),
            Tag::Node(_) => BTreeSet::new(),
        };
        decisions.insert(
            dir,
            Decision {
                tag: parsed.tag,
                scope: parsed.scope,
                arch_file,
                line: parsed.line,
                peers,
            },
        );
    }

    let mut ownership = Ownership {
        excluded,
        unread,
        ignored,
        ..Ownership::default()
    };
    let derived = classify(&mut ownership, sources, assets, &trivial, |source| {
        nearest_decision(&decisions, source)
            .map(|(dir, decision)| decision.verdict(dir, source, trivial.contains(source)))
    })?;
    tags.extend(derived);
    tags.sort();
    ownership.tags = tags;
    ownership.tag_dirs = decisions
        .into_iter()
        .map(|(dir, decision)| {
            let tag = DirTag {
                tag: decision.tag,
                scope: decision.scope,
                peers: decision.peers,
            };
            (dir, tag)
        })
        .collect();
    Ok(ownership)
}

/// What a rule says a file is.
enum Decided {
    /// A folded entry file: it has no node.
    Folded,
    /// The node, or why a `node: *` name is not one.
    Node(Result<NodeId, ModelError>),
}

/// What a tag rule decides about one file.
struct Verdict {
    decided: Decided,
    /// Where the rule is written, for an error and for the derived tags.
    at: Location,
    /// Whether the node is derived from a module name (the tag is `node: *`, or a module `%% map`).
    derived: bool,
    /// With `%% map` lines: the glob that decided.
    site: Option<MapSite>,
}

impl Verdict {
    /// The node and glob, `None` for a folded file or a name that is no node id.
    fn into_owner(self) -> Option<(NodeId, Option<MapSite>)> {
        match self.decided {
            Decided::Node(node) => Some((node.ok()?, self.site)),
            Decided::Folded => None,
        }
    }
}

/// Sorts every walked file of the scan into `ownership`: owned, unmapped or folded sources, and
/// the owners of assets and excluded files, by the verdicts of `decide`. A source file no rule
/// covers is folded when it is one of `trivial` (an entry file that declares nothing), else
/// unmapped. Returns the derived tags as (rule location, node name).
fn classify(
    ownership: &mut Ownership,
    sources: Vec<PathBuf>,
    assets: Vec<PathBuf>,
    trivial: &BTreeSet<PathBuf>,
    decide: impl Fn(&Path) -> Option<Verdict>,
) -> Result<BTreeSet<(Location, String)>, OwnershipError> {
    let mut derived: BTreeSet<(Location, String)> = BTreeSet::new();
    for source in sources {
        let Some(verdict) = decide(&source) else {
            if trivial.contains(&source) {
                ownership.folded.insert(source);
            } else {
                ownership.unmapped.insert(source);
            }
            continue;
        };
        let (at, is_derived, site) = (verdict.at, verdict.derived, verdict.site);
        match verdict.decided {
            Decided::Folded => {
                ownership.folded.insert(source);
            }
            Decided::Node(node) => {
                let node = node.map_err(|error| OwnershipError::InvalidModule {
                    at: at.clone(),
                    file: source.clone(),
                    source: error,
                })?;
                if is_derived {
                    derived.insert((at, node.to_string()));
                }
                if let Some(site) = site {
                    ownership.map_sites.insert(source.clone(), site);
                }
                ownership.owners.insert(source, node);
            }
        }
    }
    for asset in assets {
        // why: an asset must never fail the scan, so a name that is no node id leaves it unmapped.
        match decide(&asset).and_then(Verdict::into_owner) {
            Some((node, site)) => {
                if let Some(site) = site {
                    ownership.map_sites.insert(asset.clone(), site);
                }
                ownership.asset_owners.insert(asset, node);
            }
            None => {
                ownership.asset_unmapped.insert(asset);
            }
        }
    }
    // why: like an asset, an excluded file must never fail the scan, so a bad derived name is no owner.
    let excluded: Vec<PathBuf> = ownership.excluded.values().flatten().cloned().collect();
    for file in excluded {
        if let Some((node, site)) = decide(&file).and_then(Verdict::into_owner) {
            if let Some(site) = site {
                ownership.map_sites.insert(file.clone(), site);
            }
            ownership.excluded_owners.insert(file, node);
        }
    }
    Ok(derived)
}

/// The peers of `dir` among `sources`, leaving out the entry files that declare nothing.
fn peers_without(dir: &Path, sources: &[PathBuf], trivial: &BTreeSet<PathBuf>) -> BTreeSet<String> {
    peers_in(
        dir,
        sources
            .iter()
            .filter(|source| !trivial.contains(*source))
            .map(PathBuf::as_path),
    )
}

/// The decision of the nearest `.arch` at or above `file` that covers it (a `scope: here` one only
/// covers the files directly in its directory), with the directory that holds it.
fn nearest_decision<'a>(
    decisions: &'a BTreeMap<PathBuf, Decision>,
    file: &Path,
) -> Option<(&'a Path, &'a Decision)> {
    file.parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .find_map(|dir| {
            let (held, decision) = decisions.get_key_value(dir)?;
            decision
                .covers(held, file)
                .then_some((held.as_path(), decision))
        })
}

/// Explains which node owns `file` and which `.arch` file decided it.
///
/// A relative `file` is taken relative to `root`; an absolute one must lie inside it. The path
/// need not exist or have a source extension. Unlike [`Mechanism::scan`], this reads only the `.arch`
/// files on the path from `file` up to `root`, so ignore files are not consulted and unrelated
/// invalid `.arch` files elsewhere do not matter. Under `node: *` the node is derived from
/// `file`'s position and the `.arch` file reported is the `node: *` one.
///
/// # Errors
/// Fails when `file` is outside `root`, an `.arch` on its path is unreadable or invalid, or a
/// `node: *` derives a name that is not a valid node id.
pub fn explain(root: &Path, file: &Path) -> Result<Owner, OwnershipError> {
    let relative = relative_to_root(root, file)?;
    for dir in relative.parent().into_iter().flat_map(Path::ancestors) {
        let arch_file = dir.join(ARCH_FILE);
        match fs::read_to_string(root.join(&arch_file)) {
            Ok(contents) => {
                let parsed = arch::parse(&arch_file, &contents)?;
                // why: only a test file needs the sibling sources, so only it pays for the walk.
                let own = relative.parent() == Some(dir);
                let entry = is_entry_file(&relative)
                    && declares_nothing(
                        &relative,
                        &fs::read_to_string(root.join(&relative)).unwrap_or_default(),
                    );
                let peers = if matches!(parsed.tag, Tag::Modules(_))
                    && (entry || (own && is_test_file(&relative)))
                {
                    let sources = discover(root)?.sources;
                    let trivial = trivial_files(root, sources.iter().map(PathBuf::as_path));
                    peers_without(dir, &sources, &trivial)
                } else {
                    BTreeSet::new()
                };
                let decision = Decision {
                    tag: parsed.tag,
                    scope: parsed.scope,
                    arch_file,
                    line: parsed.line,
                    peers,
                };
                if decision.covers(dir, &relative) {
                    let folded = entry
                        && matches!(decision.tag, Tag::Modules(_))
                        && folds(dir, &relative, &decision.peers);
                    let node = if folded {
                        None
                    } else {
                        Some(decision.node_for(dir, &relative)?)
                    };
                    let rule = Rule::Tag {
                        arch_file: decision.arch_file,
                    };
                    return Ok(match node {
                        Some(node) => Owner::Owned { node, rule },
                        None => Owner::Folded { rule: Some(rule) },
                    });
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(OwnershipError::Io {
                    path: root.join(&arch_file),
                    source,
                });
            }
        }
    }
    // why: a foldable entry file that declares nothing and that no rule covers is folded, with no rule.
    if is_foldable_in(root, &relative) {
        return Ok(Owner::Folded { rule: None });
    }
    Ok(Owner::Unowned)
}

/// Lexically reduces `file` to a clean path relative to `root`.
fn relative_to_root(root: &Path, file: &Path) -> Result<PathBuf, OwnershipError> {
    let outside = || OwnershipError::OutsideRoot {
        path: file.to_path_buf(),
        root: root.to_path_buf(),
    };
    let stripped = if file.is_absolute() {
        file.strip_prefix(root).map_err(|_| outside())?
    } else {
        file
    };
    let mut clean = PathBuf::new();
    for component in stripped.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(outside());
            }
        }
    }
    Ok(clean)
}

fn read_tag(root: &Path, arch_file: &Path) -> Result<arch::ArchTag, OwnershipError> {
    let full = root.join(arch_file);
    let contents =
        fs::read_to_string(&full).map_err(|source| OwnershipError::Io { path: full, source })?;
    arch::parse(arch_file, &contents)
}

/// What the walk found, root-relative and sorted.
struct Discovered {
    sources: Vec<PathBuf>,
    assets: Vec<PathBuf>,
    arch_files: Vec<PathBuf>,
    excluded: BTreeMap<Exclusion, BTreeSet<PathBuf>>,
    unread: BTreeMap<String, BTreeSet<PathBuf>>,
    ignored: BTreeSet<PathBuf>,
}

/// The patterns of the root `.driftwallignore` that whitelist (`!pattern`) files.
fn opt_ins(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    // why: a missing or partly malformed file only means fewer opt-ins, like the walker's view.
    let _ = builder.add(root.join(IGNORE_FILE));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// The first lines of the file at `path` as text (lossily decoded), or empty when it cannot be read.
fn read_head(path: &Path) -> String {
    // why: a file that cannot be read here is reported by the import collection, with its path.
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    let mut reader = io::BufReader::new(file);
    let mut head = Vec::new();
    for _ in 0..HEADER_LINES {
        match reader.read_until(b'\n', &mut head) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// Whether `entry` is a directory below the root that is a nested repository or a git worktree,
/// which git itself does not walk into either.
fn nested_repository(entry: &ignore::DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_some_and(|kind| kind.is_dir())
        && is_repository(entry.path())
}

/// Whether `dir` is the top of a real git repository: its `.git` is a directory holding `HEAD`, or
/// a file that starts with `gitdir:` (a worktree or a submodule). An empty `.git` file or
/// directory is no repository, and git itself tracks the files around it.
pub(crate) fn is_repository(dir: &Path) -> bool {
    let git = dir.join(".git");
    let Ok(metadata) = fs::metadata(&git) else {
        return false;
    };
    if metadata.is_dir() {
        return git.join("HEAD").is_file();
    }
    let mut start = [0_u8; GITDIR_PREFIX.len()];
    metadata.is_file()
        && fs::File::open(&git)
            .and_then(|mut file| file.read_exact(&mut start))
            .is_ok()
        && start == *GITDIR_PREFIX.as_bytes()
}

/// The walker over `root`. With `driftwallignore` off it still honors `.gitignore`, so the files it
/// finds beyond the normal walk are exactly the ones only `.driftwallignore` hides.
fn walker(root: &Path, driftwallignore: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != OsStr::new(".git") && !nested_repository(entry));
    if driftwallignore {
        builder.add_custom_ignore_filename(IGNORE_FILE);
    }
    builder
}

/// The walked files below `root`, relative and in walk order.
fn walked_files(root: &Path, driftwallignore: bool) -> Result<Vec<PathBuf>, OwnershipError> {
    let mut files = Vec::new();
    for entry in walker(root, driftwallignore).build() {
        let entry = entry.map_err(|source| OwnershipError::Walk {
            root: root.to_path_buf(),
            source,
        })?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if let Ok(relative) = entry.path().strip_prefix(root) {
            files.push(relative.components().collect());
        }
    }
    Ok(files)
}

fn discover(root: &Path) -> Result<Discovered, OwnershipError> {
    let opted_in = opt_ins(root);

    let mut found = Discovered {
        sources: Vec::new(),
        assets: Vec::new(),
        arch_files: Vec::new(),
        excluded: BTreeMap::new(),
        unread: BTreeMap::new(),
        ignored: BTreeSet::new(),
    };
    let mut has_ignore_file = false;
    for relative in walked_files(root, true)? {
        has_ignore_file |= relative.file_name() == Some(OsStr::new(IGNORE_FILE));
        if relative.file_name() == Some(OsStr::new(ARCH_FILE)) {
            found.arch_files.push(relative);
        } else if language::is_source(root, &relative) {
            let reason = exclusion_of(&relative)
                .or_else(|| {
                    announces_generated(&read_head(&root.join(&relative)))
                        .then_some(Exclusion::Generated)
                })
                .filter(|_| {
                    !opted_in
                        .matched_path_or_any_parents(&relative, false)
                        .is_whitelist()
                });
            match reason {
                Some(reason) => {
                    found.excluded.entry(reason).or_default().insert(relative);
                }
                None => found.sources.push(relative),
            }
        } else {
            if let Some(key) = census::unread_key(root, &relative) {
                found
                    .unread
                    .entry(key)
                    .or_default()
                    .insert(relative.clone());
            }
            found.assets.push(relative);
        }
    }
    found.sources.sort();
    found.assets.sort();
    found.arch_files.sort();
    if has_ignore_file {
        found.ignored = hidden_sources(root, &found)?;
    }
    Ok(found)
}

/// The source files that only `.driftwallignore` hides: the ones a second walk without it finds
/// (with `.gitignore` still honored) and the first walk did not.
fn hidden_sources(root: &Path, seen: &Discovered) -> Result<BTreeSet<PathBuf>, OwnershipError> {
    let seen_excluded = |file: &PathBuf| seen.excluded.values().any(|files| files.contains(file));
    Ok(walked_files(root, false)?
        .into_iter()
        .filter(|file| language::is_source(root, file))
        .filter(|file| seen.sources.binary_search(file).is_err() && !seen_excluded(file))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::Exclusion;
    use tempfile::TempDir;

    fn tree(files: &[(&str, &str)]) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    fn owner(ownership: &Ownership, path: &str) -> Option<String> {
        ownership
            .owners
            .get(Path::new(path))
            .map(|node| node.as_str().to_owned())
    }

    fn unmapped(ownership: &Ownership) -> Vec<String> {
        ownership
            .unmapped
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn nested_arch_overrides_the_parent() {
        let dir = tree(&[
            ("a/.arch", "node: outer\n"),
            ("a/x.ts", ""),
            ("a/inner/.arch", "node: inner\n"),
            ("a/inner/y.ts", ""),
            ("a/inner/deep/z.py", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, "a/x.ts").as_deref(), Some("outer"));
        assert_eq!(owner(&o, "a/inner/y.ts").as_deref(), Some("inner"));
        assert_eq!(owner(&o, "a/inner/deep/z.py").as_deref(), Some("inner"));
        assert!(o.unmapped.is_empty());
    }

    #[test]
    fn sibling_directories_inherit_independently() {
        let dir = tree(&[
            ("a/.arch", "node: alpha\n"),
            ("a/one.rs", ""),
            ("b/.arch", "node: beta\n"),
            ("b/two.rs", ""),
            ("c/three.rs", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, "a/one.rs").as_deref(), Some("alpha"));
        assert_eq!(owner(&o, "b/two.rs").as_deref(), Some("beta"));
        assert_eq!(unmapped(&o), ["c/three.rs"]);
    }

    #[test]
    fn untagged_source_files_are_unmapped() {
        let dir = tree(&[("lonely.py", ""), ("pkg/mod.py", "")]);
        let o = scan(dir.path()).unwrap();
        assert!(o.owners.is_empty());
        assert_eq!(unmapped(&o), ["lonely.py", "pkg/mod.py"]);
        assert_eq!(o.tags, Vec::new());
    }

    #[test]
    fn unsupported_extensions_are_not_source() {
        let dir = tree(&[
            ("a/.arch", "node: alpha\n"),
            ("a/README.md", ""),
            ("a/data.json", ""),
            ("a/Makefile", ""),
            ("a/real.ts", ""),
            ("loose.md", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(
            o.owners.keys().collect::<Vec<_>>(),
            [Path::new("a/real.ts")]
        );
        assert!(o.unmapped.is_empty());
    }

    #[test]
    fn gitignore_is_honored_without_a_git_directory() {
        let dir = tree(&[
            (".gitignore", "generated/\n*.gen.ts\n"),
            (".arch", "node: root\n"),
            ("src/keep.ts", ""),
            ("src/skip.gen.ts", ""),
            ("generated/out.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(
            o.owners.keys().collect::<Vec<_>>(),
            [Path::new("src/keep.ts")]
        );
    }

    #[test]
    fn driftwallignore_excludes_files_and_directories() {
        let dir = tree(&[
            (".driftwallignore", "vendor/\nlegacy.js\n"),
            (".arch", "node: root\n"),
            ("vendor/lib.js", ""),
            ("legacy.js", ""),
            ("app.js", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(o.owners.keys().collect::<Vec<_>>(), [Path::new("app.js")]);
        assert!(o.unmapped.is_empty());
    }

    #[test]
    fn ignored_arch_files_do_not_tag_anything() {
        let dir = tree(&[
            (".gitignore", "private/\n"),
            ("private/.arch", "node: secret\n"),
            ("private/x.ts", ""),
            ("public/x.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(o.tags, Vec::new());
        assert_eq!(unmapped(&o), ["public/x.ts"]);
    }

    #[test]
    fn git_directory_is_always_skipped() {
        let dir = tree(&[
            (".arch", "node: root\n"),
            (".git/hooks/pre-commit.py", ""),
            (".git/.arch", "node: bogus\n"),
            ("a.py", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(o.owners.keys().collect::<Vec<_>>(), [Path::new("a.py")]);
        assert_eq!(o.tags.len(), 1);
    }

    #[test]
    fn hidden_directories_are_scanned() {
        let dir = tree(&[(".arch", "node: root\n"), (".config/tool.py", "")]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, ".config/tool.py").as_deref(), Some("root"));
    }

    #[test]
    fn arch_in_the_root_tags_everything() {
        let dir = tree(&[
            (".arch", "node: everything\n"),
            ("a.ts", ""),
            ("b/c/d.py", ""),
            ("e/f.rs", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(o.owners.len(), 3);
        assert!(o.owners.values().all(|n| n.as_str() == "everything"));
        assert!(o.unmapped.is_empty());
    }

    #[test]
    fn every_arch_file_is_recorded_as_a_tag_with_its_line() {
        let dir = tree(&[
            ("b/.arch", "# who\n\nnode: beta\n"),
            ("a/.arch", "node: alpha\n"),
            ("a/x.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(
            o.tags,
            vec![
                (
                    Location {
                        file: PathBuf::from("a/.arch"),
                        line: 1
                    },
                    "alpha".to_owned()
                ),
                (
                    Location {
                        file: PathBuf::from("b/.arch"),
                        line: 3
                    },
                    "beta".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn invalid_arch_files_report_file_and_line() {
        let cases: [(&str, u32, ArchProblem); 4] = [
            ("# only a comment\n", 1, ArchProblem::MissingNode),
            ("node: a\n\nnode: b\n", 3, ArchProblem::DuplicateNode),
            (
                "# hi\nnode: 1bad\n",
                2,
                ArchProblem::InvalidId(crate::contract::ModelError::InvalidNodeId("1bad".into())),
            ),
            (
                "node: a\nwat\n",
                2,
                ArchProblem::UnrecognisedLine("wat".into()),
            ),
        ];
        for (contents, line, expected) in cases {
            let dir = tree(&[("pkg/.arch", contents), ("pkg/a.ts", "")]);
            match scan(dir.path()) {
                Err(OwnershipError::InvalidArch { at, problem }) => {
                    assert_eq!(at.file, Path::new("pkg/.arch"), "{contents:?}");
                    assert_eq!(at.line, line, "{contents:?}");
                    assert_eq!(problem, expected, "{contents:?}");
                }
                other => panic!("expected InvalidArch for {contents:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn invalid_arch_error_message_names_file_and_line() {
        let dir = tree(&[("pkg/.arch", "node: a\nwat\n")]);
        let message = scan(dir.path()).unwrap_err().to_string();
        assert!(message.starts_with("pkg/.arch:2:"), "{message}");
    }

    #[test]
    fn missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(matches!(
            scan(&missing),
            Err(OwnershipError::Walk { .. } | OwnershipError::Io { .. })
        ));
    }

    #[test]
    fn output_is_deterministic_and_sorted() {
        let files = [
            ("z/.arch", "node: zed\n"),
            ("z/b.ts", ""),
            ("z/a.ts", ""),
            ("m/.arch", "node: em\n"),
            ("m/k.py", ""),
            ("a.rs", ""),
            ("b/c.rs", ""),
        ];
        let first = scan(tree(&files).path()).unwrap();
        let mut reversed = files;
        reversed.reverse();
        let second = scan(tree(&reversed).path()).unwrap();
        assert_eq!(first, second);

        let owners: Vec<_> = first.owners.keys().collect();
        let mut sorted = owners.clone();
        sorted.sort();
        assert_eq!(owners, sorted);
        assert_eq!(
            first
                .tags
                .iter()
                .map(|(l, _)| l.file.clone())
                .collect::<Vec<_>>(),
            [PathBuf::from("m/.arch"), PathBuf::from("z/.arch")]
        );
        assert_eq!(unmapped(&first), ["a.rs", "b/c.rs"]);
    }

    #[test]
    fn explain_reports_the_deciding_arch_file() {
        let dir = tree(&[
            (".arch", "node: root\n"),
            ("a/.arch", "node: outer\n"),
            ("a/x.ts", ""),
            ("a/inner/.arch", "node: inner\n"),
            ("a/inner/y.ts", ""),
            ("a/other/z.ts", ""),
            ("top.py", ""),
        ]);
        let explain_of = |path: &str| explain(dir.path(), Path::new(path)).unwrap();

        let inner = explain_of("a/inner/y.ts");
        assert_eq!(inner.node().map(NodeId::as_str), Some("inner"));
        assert_eq!(inner.arch_file(), Some(Path::new("a/inner/.arch")));

        let inherited = explain_of("a/other/z.ts");
        assert_eq!(inherited.node().map(NodeId::as_str), Some("outer"));
        assert_eq!(inherited.arch_file(), Some(Path::new("a/.arch")));

        let top = explain_of("top.py");
        assert_eq!(top.node().map(NodeId::as_str), Some("root"));
        assert_eq!(top.arch_file(), Some(Path::new(".arch")));
    }

    #[test]
    fn explain_agrees_with_scan() {
        let dir = tree(&[
            ("a/.arch", "node: alpha\n"),
            ("a/b/c.ts", ""),
            ("d/e.ts", ""),
        ]);
        let scanned = scan(dir.path()).unwrap();
        for (path, node) in &scanned.owners {
            let explanation = explain(dir.path(), path).unwrap();
            assert_eq!(explanation.node(), Some(node));
        }
        for path in &scanned.unmapped {
            let explanation = explain(dir.path(), path).unwrap();
            assert_eq!(explanation.node(), None);
            assert_eq!(explanation.arch_file(), None);
        }
    }

    #[test]
    fn explain_unmapped_returns_none() {
        let dir = tree(&[("x/y.ts", "")]);
        let explanation = explain(dir.path(), Path::new("x/y.ts")).unwrap();
        assert_eq!(explanation, Owner::Unowned);
    }

    #[test]
    fn an_entry_file_that_declares_nothing_is_folded_unless_a_module_has_its_name() {
        let dir = tree(&[
            ("src/.arch", "node: *\n"),
            ("src/a.py", "x = 1\n"),
            ("src/b.py", "x = 1\n"),
            ("src/__init__.py", "\"\"\"doc\"\"\"\n"),
            ("src/lib.rs", "pub mod a;\n"),
            ("src/sub/__init__.py", ""),
            ("src/sub/x.py", "x = 1\n"),
            ("src/lone/__init__.py", ""),
            ("src/main.rs", "fn main() {}\n"),
        ]);
        let found = scan(dir.path()).unwrap();
        let folded: Vec<&str> = found.folded.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(
            folded,
            ["src/__init__.py", "src/lib.rs", "src/lone/__init__.py"]
        );
        assert!(!found.owners.contains_key(Path::new("src/lib.rs")));
        assert!(!found.unmapped.contains(Path::new("src/lib.rs")));
        let owner = |file: &str| found.owners.get(Path::new(file)).map(NodeId::as_str);
        assert_eq!(owner("src/sub/__init__.py"), Some("sub"));
        assert_eq!(owner("src/main.rs"), Some("main"));
        let names: Vec<&str> = found.tags.iter().map(|(_, name)| name.as_str()).collect();
        assert_eq!(names, ["a", "b", "main", "sub"]);
    }

    #[test]
    fn a_folded_entry_file_is_answered_by_explain_and_a_named_module_keeps_it() {
        let dir = tree(&[
            ("src/.arch", "node: *\n"),
            ("src/a.py", "x = 1\n"),
            ("src/lib.rs", "pub mod a;\n"),
            ("src/lib/x.rs", "pub fn f() {}\n"),
            ("src/__init__.py", ""),
        ]);
        let folded = explain(dir.path(), Path::new("src/__init__.py")).unwrap();
        assert!(folded.is_folded() && folded.node().is_none());
        assert_eq!(folded.arch_file(), Some(Path::new("src/.arch")));
        let named = explain(dir.path(), Path::new("src/lib.rs")).unwrap();
        assert!(!named.is_folded());
        assert_eq!(named.node().map(NodeId::as_str), Some("lib"));
        let found = scan(dir.path()).unwrap();
        assert_eq!(
            found
                .owners
                .get(Path::new("src/lib.rs"))
                .map(NodeId::as_str),
            Some("lib")
        );
    }

    #[test]
    fn an_uncovered_entry_file_that_declares_nothing_is_folded_and_explained_without_a_rule() {
        let dir = tree(&[
            ("a/.arch", "node: alpha\n"),
            ("a/x.rs", "pub fn x() {}\n"),
            ("b/mod.rs", "mod c;\n"),
            ("b/lib.rs", "pub mod c;\n"),
            ("b/main.rs", "pub use c::C;\n"),
            ("pkg/__init__.py", ""),
            ("pkg/mod.py", ""),
            ("a/mod.rs", "mod x;\n"),
        ]);
        let found = scan(dir.path()).unwrap();
        let folded: Vec<&str> = found.folded.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(folded, ["b/lib.rs", "b/mod.rs", "pkg/__init__.py"]);
        let unmapped: Vec<&str> = found.unmapped.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(unmapped, ["b/main.rs", "pkg/mod.py"]);
        let covered = found.owners.get(Path::new("a/mod.rs")).map(NodeId::as_str);
        assert_eq!(covered, Some("alpha"));
        let answer = explain(dir.path(), Path::new("b/mod.rs")).unwrap();
        assert_eq!(answer, Owner::Folded { rule: None });
        assert!(answer.rule().is_none() && answer.node().is_none());
        assert_eq!(
            explain(dir.path(), Path::new("b/main.rs")).unwrap(),
            Owner::Unowned
        );
        assert_eq!(
            explain(dir.path(), Path::new("b/absent/mod.rs")).unwrap(),
            Owner::Unowned
        );
    }

    #[test]
    fn a_declarations_only_mod_rs_under_node_star_folds_unless_a_module_has_its_name() {
        let dir = tree(&[
            ("src/.arch", "node: *\n"),
            ("src/mod.rs", "mod a;\n"),
            ("src/a.rs", "pub fn a() {}\n"),
            ("src/lone/mod.rs", "mod x;\n"),
            ("src/named/mod.rs", "mod x;\n"),
            ("src/named/x.rs", "pub fn x() {}\n"),
            ("src/code/mod.rs", "pub fn f() {}\n"),
        ]);
        let found = scan(dir.path()).unwrap();
        let folded: Vec<&str> = found.folded.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(folded, ["src/lone/mod.rs", "src/mod.rs"]);
        let owner = |file: &str| found.owners.get(Path::new(file)).map(NodeId::as_str);
        assert_eq!(owner("src/named/mod.rs"), Some("named"));
        assert_eq!(owner("src/code/mod.rs"), Some("code"));
    }

    #[test]
    fn an_entry_file_under_a_plain_node_tag_is_never_folded() {
        let dir = tree(&[("a/.arch", "node: alpha\n"), ("a/__init__.py", "")]);
        let found = scan(dir.path()).unwrap();
        assert!(found.folded.is_empty());
        assert_eq!(found.owners.len(), 1);
    }

    #[test]
    fn a_nested_repository_or_worktree_is_not_walked() {
        let dir = tree(&[
            ("src/.arch", "node: app\n"),
            ("src/a.ts", ""),
            // why: a git worktree holds a `.git` file, a nested clone a `.git` directory.
            (
                ".claude/worktrees/agent/.git",
                "gitdir: ../../../.git/worktrees/agent\n",
            ),
            (".claude/worktrees/agent/src/a.ts", ""),
            ("vendor/lib/.git/HEAD", "ref: refs/heads/main\n"),
            ("vendor/lib/b.ts", ""),
        ]);
        let found = scan(dir.path()).unwrap();
        let walked: Vec<&Path> = found
            .owners
            .keys()
            .chain(&found.unmapped)
            .map(PathBuf::as_path)
            .collect();
        assert_eq!(walked, [Path::new("src/a.ts")]);
    }

    #[test]
    fn an_empty_or_bogus_git_entry_does_not_hide_a_subtree() {
        let dir = tree(&[
            ("a/.git", ""),
            ("a/x.ts", ""),
            ("b/.git/placeholder", ""),
            ("b/y.ts", ""),
            ("c/.git", "not a pointer\n"),
            ("c/z.ts", ""),
            ("d/.git", "gitdir"),
            ("d/w.ts", ""),
        ]);
        let found = scan(dir.path()).unwrap();
        let walked: Vec<&Path> = found.unmapped.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            walked,
            [
                Path::new("a/x.ts"),
                Path::new("b/y.ts"),
                Path::new("c/z.ts"),
                Path::new("d/w.ts"),
            ]
        );
    }

    #[test]
    fn only_a_gitdir_file_or_a_directory_with_head_is_a_repository() {
        let dir = tree(&[
            ("file/.git", "gitdir: ../elsewhere\n"),
            ("dir/.git/HEAD", "ref: refs/heads/main\n"),
            ("empty/.git", ""),
            ("hollow/.git/config", ""),
        ]);
        let is = |name: &str| is_repository(&dir.path().join(name));
        assert!(is("file") && is("dir"));
        assert!(!is("empty") && !is("hollow") && !is("missing"));
    }

    #[test]
    fn explain_accepts_absolute_and_dotted_paths() {
        let dir = tree(&[("a/.arch", "node: alpha\n"), ("a/x.ts", "")]);
        let absolute = explain(dir.path(), &dir.path().join("a/x.ts")).unwrap();
        let dotted = explain(dir.path(), Path::new("./a/x.ts")).unwrap();
        assert_eq!(absolute, dotted);
        assert_eq!(absolute.arch_file(), Some(Path::new("a/.arch")));
    }

    #[test]
    fn explain_rejects_paths_outside_the_root() {
        let dir = tree(&[("a/.arch", "node: alpha\n")]);
        for path in [Path::new("../x.ts"), Path::new("a/../../x.ts")] {
            assert!(matches!(
                explain(dir.path(), path),
                Err(OwnershipError::OutsideRoot { .. })
            ));
        }
        let elsewhere = tempfile::tempdir().unwrap();
        assert!(matches!(
            explain(dir.path(), &elsewhere.path().join("x.ts")),
            Err(OwnershipError::OutsideRoot { .. })
        ));
    }

    #[test]
    fn explain_reports_invalid_arch_on_the_path() {
        let dir = tree(&[("a/.arch", "node: a\nnode: b\n")]);
        match explain(dir.path(), Path::new("a/x.ts")) {
            Err(OwnershipError::InvalidArch { at, problem }) => {
                assert_eq!(at.file, Path::new("a/.arch"));
                assert_eq!(at.line, 2);
                assert_eq!(problem, ArchProblem::DuplicateNode);
            }
            other => panic!("expected InvalidArch, got {other:?}"),
        }
    }

    /// Maps each path to its owner, as plain strings, for a tree whose root `.arch` is `node: *`.
    fn derived(paths: &[&str]) -> Vec<(String, String)> {
        let mut files: Vec<(&str, &str)> = vec![("src/.arch", "node: *\n")];
        // why: an entry file with no statement is folded, which these tests do not exercise.
        files.extend(paths.iter().map(|path| (*path, "x\n")));
        let dir = tree(&files);
        let o = scan(dir.path()).unwrap();
        assert!(o.unmapped.is_empty(), "{:?}", o.unmapped);
        o.owners
            .iter()
            .map(|(path, node)| (path.display().to_string(), node.to_string()))
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        expected
            .iter()
            .map(|(path, node)| ((*path).to_owned(), (*node).to_owned()))
            .collect()
    }

    #[test]
    fn star_names_flat_files_by_their_stem() {
        assert_eq!(
            derived(&["src/gitlab.rs", "src/store.ts", "src/auth.py"]),
            pairs(&[
                ("src/auth.py", "auth"),
                ("src/gitlab.rs", "gitlab"),
                ("src/store.ts", "store"),
            ])
        );
    }

    #[test]
    fn star_joins_test_suffixed_files_to_their_module() {
        assert_eq!(
            derived(&["src/a.ts", "src/a.test.ts", "src/a.spec.tsx", "src/b.ts"]),
            pairs(&[
                ("src/a.spec.tsx", "a"),
                ("src/a.test.ts", "a"),
                ("src/a.ts", "a"),
                ("src/b.ts", "b"),
            ])
        );
    }

    #[test]
    fn star_names_main_and_lib_by_their_stem() {
        assert_eq!(
            derived(&["src/main.rs", "src/lib.rs"]),
            pairs(&[("src/lib.rs", "lib"), ("src/main.rs", "main")])
        );
    }

    #[test]
    fn star_names_directory_entries_after_the_tagged_directory() {
        assert_eq!(
            derived(&[
                "src/mod.rs",
                "src/__init__.py",
                "src/index.ts",
                "src/index.d.ts",
            ]),
            pairs(&[
                ("src/__init__.py", "src"),
                ("src/index.d.ts", "src"),
                ("src/index.ts", "src"),
                ("src/mod.rs", "src"),
            ])
        );
    }

    #[test]
    fn star_names_entries_in_the_repository_root_root() {
        let dir = tree(&[(".arch", "node: *\n"), ("index.ts", ""), ("a.ts", "")]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, "index.ts").as_deref(), Some("root"));
        assert_eq!(owner(&o, "a.ts").as_deref(), Some("a"));
    }

    #[test]
    fn star_makes_a_sibling_module_and_its_directory_one_node() {
        assert_eq!(
            derived(&[
                "src/gitlab.rs",
                "src/gitlab/client.rs",
                "src/gitlab/deep/x.rs"
            ]),
            pairs(&[
                ("src/gitlab/client.rs", "gitlab"),
                ("src/gitlab/deep/x.rs", "gitlab"),
                ("src/gitlab.rs", "gitlab"),
            ])
        );
    }

    #[test]
    fn star_makes_a_plain_subdirectory_one_node() {
        assert_eq!(
            derived(&[
                "src/routes/a.rs",
                "src/routes/mod.rs",
                "src/routes/deep/b.rs",
                "src/ui/index.ts"
            ]),
            pairs(&[
                ("src/routes/a.rs", "routes"),
                ("src/routes/deep/b.rs", "routes"),
                ("src/routes/mod.rs", "routes"),
                ("src/ui/index.ts", "ui"),
            ])
        );
    }

    #[test]
    fn a_nested_arch_overrides_inside_a_star_tree() {
        let dir = tree(&[
            ("src/.arch", "node: *\n"),
            ("src/a.rs", ""),
            ("src/legacy/.arch", "node: third\n"),
            ("src/legacy/x.rs", ""),
            ("src/legacy/deep/y.rs", ""),
            ("src/split/.arch", "node: *\n"),
            ("src/split/p.rs", ""),
            ("src/split/q/r.rs", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, "src/a.rs").as_deref(), Some("a"));
        assert_eq!(owner(&o, "src/legacy/x.rs").as_deref(), Some("third"));
        assert_eq!(owner(&o, "src/legacy/deep/y.rs").as_deref(), Some("third"));
        assert_eq!(owner(&o, "src/split/p.rs").as_deref(), Some("p"));
        assert_eq!(owner(&o, "src/split/q/r.rs").as_deref(), Some("q"));
    }

    #[test]
    fn star_reports_an_invalid_derived_id_at_the_arch_file() {
        for bad in ["_.py", "__.py", ".eslintrc.js", "9lives.ts", "a b.ts"] {
            let dir = tree(&[
                ("pkg/.arch", "# c\nnode: *\n"),
                ("pkg/ok.py", ""),
                (&format!("pkg/{bad}"), ""),
            ]);
            match scan(dir.path()) {
                Err(OwnershipError::InvalidModule { at, file, .. }) => {
                    assert_eq!(at.file, Path::new("pkg/.arch"), "{bad}");
                    assert_eq!(at.line, 2, "{bad}");
                    assert_eq!(file, Path::new("pkg").join(bad), "{bad}");
                }
                other => panic!("expected InvalidModule for {bad}, got {other:?}"),
            }
        }
    }

    #[test]
    fn star_names_python_entries_and_private_modules_without_underscores() {
        assert_eq!(
            derived(&[
                "src/__init__.py",
                "src/__main__.py",
                "src/_private.py",
                "src/__dunder.py",
                "src/_x/y.py",
                "src/x.py",
            ]),
            pairs(&[
                ("src/__dunder.py", "dunder"),
                ("src/__init__.py", "src"),
                ("src/__main__.py", "src"),
                ("src/_private.py", "private"),
                ("src/_x/y.py", "x"),
                ("src/x.py", "x"),
            ])
        );
    }

    #[test]
    fn star_reports_an_invalid_directory_name() {
        let dir = tree(&[(".arch", "node: *\n"), ("my.pkg/a.py", "")]);
        let message = scan(dir.path()).unwrap_err().to_string();
        assert_eq!(message, ".arch:1: `node: *` cannot name my.pkg/a.py");
    }

    #[test]
    fn star_emits_one_tag_per_distinct_derived_node() {
        let dir = tree(&[
            ("src/.arch", "# c\n\nnode: *\n"),
            ("src/b.ts", ""),
            ("src/a.ts", ""),
            ("src/a.test.ts", ""),
            ("src/gitlab.rs", ""),
            ("src/gitlab/x.rs", ""),
            ("z/.arch", "node: zed\n"),
            ("z/k.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        let at = |file: &str, line| Location {
            file: PathBuf::from(file),
            line,
        };
        assert_eq!(
            o.tags,
            vec![
                (at("src/.arch", 3), "a".to_owned()),
                (at("src/.arch", 3), "b".to_owned()),
                (at("src/.arch", 3), "gitlab".to_owned()),
                (at("z/.arch", 1), "zed".to_owned()),
            ]
        );
    }

    #[test]
    fn star_without_sources_emits_no_tags() {
        let dir = tree(&[("src/.arch", "node: *\n"), ("src/README.md", "")]);
        assert_eq!(scan(dir.path()).unwrap().tags, Vec::new());
    }

    #[test]
    fn star_output_is_deterministic() {
        let files = [
            ("src/.arch", "node: *\n"),
            ("src/b.rs", ""),
            ("src/a/x.rs", ""),
            ("src/a.rs", ""),
            ("src/c/.arch", "node: sealed\n"),
            ("src/c/y.rs", ""),
        ];
        let first = scan(tree(&files).path()).unwrap();
        let mut reversed = files;
        reversed.reverse();
        assert_eq!(first, scan(tree(&reversed).path()).unwrap());
    }

    #[test]
    fn explain_derives_the_node_and_names_the_deciding_arch_file() {
        let dir = tree(&[
            ("src/.arch", "node: *\n"),
            ("src/gitlab.rs", ""),
            ("src/main.rs", "fn main() {}\n"),
            ("src/routes/a.rs", ""),
            ("src/routes/inner/.arch", "node: inner\n"),
            ("src/routes/inner/b.rs", ""),
        ]);
        let explain_of = |path: &str| explain(dir.path(), Path::new(path)).unwrap();
        let expect = |path: &str, node: &str, arch: &str| {
            let explanation = explain_of(path);
            assert_eq!(explanation.node().map(NodeId::as_str), Some(node), "{path}");
            assert_eq!(explanation.arch_file(), Some(Path::new(arch)), "{path}");
        };
        expect("src/gitlab.rs", "gitlab", "src/.arch");
        expect("src/gitlab/new.rs", "gitlab", "src/.arch");
        expect("src/routes/a.rs", "routes", "src/.arch");
        expect("src/routes/inner/b.rs", "inner", "src/routes/inner/.arch");
        expect("src/main.rs", "main", "src/.arch");
    }

    #[test]
    fn explain_agrees_with_scan_under_star() {
        let dir = tree(&[
            ("src/.arch", "node: *\n"),
            ("src/a.ts", ""),
            ("src/a.test.ts", ""),
            ("src/index.ts", ""),
            ("src/d/e.ts", ""),
            ("src/d/f/g.ts", ""),
        ]);
        let scanned = scan(dir.path()).unwrap();
        for (path, node) in &scanned.owners {
            let explanation = explain(dir.path(), path).unwrap();
            assert_eq!(explanation.node(), Some(node), "{}", path.display());
        }
    }

    #[test]
    fn explain_reports_an_invalid_derived_id() {
        let dir = tree(&[("src/.arch", "node: *\n")]);
        match explain(dir.path(), Path::new("src/__.py")) {
            Err(OwnershipError::InvalidModule { at, .. }) => {
                assert_eq!(at.file, Path::new("src/.arch"));
                assert_eq!(at.line, 1);
            }
            other => panic!("expected InvalidModule, got {other:?}"),
        }
    }

    #[test]
    fn a_prefix_is_put_in_front_of_every_derived_node() {
        assert_eq!(
            derived_with(
                "ui-*",
                &[
                    "src/store.ts",
                    "src/store.test.ts",
                    "src/index.ts",
                    "src/components/a.ts",
                    "src/components/deep/b.ts",
                    "src/9lives.ts",
                ]
            ),
            pairs(&[
                ("src/9lives.ts", "ui-9lives"),
                ("src/components/a.ts", "ui-components"),
                ("src/components/deep/b.ts", "ui-components"),
                ("src/index.ts", "ui-src"),
                ("src/store.test.ts", "ui-store"),
                ("src/store.ts", "ui-store"),
            ])
        );
    }

    /// Like [`derived`], with the `node:` value of `src/.arch` given.
    fn derived_with(value: &str, paths: &[&str]) -> Vec<(String, String)> {
        let arch = format!("node: {value}\n");
        let mut files: Vec<(&str, &str)> = vec![("src/.arch", &arch)];
        files.extend(paths.iter().map(|path| (*path, "")));
        let dir = tree(&files);
        let o = scan(dir.path()).unwrap();
        assert!(o.unmapped.is_empty(), "{:?}", o.unmapped);
        o.owners
            .iter()
            .map(|(path, node)| (path.display().to_string(), node.to_string()))
            .collect()
    }

    #[test]
    fn prefixed_tags_name_the_prefixed_nodes_at_the_star_line() {
        let dir = tree(&[
            ("ui/.arch", "# c\nnode: ui-*\n"),
            ("ui/b.ts", ""),
            ("ui/a/x.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        let at = |line| Location {
            file: PathBuf::from("ui/.arch"),
            line,
        };
        assert_eq!(
            o.tags,
            vec![(at(2), "ui-a".to_owned()), (at(2), "ui-b".to_owned())]
        );
    }

    #[test]
    fn a_prefixed_name_must_still_be_non_empty_and_valid() {
        for bad in [".eslintrc.js", "a b.ts", "_.py"] {
            let dir = tree(&[
                ("pkg/.arch", "node: ui-*\n"),
                ("pkg/ok.py", ""),
                (&format!("pkg/{bad}"), ""),
            ]);
            assert!(
                matches!(scan(dir.path()), Err(OwnershipError::InvalidModule { .. })),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_here_tag_does_not_reach_subdirectories() {
        let dir = tree(&[
            (".arch", "node: root\nscope: here\n"),
            ("vite.config.ts", ""),
            ("src/a.ts", ""),
            ("src/deep/b.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, "vite.config.ts").as_deref(), Some("root"));
        assert_eq!(unmapped(&o), ["src/a.ts", "src/deep/b.ts"]);
    }

    #[test]
    fn a_here_tag_leaves_subdirectories_to_the_tag_above_it() {
        let dir = tree(&[
            ("a/.arch", "node: outer\n"),
            ("a/x.ts", ""),
            ("a/b/.arch", "node: inner\nscope: here\n"),
            ("a/b/y.ts", ""),
            ("a/b/deep/z.ts", ""),
            ("a/b/deep/.arch", "node: nearest\n"),
            ("a/b/deep/w.ts", ""),
            ("a/b/other/v.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, "a/b/y.ts").as_deref(), Some("inner"));
        assert_eq!(owner(&o, "a/b/other/v.ts").as_deref(), Some("outer"));
        assert_eq!(owner(&o, "a/b/deep/w.ts").as_deref(), Some("nearest"));
        assert!(o.unmapped.is_empty());
    }

    #[test]
    fn a_here_star_tag_names_only_the_files_directly_in_its_directory() {
        let dir = tree(&[
            ("src/.arch", "node: ui-*\nscope: here\n"),
            ("src/a.ts", ""),
            ("src/b/c.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(owner(&o, "src/a.ts").as_deref(), Some("ui-a"));
        assert_eq!(unmapped(&o), ["src/b/c.ts"]);
    }

    #[test]
    fn the_scan_records_the_scope_of_each_tag_directory() {
        let dir = tree(&[
            (".arch", "node: root\nscope: here\n"),
            ("a/.arch", "node: ui-*\n"),
            ("a/x.ts", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        let scope_of = |dir: &str| o.tag_dirs.get(Path::new(dir)).map(|tag| tag.scope);
        assert_eq!(scope_of(""), Some(Scope::Here));
        assert_eq!(scope_of("a"), Some(Scope::Subtree));
        assert_eq!(
            o.tag_dirs
                .get(Path::new("a"))
                .map(|tag| tag.tag.to_string()),
            Some("ui-*".to_owned())
        );
    }

    #[test]
    fn explain_agrees_with_scan_for_scopes_and_prefixes() {
        let dir = tree(&[
            (".arch", "node: root\nscope: here\n"),
            ("top.ts", ""),
            ("lone/x.ts", ""),
            ("ui/.arch", "node: ui-*\n"),
            ("ui/store.ts", ""),
            ("ui/components/a.ts", ""),
            ("ui/components/deep/.arch", "node: inner\nscope: here\n"),
            ("ui/components/deep/b.ts", ""),
            ("ui/components/deep/more/c.ts", ""),
            ("pkg/.arch", "node: p-*\nscope: here\n"),
            ("pkg/m.ts", ""),
            ("pkg/sub/n.ts", ""),
        ]);
        let scanned = scan(dir.path()).unwrap();
        assert!(!scanned.owners.is_empty() && !scanned.unmapped.is_empty());
        for (path, node) in &scanned.owners {
            let explanation = explain(dir.path(), path).unwrap();
            assert_eq!(explanation.node(), Some(node), "{}", path.display());
        }
        for path in &scanned.unmapped {
            let explanation = explain(dir.path(), path).unwrap();
            assert_eq!(explanation.node(), None, "{}", path.display());
            assert_eq!(explanation.arch_file(), None, "{}", path.display());
        }
        assert_eq!(unmapped(&scanned), ["lone/x.ts", "pkg/sub/n.ts"]);
    }

    #[test]
    fn explain_names_the_arch_file_that_covers_a_file_below_a_here_tag() {
        let dir = tree(&[
            ("a/.arch", "node: outer\n"),
            ("a/b/.arch", "node: inner\nscope: here\n"),
        ]);
        let explain_of = |path: &str| explain(dir.path(), Path::new(path)).unwrap();
        let direct = explain_of("a/b/y.ts");
        assert_eq!(direct.node().map(NodeId::as_str), Some("inner"));
        assert_eq!(direct.arch_file(), Some(Path::new("a/b/.arch")));
        let below = explain_of("a/b/new/z.ts");
        assert_eq!(below.node().map(NodeId::as_str), Some("outer"));
        assert_eq!(below.arch_file(), Some(Path::new("a/.arch")));
    }

    #[test]
    fn explain_derives_a_prefixed_node() {
        let dir = tree(&[("ui/.arch", "node: ui-*\n")]);
        let explanation = explain(dir.path(), Path::new("ui/components/new.ts")).unwrap();
        assert_eq!(
            explanation.node().map(NodeId::as_str),
            Some("ui-components")
        );
        assert_eq!(explanation.arch_file(), Some(Path::new("ui/.arch")));
    }

    fn excluded(ownership: &Ownership, reason: Exclusion) -> Vec<String> {
        ownership
            .excluded
            .get(&reason)
            .into_iter()
            .flatten()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn minified_and_vendored_files_are_counted_and_not_owned() {
        let dir = tree(&[
            (".arch", "node: app\n"),
            ("src/app.js", ""),
            ("static/htmx.min.js", ""),
            ("static/site.min.css", ""),
            ("ui/vendor/basecoat/basecoat.min.js", ""),
            ("ui/vendor/other.js", ""),
            ("third_party/lib.py", ""),
            ("vendor/z.rs", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(
            o.owners.keys().collect::<Vec<_>>(),
            [Path::new("src/app.js")]
        );
        assert!(o.unmapped.is_empty());
        assert_eq!(excluded(&o, Exclusion::Minified), ["static/htmx.min.js"]);
        assert_eq!(
            excluded(&o, Exclusion::Vendored),
            [
                "third_party/lib.py",
                "ui/vendor/basecoat/basecoat.min.js",
                "ui/vendor/other.js",
                "vendor/z.rs"
            ]
        );
    }

    #[test]
    fn a_negated_pattern_in_the_root_driftwallignore_opts_files_back_in() {
        let dir = tree(&[
            (".driftwallignore", "!vendor/mine/\n!static/keep.min.js\n"),
            (".arch", "node: app\n"),
            ("vendor/mine/a.js", ""),
            ("vendor/theirs/b.js", ""),
            ("static/keep.min.js", ""),
            ("static/drop.min.js", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(
            o.owners.keys().collect::<Vec<_>>(),
            [
                Path::new("static/keep.min.js"),
                Path::new("vendor/mine/a.js")
            ]
        );
        assert_eq!(excluded(&o, Exclusion::Minified), ["static/drop.min.js"]);
        assert_eq!(excluded(&o, Exclusion::Vendored), ["vendor/theirs/b.js"]);
    }

    #[test]
    fn dist_directories_and_generated_headers_are_counted_as_generated() {
        let dir = tree(&[
            (".arch", "node: app\n"),
            ("src/app.ts", "export const a = 1;\n"),
            ("ui/dist/assets/index-4f9a1c.js", "var a=1;"),
            ("dist/x.py", "X = 1\n"),
            (
                "apps/mac/Resources/audio-page.js",
                "// Generated from audio.html.\n// Do not edit.\nexport const p = 1;\n",
            ),
            (
                "src/api.ts",
                "/** @generated by codegen */\nexport const api = 1;\n",
            ),
            (
                "src/late.ts",
                "\n\n\n\n\n// do not edit\nexport const late = 1;\n",
            ),
            (
                "src/note.py",
                "# See docs; AUTO-GENERATED tables below\nX = 1\n",
            ),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(
            excluded(&o, Exclusion::Generated),
            [
                "apps/mac/Resources/audio-page.js",
                "dist/x.py",
                "src/api.ts",
                "src/note.py",
                "ui/dist/assets/index-4f9a1c.js",
            ]
        );
        assert_eq!(
            o.owners.keys().collect::<Vec<_>>(),
            [Path::new("src/app.ts"), Path::new("src/late.ts")]
        );
        assert!(o.unmapped.is_empty());
    }

    #[test]
    fn a_negated_pattern_opts_generated_files_back_in() {
        let dir = tree(&[
            (".driftwallignore", "!/dist/\n!src/api.ts\n"),
            (".arch", "node: app\n"),
            ("dist/x.js", ""),
            ("other/dist/y.js", ""),
            ("src/api.ts", "// @generated\n"),
            ("src/b.ts", "// @generated\n"),
        ]);
        let o = scan(dir.path()).unwrap();
        assert_eq!(
            o.owners.keys().collect::<Vec<_>>(),
            [Path::new("dist/x.js"), Path::new("src/api.ts")]
        );
        assert_eq!(
            excluded(&o, Exclusion::Generated),
            ["other/dist/y.js", "src/b.ts"]
        );
    }

    #[test]
    fn a_file_that_is_not_utf8_is_still_checked_for_a_generated_header() {
        let dir = tree(&[(".arch", "node: app\n"), ("a.js", "")]);
        fs::write(dir.path().join("b.js"), b"// @generated \xff\xfe\nvar a;\n").unwrap();
        let o = scan(dir.path()).unwrap();
        assert_eq!(excluded(&o, Exclusion::Generated), ["b.js"]);
    }

    #[test]
    fn an_ignored_vendor_file_is_not_counted_as_excluded() {
        let dir = tree(&[
            (".gitignore", "vendor/\n"),
            (".arch", "node: app\n"),
            ("vendor/a.js", ""),
            ("b.js", ""),
        ]);
        let o = scan(dir.path()).unwrap();
        assert!(o.excluded.is_empty());
    }
}
