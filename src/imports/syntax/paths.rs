//! Repository-relative path helpers shared by the resolvers.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Every directory at or above a file in `files`, the repository root (the empty path) included.
pub fn directories<'a>(files: impl IntoIterator<Item = &'a Path>) -> BTreeSet<PathBuf> {
    files
        .into_iter()
        .filter_map(Path::parent)
        .flat_map(Path::ancestors)
        .map(Path::to_path_buf)
        .collect()
}

/// Joins `specifier` onto `dir` and folds `.` and `..` lexically. `None` means the path leaves the
/// repository root.
pub fn join_normalised(dir: Option<&Path>, specifier: &str) -> Option<PathBuf> {
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
