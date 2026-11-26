//! What a path is, and who owns it: the answer behind `driftwall where`.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;

use super::mapping::explain_mapped;
use super::{
    ARCH_FILE, Discovered, MapContext, OwnershipError, discover, explain, is_repository,
    language_of, relative_to_root,
};
use crate::code::{FileKind, Located, Owner};

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
    locate_with(root, file, None)
}

/// Like [`locate`], with the owner decided by the `%% map` lines of `context` and no `.arch` file
/// read. A directory is answered as a file directly in it.
///
/// # Errors
/// As [`locate`], plus the errors of [`scan_mapped`](super::scan_mapped).
pub fn locate_mapped(
    root: &Path,
    file: &Path,
    context: MapContext<'_>,
) -> Result<Located, OwnershipError> {
    locate_with(root, file, Some(context))
}

fn locate_with(
    root: &Path,
    file: &Path,
    context: Option<MapContext<'_>>,
) -> Result<Located, OwnershipError> {
    let relative = relative_to_root(root, file)?;
    let full = root.join(&relative);
    let metadata = match fs::metadata(&full) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(OwnershipError::NoSuchFile { path: relative });
        }
        Err(source) => return Err(OwnershipError::Io { path: full, source }),
    };
    // why: the walk never enters a nested repository, so nothing there is read or owned.
    if relative
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .any(|ancestor| {
            let full = root.join(ancestor);
            (ancestor != relative || metadata.is_dir()) && is_repository(&full)
        })
    {
        return Ok(Located {
            kind: FileKind::NestedRepository,
            owner: Owner::Unowned,
        });
    }
    let kind = if metadata.is_dir() {
        FileKind::Directory
    } else if relative.file_name() == Some(OsStr::new(ARCH_FILE)) {
        FileKind::TagFile
    } else {
        kind_of(root, &relative)?
    };
    if kind == FileKind::TagFile {
        return Ok(Located {
            kind,
            owner: Owner::Unowned,
        });
    }
    let explained = context.map_or_else(
        || explain(root, &relative),
        |context| explain_mapped(root, &relative, kind == FileKind::Directory, context),
    );
    let owner = match explained {
        // why: an asset or unread file must never fail, so a name that is no node id is unowned.
        Err(OwnershipError::InvalidModule { .. })
            if matches!(
                kind,
                FileKind::Asset | FileKind::Unread(_) | FileKind::Excluded(_) | FileKind::Ignored
            ) =>
        {
            Owner::Unowned
        }
        other => other?,
    };
    Ok(Located {
        kind: if owner.is_folded() && kind == FileKind::Source {
            FileKind::Folded
        } else {
            kind
        },
        owner,
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
    use crate::code::Exclusion;

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
        (found.kind, found.owner.node().map(ToString::to_string))
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
            (
                FileKind::Excluded(Exclusion::Vendored),
                Some("a".to_owned())
            )
        );
        assert_eq!(
            named("a/hidden.ts"),
            (FileKind::Ignored, Some("a".to_owned()))
        );
        assert_eq!(named("a/.arch"), (FileKind::TagFile, None));
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
    fn a_path_inside_a_nested_repository_is_not_read_and_owned_by_nobody() {
        let dir = tree(&[
            ("a/.arch", "node: a\n"),
            ("a/vendored/.git", "gitdir: ../../.git/modules/v\n"),
            ("a/vendored/x.ts", ""),
            ("a/vendored/deep/y.ts", ""),
            ("a/clone/.git/HEAD", "ref: refs/heads/main\n"),
            ("a/clone/z.ts", ""),
        ]);
        for file in [
            "a/vendored/x.ts",
            "a/vendored/deep/y.ts",
            "a/vendored/deep",
            "a/vendored",
            "a/clone/z.ts",
        ] {
            assert_eq!(
                answer(&dir, file),
                (FileKind::NestedRepository, None),
                "{file}"
            );
        }
    }

    #[test]
    fn an_empty_git_entry_hides_nothing_from_where() {
        let dir = tree(&[
            ("a/.arch", "node: a\n"),
            ("a/sub/.git", ""),
            ("a/sub/x.ts", ""),
            ("a/hollow/.git/config", ""),
            ("a/hollow/y.ts", ""),
        ]);
        let source = (FileKind::Source, Some("a".to_owned()));
        assert_eq!(answer(&dir, "a/sub/x.ts"), source);
        assert_eq!(answer(&dir, "a/hollow/y.ts"), source);
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
