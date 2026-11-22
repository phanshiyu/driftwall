//! Walks the repository and maps every file to its owning node via `.arch` files.

mod arch;
mod error;
mod language;

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use ignore::WalkBuilder;

pub use error::{ArchProblem, OwnershipError};
pub use language::language_of;

use crate::model::{Location, NodeId, Ownership};

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

/// Scans `root` and maps every source file to the node named by its nearest `.arch` file.
///
/// Honors `.gitignore` (with or without a `.git` directory) and `.driftwallignore`. Machine
/// specific ignore sources (global git config, `.git/info/exclude`, parent directories) are
/// deliberately not consulted so the result depends only on the contents of `root`.
///
/// # Errors
/// Fails when the tree cannot be walked, a file cannot be read, or a `.arch` file is invalid.
pub fn scan(root: &Path) -> Result<Ownership, OwnershipError> {
    let (sources, arch_files) = discover(root)?;

    let mut decisions: BTreeMap<PathBuf, NodeId> = BTreeMap::new();
    let mut tags = Vec::new();
    for arch_file in arch_files {
        let tag = read_tag(root, &arch_file)?;
        tags.push((
            Location {
                file: arch_file.clone(),
                line: tag.line,
            },
            tag.raw,
        ));
        let dir = arch_file
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        decisions.insert(dir, tag.node);
    }
    tags.sort();

    let mut ownership = Ownership {
        tags,
        ..Ownership::default()
    };
    for source in sources {
        let owner = source
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .find_map(|dir| decisions.get(dir));
        match owner {
            Some(node) => {
                ownership.owners.insert(source, node.clone());
            }
            None => {
                ownership.unmapped.insert(source);
            }
        }
    }
    Ok(ownership)
}

/// Explains which node owns `file` and which `.arch` file decided it.
///
/// A relative `file` is taken relative to `root`; an absolute one must lie inside it. The path
/// need not exist or have a source extension. Unlike [`scan`], this reads only the `.arch`
/// files on the path from `file` up to `root`, so ignore files are not consulted and unrelated
/// invalid `.arch` files elsewhere do not matter.
///
/// # Errors
/// Fails when `file` is outside `root`, or an `.arch` on its path is unreadable or invalid.
pub fn explain(root: &Path, file: &Path) -> Result<Explanation, OwnershipError> {
    let relative = relative_to_root(root, file)?;
    for dir in relative.parent().into_iter().flat_map(Path::ancestors) {
        let arch_file = dir.join(ARCH_FILE);
        match fs::read_to_string(root.join(&arch_file)) {
            Ok(contents) => {
                let tag = arch::parse(&arch_file, &contents)?;
                return Ok(Explanation {
                    node: Some(tag.node),
                    arch_file: Some(arch_file),
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

/// Returns root-relative (source files, `.arch` files), each sorted.
fn discover(root: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), OwnershipError> {
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

    let mut sources = Vec::new();
    let mut arch_files = Vec::new();
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
            arch_files.push(relative);
        } else if language_of(&relative).is_some() {
            sources.push(relative);
        }
    }
    sources.sort();
    arch_files.sort();
    Ok((sources, arch_files))
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
