//! Walks the repository and maps every file to its owning node via `.arch` files.

mod arch;
mod error;
mod exclude;
mod language;
mod module;

#[cfg(test)]
mod asset_tests;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub use error::{ArchProblem, OwnershipError};
pub use exclude::exclusion_of;
pub use language::language_of;
pub use module::module_name;

pub use crate::model::Tag;
use crate::model::{Exclusion, Location, NodeId, Ownership};

/// Name of the file that tags a directory with a node.
const ARCH_FILE: &str = ".arch";
/// Extra gitignore-syntax exclusions, honored in addition to `.gitignore`.
const IGNORE_FILE: &str = ".driftwallignore";

/// The answer to "who owns this path, and why".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// The owning node, or `None` when no `.arch` applies.
    pub node: Option<NodeId>,
    /// The `.arch` file that decided it, relative to the root, or `None` when unmapped.
    pub arch_file: Option<PathBuf>,
}

/// One `.arch` file's decision about its directory.
struct Decision {
    tag: Tag,
    arch_file: PathBuf,
    line: u32,
}

impl Decision {
    /// The node `file` gets from this decision, where `dir` is the directory of the `.arch` file.
    fn node_for(&self, dir: &Path, file: &Path) -> Result<NodeId, OwnershipError> {
        match &self.tag {
            Tag::Node(id) => Ok(id.clone()),
            Tag::Modules => {
                let name = module_name(dir, file).unwrap_or_default();
                NodeId::new(&name).map_err(|source| OwnershipError::InvalidModule {
                    at: self.location(),
                    file: file.to_path_buf(),
                    source,
                })
            }
        }
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
/// Vendored and minified files (see [`exclusion_of`]) are not source: they appear in
/// [`Ownership::excluded`] and nowhere else, unless the root `.driftwallignore` whitelists them
/// with a negated pattern.
///
/// Every other walked file is an asset, owned by the same tag rules and listed in
/// [`Ownership::asset_owners`] or [`Ownership::asset_unmapped`]. Assets never appear in
/// [`Ownership::owners`], [`Ownership::unmapped`], [`Ownership::tags`] or [`Ownership::excluded`].
///
/// Honors `.gitignore` (with or without a `.git` directory) and `.driftwallignore`. Machine
/// specific ignore sources (global git config, `.git/info/exclude`, parent directories) are
/// deliberately not consulted so the result depends only on the contents of `root`.
///
/// # Errors
/// Fails when the tree cannot be walked, a file cannot be read, a `.arch` file is invalid, or a
/// `node: *` derives a name that is not a valid node id.
pub fn scan(root: &Path) -> Result<Ownership, OwnershipError> {
    let Discovered {
        sources,
        assets,
        arch_files,
        excluded,
    } = discover(root)?;

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
        decisions.insert(
            dir,
            Decision {
                tag: parsed.tag,
                arch_file,
                line: parsed.line,
            },
        );
    }

    let mut ownership = Ownership {
        excluded,
        ..Ownership::default()
    };
    let mut derived: BTreeSet<(Location, String)> = BTreeSet::new();
    for source in sources {
        match nearest_decision(&decisions, &source) {
            Some((dir, decision)) => {
                let node = decision.node_for(dir, &source)?;
                if decision.tag == Tag::Modules {
                    derived.insert((decision.location(), node.to_string()));
                }
                ownership.owners.insert(source, node);
            }
            None => {
                ownership.unmapped.insert(source);
            }
        }
    }
    for asset in assets {
        // why: an asset must never fail the scan, so a name that is no node id leaves it unmapped.
        let owner = nearest_decision(&decisions, &asset)
            .and_then(|(dir, decision)| decision.node_for(dir, &asset).ok());
        match owner {
            Some(node) => {
                ownership.asset_owners.insert(asset, node);
            }
            None => {
                ownership.asset_unmapped.insert(asset);
            }
        }
    }
    tags.extend(derived);
    tags.sort();
    ownership.tags = tags;
    ownership.tag_dirs = decisions
        .into_iter()
        .map(|(dir, decision)| (dir, decision.tag))
        .collect();
    Ok(ownership)
}

/// The decision of the nearest `.arch` at or above `file`, with the directory that holds it.
fn nearest_decision<'a>(
    decisions: &'a BTreeMap<PathBuf, Decision>,
    file: &Path,
) -> Option<(&'a Path, &'a Decision)> {
    file.parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .find_map(|dir| decisions.get_key_value(dir))
        .map(|(dir, decision)| (dir.as_path(), decision))
}

/// Explains which node owns `file` and which `.arch` file decided it.
///
/// A relative `file` is taken relative to `root`; an absolute one must lie inside it. The path
/// need not exist or have a source extension. Unlike [`scan`], this reads only the `.arch`
/// files on the path from `file` up to `root`, so ignore files are not consulted and unrelated
/// invalid `.arch` files elsewhere do not matter. Under `node: *` the node is derived from
/// `file`'s position and the `.arch` file reported is the `node: *` one.
///
/// # Errors
/// Fails when `file` is outside `root`, an `.arch` on its path is unreadable or invalid, or a
/// `node: *` derives a name that is not a valid node id.
pub fn explain(root: &Path, file: &Path) -> Result<Explanation, OwnershipError> {
    let relative = relative_to_root(root, file)?;
    for dir in relative.parent().into_iter().flat_map(Path::ancestors) {
        let arch_file = dir.join(ARCH_FILE);
        match fs::read_to_string(root.join(&arch_file)) {
            Ok(contents) => {
                let parsed = arch::parse(&arch_file, &contents)?;
                let decision = Decision {
                    tag: parsed.tag,
                    arch_file,
                    line: parsed.line,
                };
                return Ok(Explanation {
                    node: Some(decision.node_for(dir, &relative)?),
                    arch_file: Some(decision.arch_file),
                });
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
    Ok(Explanation {
        node: None,
        arch_file: None,
    })
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
}

/// The patterns of the root `.driftwallignore` that whitelist (`!pattern`) files.
fn opt_ins(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    // why: a missing or partly malformed file only means fewer opt-ins, like the walker's view.
    let _ = builder.add(root.join(IGNORE_FILE));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn discover(root: &Path) -> Result<Discovered, OwnershipError> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .add_custom_ignore_filename(IGNORE_FILE)
        .filter_entry(|entry| entry.file_name() != OsStr::new(".git"));
    let opted_in = opt_ins(root);

    let mut found = Discovered {
        sources: Vec::new(),
        assets: Vec::new(),
        arch_files: Vec::new(),
        excluded: BTreeMap::new(),
    };
    for entry in builder.build() {
        let entry = entry.map_err(|source| OwnershipError::Walk {
            root: root.to_path_buf(),
            source,
        })?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let relative: PathBuf = relative.components().collect();
        if relative.file_name() == Some(OsStr::new(ARCH_FILE)) {
            found.arch_files.push(relative);
        } else if language_of(&relative).is_some() {
            let reason = exclusion_of(&relative).filter(|_| {
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
            found.assets.push(relative);
        }
    }
    found.sources.sort();
    found.assets.sort();
    found.arch_files.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Exclusion;
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
                ArchProblem::InvalidId(crate::model::ModelError::InvalidNodeId("1bad".into())),
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
        assert_eq!(inner.node.as_ref().map(NodeId::as_str), Some("inner"));
        assert_eq!(inner.arch_file.as_deref(), Some(Path::new("a/inner/.arch")));

        let inherited = explain_of("a/other/z.ts");
        assert_eq!(inherited.node.as_ref().map(NodeId::as_str), Some("outer"));
        assert_eq!(inherited.arch_file.as_deref(), Some(Path::new("a/.arch")));

        let top = explain_of("top.py");
        assert_eq!(top.node.as_ref().map(NodeId::as_str), Some("root"));
        assert_eq!(top.arch_file.as_deref(), Some(Path::new(".arch")));
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
            assert_eq!(explanation.node.as_ref(), Some(node));
        }
        for path in &scanned.unmapped {
            let explanation = explain(dir.path(), path).unwrap();
            assert_eq!(explanation.node, None);
            assert_eq!(explanation.arch_file, None);
        }
    }

    #[test]
    fn explain_unmapped_returns_none() {
        let dir = tree(&[("x/y.ts", "")]);
        let explanation = explain(dir.path(), Path::new("x/y.ts")).unwrap();
        assert_eq!(
            explanation,
            Explanation {
                node: None,
                arch_file: None
            }
        );
    }

    #[test]
    fn explain_accepts_absolute_and_dotted_paths() {
        let dir = tree(&[("a/.arch", "node: alpha\n"), ("a/x.ts", "")]);
        let absolute = explain(dir.path(), &dir.path().join("a/x.ts")).unwrap();
        let dotted = explain(dir.path(), Path::new("./a/x.ts")).unwrap();
        assert_eq!(absolute, dotted);
        assert_eq!(absolute.arch_file.as_deref(), Some(Path::new("a/.arch")));
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
        files.extend(paths.iter().map(|path| (*path, "")));
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
        for bad in ["_private.py", ".eslintrc.js", "9lives.ts", "a b.ts"] {
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
            ("src/routes/a.rs", ""),
            ("src/routes/inner/.arch", "node: inner\n"),
            ("src/routes/inner/b.rs", ""),
        ]);
        let explain_of = |path: &str| explain(dir.path(), Path::new(path)).unwrap();
        let expect = |path: &str, node: &str, arch: &str| {
            let explanation = explain_of(path);
            assert_eq!(
                explanation.node.as_ref().map(NodeId::as_str),
                Some(node),
                "{path}"
            );
            assert_eq!(
                explanation.arch_file.as_deref(),
                Some(Path::new(arch)),
                "{path}"
            );
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
            assert_eq!(explanation.node.as_ref(), Some(node), "{}", path.display());
        }
    }

    #[test]
    fn explain_reports_an_invalid_derived_id() {
        let dir = tree(&[("src/.arch", "node: *\n")]);
        match explain(dir.path(), Path::new("src/_x.py")) {
            Err(OwnershipError::InvalidModule { at, .. }) => {
                assert_eq!(at.file, Path::new("src/.arch"));
                assert_eq!(at.line, 1);
            }
            other => panic!("expected InvalidModule, got {other:?}"),
        }
    }

    #[test]
    fn driftwall_repository_maps_every_source_file() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let o = scan(root).unwrap();
        let in_src = |p: &&PathBuf| p.starts_with("src");

        let stray: Vec<_> = o.unmapped.iter().filter(in_src).collect();
        assert!(stray.is_empty(), "unmapped files under src/: {stray:?}");

        let modules = [
            "model",
            "ownership",
            "diagram",
            "imports",
            "lint",
            "init",
            "report",
            "cli",
        ];
        let mut seen = std::collections::BTreeSet::new();
        for (path, node) in o.owners.iter().filter(|(p, _)| p.starts_with("src")) {
            let mut parts = path.components().skip(1);
            let expected = match (parts.next(), parts.next()) {
                (Some(Component::Normal(dir)), Some(_)) => dir.to_string_lossy().into_owned(),
                _ => "cli".to_owned(),
            };
            assert_eq!(node.as_str(), expected, "{}", path.display());
            seen.insert(expected);
        }
        for module in modules {
            assert!(seen.contains(module), "no files found for {module}");
        }
        assert_eq!(
            o.owners.get(Path::new("src/lib.rs")).map(NodeId::as_str),
            Some("cli")
        );
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
