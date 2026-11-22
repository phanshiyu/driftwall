//! The fixed rule behind `node: *`: which node name a file gets from its position alone.

use std::ffi::OsStr;
use std::path::{Component, Path};

/// Name given to the repository root directory when an entry file takes its directory's name.
const ROOT_NAME: &str = "root";
/// Entry files that are named after their directory, not their own stem.
const DIRECTORY_ENTRIES: [&str; 3] = ["mod.rs", "__init__.py", "__main__.py"];
/// File stem of the entry files (`index.ts`, `index.d.ts`, `index.js`) named after their directory.
const INDEX_STEM: &str = "index";

/// The raw node name `file` gets under a `node: *` tag in `dir`, or `None` when `file` is not
/// inside `dir`. Both are repository-relative, and `dir` is empty for the repository root.
///
/// The name is not validated: callers check it with [`NodeId::new`](crate::model::NodeId::new).
///
/// - A file in a subdirectory belongs to the node named after that first subdirectory, which is
///   also the node of a sibling module of the same name (`gitlab.rs` and `gitlab/x.rs`).
/// - A file directly in `dir` is named by its file name up to the first `.`, so `a.test.ts` joins
///   `a`, and `main.rs` and `lib.rs` are `main` and `lib`.
/// - `mod.rs`, `__init__.py`, `__main__.py` and `index.<ext>` take the name of `dir` itself, or
///   `root` for the repository root.
/// - Leading underscores are stripped from the result (`_private.py` is `private`), which can leave
///   it empty.
#[must_use]
pub fn module_name(dir: &Path, file: &Path) -> Option<String> {
    let name = raw_name(dir, file)?;
    Some(name.trim_start_matches('_').to_owned())
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
        module_name(Path::new(dir), Path::new(file))
    }

    #[test]
    fn a_file_is_named_by_its_stem_up_to_the_first_dot() {
        assert_eq!(name("src", "src/gitlab.rs").as_deref(), Some("gitlab"));
        assert_eq!(name("src", "src/a.test.ts").as_deref(), Some("a"));
        assert_eq!(name("src", "src/a.b.c.py").as_deref(), Some("a"));
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
        assert_eq!(name("pkg", "pkg/__x.test.py").as_deref(), Some("x"));
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
