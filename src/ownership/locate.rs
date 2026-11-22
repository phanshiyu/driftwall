//! What a path is, and who owns it: the answer behind `driftwall where`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{
    Discovered, Explanation, OwnershipError, discover, explain, language_of, relative_to_root,
};
use crate::model::{Exclusion, NodeId};

/// What a walked path is to driftwall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    /// A source file in a language driftwall reads.
    Source,
    /// Any other file, such as `data.json`.
    Asset,
    /// A code file in a language driftwall cannot read, with its census key (`go`, `shebang`).
    Unread(String),
    /// A source file the scan leaves out as vendored, minified or generated.
    Excluded(Exclusion),
    /// A source file only `.driftwallignore` hides.
    Ignored,
    /// A directory.
    Directory,
}

/// The answer of [`locate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// What the path is.
    pub kind: FileKind,
    /// The owning node. Always `None` for an excluded or ignored file, which nothing owns.
    pub node: Option<NodeId>,
    /// The `.arch` file that decided it, relative to the root.
    pub arch_file: Option<PathBuf>,
}

/// Says what `file` is (see [`FileKind`]) and who owns it, like [`explain`] for the owner.
///
/// A relative `file` is taken relative to `root`; an absolute one must lie inside it. A file the
/// walk did not see (git ignores it) is judged by its name alone. Only the `.arch` files on the
/// path of `file` are read, so an invalid one elsewhere does not matter. A name that a `node: *`
/// tag cannot turn into a node id leaves an asset or unread file without an owner, as in the scan.
///
/// # Errors
/// Fails with [`OwnershipError::NoSuchFile`] when nothing exists at `file`, and as [`explain`] does
/// otherwise, plus when the tree cannot be walked.
pub fn locate(root: &Path, file: &Path) -> Result<Located, OwnershipError> {
    let relative = relative_to_root(root, file)?;
    let full = root.join(&relative);
    let metadata = match fs::metadata(&full) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(OwnershipError::NoSuchFile { path: relative });
        }
        Err(source) => return Err(OwnershipError::Io { path: full, source }),
    };
    let kind = if metadata.is_dir() {
        FileKind::Directory
    } else {
        kind_of(root, &relative)?
    };
    if matches!(kind, FileKind::Excluded(_) | FileKind::Ignored) {
        return Ok(Located {
            kind,
            node: None,
            arch_file: None,
        });
    }
    let Explanation { node, arch_file } = match explain(root, &relative) {
        // why: an asset or unread file must never fail, so a name that is no node id is unowned.
        Err(OwnershipError::InvalidModule { .. })
            if matches!(kind, FileKind::Asset | FileKind::Unread(_)) =>
        {
            Explanation {
                node: None,
                arch_file: None,
            }
        }
        other => other?,
    };
    Ok(Located {
        kind,
        node,
        arch_file,
    })
}

/// The kind of the existing file `relative`: what the walk says, else what its name says.
fn kind_of(root: &Path, relative: &Path) -> Result<FileKind, OwnershipError> {
    let Discovered {
        sources,
        assets,
        excluded,
        unread,
        ignored,
        ..
    } = discover(root)?;
    let relative = relative.to_path_buf();
    if let Some(reason) = excluded
        .iter()
        .find_map(|(reason, files)| files.contains(&relative).then_some(*reason))
    {
        return Ok(FileKind::Excluded(reason));
    }
    if ignored.contains(&relative) {
        return Ok(FileKind::Ignored);
    }
    if let Some(key) = unread
        .iter()
        .find_map(|(key, files)| files.contains(&relative).then(|| key.clone()))
    {
        return Ok(FileKind::Unread(key));
    }
    if sources.binary_search(&relative).is_ok() {
        return Ok(FileKind::Source);
    }
    if assets.binary_search(&relative).is_ok() {
        return Ok(FileKind::Asset);
    }
    // why: git ignores the file, so the walk never saw it; its name still says what it is.
    Ok(if language_of(&relative).is_some() {
        FileKind::Source
    } else if let Some(key) = super::census::unread_key(root, &relative) {
        FileKind::Unread(key)
    } else {
        FileKind::Asset
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    fn answer(dir: &tempfile::TempDir, file: &str) -> (FileKind, Option<String>) {
        let found = locate(dir.path(), Path::new(file)).unwrap();
        (found.kind, found.node.map(|node| node.to_string()))
    }

    fn repo() -> tempfile::TempDir {
        tree(&[
            (".gitignore", "secret.ts\n"),
            (".driftwallignore", "hidden.ts\n"),
            ("a/.arch", "node: a\n"),
            ("a/main.ts", ""),
            ("a/data.json", "{}"),
            ("a/view.go", ""),
            ("a/vendor/lib.js", ""),
            ("a/hidden.ts", ""),
            ("a/secret.ts", ""),
            ("loose.ts", ""),
            ("loose.json", ""),
            ("loose.go", ""),
        ])
    }

    #[test]
    fn every_kind_of_file_is_named() {
        let dir = repo();
        let named = |file| answer(&dir, file);
        assert_eq!(named("a/main.ts"), (FileKind::Source, Some("a".to_owned())));
        assert_eq!(
            named("a/data.json"),
            (FileKind::Asset, Some("a".to_owned()))
        );
        assert_eq!(
            named("a/view.go"),
            (FileKind::Unread("go".to_owned()), Some("a".to_owned()))
        );
        assert_eq!(
            named("a/vendor/lib.js"),
            (FileKind::Excluded(Exclusion::Vendored), None)
        );
        assert_eq!(named("a/hidden.ts"), (FileKind::Ignored, None));
        assert_eq!(
            named("a/secret.ts"),
            (FileKind::Source, Some("a".to_owned()))
        );
        assert_eq!(
            named("a/vendor"),
            (FileKind::Directory, Some("a".to_owned()))
        );
        assert_eq!(named("loose.ts"), (FileKind::Source, None));
        assert_eq!(named("loose.json"), (FileKind::Asset, None));
        assert_eq!(named("loose.go"), (FileKind::Unread("go".to_owned()), None));
    }

    #[test]
    fn a_path_that_does_not_exist_is_an_error_naming_it() {
        let dir = repo();
        let error = locate(dir.path(), Path::new("a/nope.ts")).unwrap_err();
        assert_eq!(error.to_string(), "no such file: a/nope.ts");
    }

    #[test]
    fn a_path_outside_the_root_is_still_outside_the_root() {
        let dir = repo();
        let error = locate(dir.path(), Path::new("../x.ts")).unwrap_err();
        assert!(error.to_string().contains("not inside"), "{error}");
    }

    #[test]
    fn an_asset_whose_name_is_no_node_id_is_unowned_not_an_error() {
        let dir = tree(&[("ui/.arch", "node: *\n"), ("ui/.eslintrc.json", "{}")]);
        assert_eq!(answer(&dir, "ui/.eslintrc.json"), (FileKind::Asset, None));
    }

    #[test]
    fn an_invalid_arch_elsewhere_does_not_matter() {
        let dir = tree(&[
            ("bad/.arch", "nope\n"),
            ("a/.arch", "node: a\n"),
            ("a/x.ts", ""),
        ]);
        assert_eq!(
            answer(&dir, "a/x.ts"),
            (FileKind::Source, Some("a".to_owned()))
        );
    }
}
