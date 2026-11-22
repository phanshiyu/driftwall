//! The fixed rule behind `node: *`: which node name a file gets from its position, and from the
//! modules next to it when it is a test file.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Component, Path};

/// The dot-separated name parts that make `a.<part>.ts` a test file.
const TEST_PARTS: [&str; 2] = ["test", "spec"];
/// Name given to the repository root directory when an entry file takes its directory's name.
const ROOT_NAME: &str = "root";
/// Entry files that are named after their directory, not their own stem.
const DIRECTORY_ENTRIES: [&str; 3] = ["mod.rs", "__init__.py", "__main__.py"];
/// File stem of the entry files (`index.ts`, `index.d.ts`, `index.js`) named after their directory.
const INDEX_STEM: &str = "index";

/// The raw node name `file` gets under a `node: *` tag in `dir`, or `None` when `file` is not
/// inside `dir`. Both are repository-relative, and `dir` is empty for the repository root.
///
/// `peers` are the non-test modules of `dir` (see [`peers_in`]). The name is not validated: callers check it with [`NodeId::new`](crate::model::NodeId::new).
///
/// - A file in a subdirectory belongs to the node named after that first subdirectory, which is
///   also the node of a sibling module of the same name (`gitlab.rs` and `gitlab/x.rs`).
/// - A file directly in `dir` is named by its file name up to the first `.`, so `a.test.ts` joins
///   `a`, and `main.rs` and `lib.rs` are `main` and `lib`.
/// - `mod.rs`, `__init__.py`, `__main__.py` and `index.<ext>` take the name of `dir` itself, or
///   `root` for the repository root.
/// - A test file directly in `dir` (see [`is_test_file`]) keeps its name only when `peers` has it,
///   and otherwise takes the name of `dir` itself, like an entry file.
/// - Leading underscores are stripped from the result (`_private.py` is `private`), which can leave
///   it empty.
#[must_use]
pub fn module_name(dir: &Path, file: &Path, peers: &BTreeSet<String>) -> Option<String> {
    let name = plain_name(dir, file)?;
    if file.parent() == Some(dir) && is_test_file(file) && !peers.contains(&name) {
        return Some(strip(&directory_name(dir)));
    }
    Some(name)
}

/// The names of the non-test modules directly in `dir`, among `files`.
///
/// That is the name of each file directly in `dir` that is not a test file, and of each
/// subdirectory that holds one of `files`. Files outside `dir` are ignored. These are the names a
/// test file may keep (see [`module_name`]).
#[must_use]
pub fn peers_in<'a>(dir: &Path, files: impl IntoIterator<Item = &'a Path>) -> BTreeSet<String> {
    files
        .into_iter()
        .filter(|file| !(file.parent() == Some(dir) && is_test_file(file)))
        .filter_map(|file| plain_name(dir, file))
        .collect()
}

/// Whether `file` is a test file by its name.
///
/// It is when it has a part `test` or `spec` between two dots after a non-empty first part
/// (`a.test.ts`, `a.b.spec.d.ts`), or is `test_<x>.py`, `<x>_test.py`, `<x>_test.rs` or
/// `<x>_tests.rs`, with `<x>` not empty.
#[must_use]
pub fn is_test_file(file: &Path) -> bool {
    let Some(name) = file.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let parts: Vec<&str> = name.split('.').collect();
    let dotted = parts.first().is_some_and(|first| !first.is_empty())
        && parts
            .get(1..parts.len().saturating_sub(1))
            .unwrap_or_default()
            .iter()
            .any(|part| TEST_PARTS.contains(part));
    let python = name.strip_suffix(".py").is_some_and(|stem| {
        stem.strip_prefix("test_")
            .is_some_and(|rest| !rest.is_empty())
            || ends_with_text(stem, &["_test"])
    });
    let rust = name
        .strip_suffix(".rs")
        .is_some_and(|stem| ends_with_text(stem, &["_test", "_tests"]));
    dotted || python || rust
}

/// Whether `stem` is some text followed by one of `suffixes`.
fn ends_with_text(stem: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suffix| {
        stem.strip_suffix(suffix)
            .is_some_and(|rest| !rest.is_empty())
    })
}

/// The name before the test file rule: by position, leading underscores stripped.
fn plain_name(dir: &Path, file: &Path) -> Option<String> {
    raw_name(dir, file).map(|name| strip(&name))
}

fn strip(name: &str) -> String {
    name.trim_start_matches('_').to_owned()
}

/// The name before the leading underscores are stripped.
fn raw_name(dir: &Path, file: &Path) -> Option<String> {
    let below = file.strip_prefix(dir).ok()?;
    let mut parts = below.components().filter_map(|part| match part {
        Component::Normal(name) => Some(name),
        Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
            None
        }
    });
    let first = parts.next()?;
    if parts.next().is_some() {
        return Some(first.to_string_lossy().into_owned());
    }
    let name = first.to_string_lossy();
    let stem = name.split('.').next().unwrap_or_default();
    if DIRECTORY_ENTRIES.iter().any(|entry| *entry == name) || stem == INDEX_STEM {
        return Some(directory_name(dir));
    }
    Some(stem.to_owned())
}

fn directory_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(OsStr::to_str)
        .map_or_else(|| ROOT_NAME.to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(dir: &str, file: &str) -> Option<String> {
        name_beside(dir, file, &[])
    }

    /// The name of `file` when the files `others` are in the repository too.
    fn name_beside(dir: &str, file: &str, others: &[&str]) -> Option<String> {
        let peers = peers_in(Path::new(dir), others.iter().map(Path::new));
        module_name(Path::new(dir), Path::new(file), &peers)
    }

    #[test]
    fn a_file_is_named_by_its_stem_up_to_the_first_dot() {
        assert_eq!(name("src", "src/gitlab.rs").as_deref(), Some("gitlab"));
        assert_eq!(name("src", "src/a.b.c.py").as_deref(), Some("a"));
    }

    #[test]
    fn a_test_file_joins_the_module_it_tests() {
        for (file, other) in [
            ("src/a.test.ts", "src/a.ts"),
            ("src/a.spec.tsx", "src/a.tsx"),
            ("src/a.b.test.d.ts", "src/a.js"),
            ("src/a.test.ts", "src/a/x.ts"),
            ("src/__a.test.ts", "src/a.ts"),
        ] {
            assert_eq!(
                name_beside("src", file, &[other]).as_deref(),
                Some("a"),
                "{file}"
            );
        }
    }

    #[test]
    fn a_test_file_without_a_module_joins_the_node_of_its_directory() {
        for file in [
            "src/rows.test.ts",
            "src/rows.spec.js",
            "src/test_rows.py",
            "src/rows_test.py",
            "src/rows_test.rs",
            "src/rows_tests.rs",
        ] {
            assert_eq!(name("src", file).as_deref(), Some("src"), "{file}");
            assert_eq!(
                name("a/_lib", &file.replace("src", "a/_lib")).as_deref(),
                Some("lib")
            );
            assert_eq!(name("", &file.replace("src/", "")).as_deref(), Some("root"));
        }
    }

    #[test]
    fn a_test_file_does_not_match_another_test_file() {
        let others = ["src/rows.spec.ts", "src/rows.test.js"];
        assert_eq!(
            name_beside("src", "src/rows.test.ts", &others).as_deref(),
            Some("src")
        );
    }

    #[test]
    fn a_python_test_file_is_named_by_its_whole_stem_so_it_joins_the_directory() {
        assert_eq!(
            name_beside("src", "src/test_rows.py", &["src/rows.py"]).as_deref(),
            Some("src")
        );
    }

    #[test]
    fn only_the_files_that_look_like_tests_are_tests() {
        for file in [
            "a.test.ts",
            "a.b.spec.d.ts",
            "snap.test.json",
            "test_a.py",
            "a_test.py",
            "a_test.rs",
            "a_tests.rs",
            "x/y/a.spec.js",
        ] {
            assert!(is_test_file(Path::new(file)), "{file}");
        }
        for file in [
            "test.ts",
            "a.test",
            ".test.ts",
            "a.tests.ts",
            "test_.py",
            "_test.py",
            "a_test.ts",
            "test_a.rs",
            "a_test.py.bak",
            "a_tests.py",
            "contest.ts",
            "latest.py",
        ] {
            assert!(!is_test_file(Path::new(file)), "{file}");
        }
    }

    #[test]
    fn a_test_file_below_the_directory_is_named_by_the_subdirectory() {
        assert_eq!(name("src", "src/rows/a.test.ts").as_deref(), Some("rows"));
    }

    #[test]
    fn the_peers_are_the_non_test_modules_directly_in_the_directory() {
        let files = [
            "src/a.ts",
            "src/a.test.ts",
            "src/b.test.ts",
            "src/c/x.test.ts",
            "src/index.ts",
            "other/z.ts",
        ];
        let peers = peers_in(Path::new("src"), files.iter().map(Path::new));
        assert_eq!(
            peers.iter().map(String::as_str).collect::<Vec<_>>(),
            ["a", "c", "src"]
        );
    }

    #[test]
    fn main_and_lib_are_modules_named_by_their_stem() {
        assert_eq!(name("src", "src/main.rs").as_deref(), Some("main"));
        assert_eq!(name("src", "src/lib.rs").as_deref(), Some("lib"));
    }

    #[test]
    fn directory_entries_take_the_directory_name() {
        for file in [
            "pkg/mod.rs",
            "pkg/__init__.py",
            "pkg/__main__.py",
            "pkg/index.ts",
            "pkg/index.d.ts",
            "pkg/index.test.js",
        ] {
            assert_eq!(name("pkg", file).as_deref(), Some("pkg"), "{file}");
        }
        assert_eq!(name("a/b", "a/b/mod.rs").as_deref(), Some("b"));
    }

    #[test]
    fn entries_in_the_repository_root_are_named_root() {
        assert_eq!(name("", "index.ts").as_deref(), Some("root"));
        assert_eq!(name("", "__init__.py").as_deref(), Some("root"));
    }

    #[test]
    fn near_misses_are_ordinary_modules() {
        assert_eq!(name("pkg", "pkg/indexer.ts").as_deref(), Some("indexer"));
        assert_eq!(name("pkg", "pkg/mod.py").as_deref(), Some("mod"));
        assert_eq!(name("pkg", "pkg/init.py").as_deref(), Some("init"));
    }

    #[test]
    fn a_subdirectory_names_the_node_whatever_it_holds() {
        assert_eq!(name("src", "src/routes/a.rs").as_deref(), Some("routes"));
        assert_eq!(
            name("src", "src/routes/deep/b.rs").as_deref(),
            Some("routes")
        );
        assert_eq!(name("src", "src/routes/mod.rs").as_deref(), Some("routes"));
        assert_eq!(name("", "ui/index.ts").as_deref(), Some("ui"));
    }

    #[test]
    fn a_sibling_module_and_its_directory_share_a_node() {
        assert_eq!(name("src", "src/gitlab.rs"), name("src", "src/gitlab/x.rs"));
    }

    #[test]
    fn leading_underscores_are_stripped_from_files_and_directories() {
        assert_eq!(name("pkg", "pkg/_private.py").as_deref(), Some("private"));
        assert_eq!(
            name_beside("pkg", "pkg/__x.test.py", &["pkg/x.py"]).as_deref(),
            Some("x")
        );
        assert_eq!(name("pkg", "pkg/_a_b.py").as_deref(), Some("a_b"));
        assert_eq!(name("pkg", "pkg/a_.py").as_deref(), Some("a_"));
        assert_eq!(name("pkg", "pkg/_x/y.py").as_deref(), Some("x"));
        assert_eq!(
            name("pkg", "pkg/__tests__/y.ts").as_deref(),
            Some("tests__")
        );
        assert_eq!(name("pkg", "pkg/_x.py"), name("pkg", "pkg/x.py"));
    }

    #[test]
    fn a_directory_entry_is_named_after_its_directory_without_underscores() {
        assert_eq!(
            name("a/_internal", "a/_internal/__init__.py").as_deref(),
            Some("internal")
        );
        assert_eq!(
            name("a/_internal", "a/_internal/__main__.py").as_deref(),
            Some("internal")
        );
    }

    #[test]
    fn a_name_of_only_underscores_is_empty() {
        for file in ["pkg/_.py", "pkg/__.py", "pkg/___/a.py"] {
            assert_eq!(name("pkg", file).as_deref(), Some(""), "{file}");
        }
    }

    #[test]
    fn hidden_files_have_an_empty_name() {
        assert_eq!(name("", ".eslintrc.js").as_deref(), Some(""));
    }

    #[test]
    fn a_file_outside_the_directory_has_no_name() {
        assert_eq!(name("src", "other/a.rs"), None);
        assert_eq!(name("src", "src"), None);
    }
}
