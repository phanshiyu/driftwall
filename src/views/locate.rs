//! `driftwall where`: what a file is, which node owns it, and which rule decided it.

use std::fs;
use std::path::{Path, PathBuf};

use crate::code::{FileKind, Located, Owner, PackageUse};
use crate::contract::DEFAULT_CONTRACT;
use crate::lint::{self, LintOptions};
use crate::ownership;
use crate::results::WhereAnswer;

use super::ViewsError;

/// Says what `file` under `root` is and who owns it.
///
/// `contract` is the contract named on the command line, `None` for the default one, which only
/// matters when it holds `%% map` lines.
///
/// # Errors
/// A contract given explicitly that is unreadable or invalid, a path outside the root or that does
/// not exist, or an unreadable or invalid `.arch` file.
pub fn locate(
    root: &Path,
    file: &Path,
    contract: Option<&Path>,
) -> Result<WhereAnswer, ViewsError> {
    let contract_path = contract.unwrap_or_else(|| Path::new(DEFAULT_CONTRACT));
    // why: a given contract must be valid; the default one only matters when it holds `%% map` lines.
    let parsed = if contract.is_some() {
        Some(lint::read_contract(root, contract_path)?)
    } else {
        lint::read_contract(root, contract_path).ok()
    };
    let has_contract = parsed.is_some();
    let rules = parsed.map(|parsed| parsed.maps).unwrap_or_default();
    let inside = inside_root(root, file);
    let mut located = ownership::Mechanism::of(contract_path, &rules).locate(root, &inside)?;
    // why: only the caller knows the contract path, so it names the file; the contract is no asset.
    if located.kind != FileKind::Directory && inside.components().eq(contract_path.components()) {
        located = Located {
            kind: FileKind::Contract,
            owner: Owner::Unowned,
        };
    }
    let packages = package_uses(root, &inside, contract_path, has_contract, &located)?;
    Ok(WhereAnswer {
        file: file.to_path_buf(),
        located,
        packages,
    })
}

/// The package uses of `file`, a source file, which only a scan of the whole repository can tell:
/// none for any other kind of file and when there is no valid contract to scan against.
fn package_uses(
    root: &Path,
    file: &Path,
    contract: &Path,
    has_contract: bool,
    located: &Located,
) -> Result<Vec<PackageUse>, ViewsError> {
    if !has_contract || located.kind != FileKind::Source {
        return Ok(Vec::new());
    }
    let options = LintOptions {
        contract: contract.to_path_buf(),
    };
    let observation = lint::observe(root, &options)?;
    Ok(observation
        .package_uses
        .get(file)
        .cloned()
        .unwrap_or_default())
}

/// Turns an absolute `file` below `root` into the root-relative path `locate` accepts.
///
/// Both are compared as given first, then with symlinks resolved (a temp dir like `/var/..` may
/// really be `/private/var/..`). Anything that still does not fit is returned unchanged so
/// `locate` reports it as outside the root.
fn inside_root(root: &Path, file: &Path) -> PathBuf {
    if !file.is_absolute() {
        return file.to_path_buf();
    }
    if let Ok(relative) = file.strip_prefix(root) {
        return relative.to_path_buf();
    }
    let resolved = fs::canonicalize(root).ok().zip(resolve(file));
    match resolved {
        Some((real_root, real_file)) => real_file
            .strip_prefix(&real_root)
            .map_or_else(|_| file.to_path_buf(), Path::to_path_buf),
        None => file.to_path_buf(),
    }
}

/// Resolves symlinks in `file`, or in its directory when the file itself does not exist.
fn resolve(file: &Path) -> Option<PathBuf> {
    fs::canonicalize(file).ok().or_else(|| {
        let parent = fs::canonicalize(file.parent()?).ok()?;
        Some(parent.join(file.file_name()?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_files_pass_through() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("a/b.ts")),
            Path::new("a/b.ts")
        );
    }

    #[test]
    fn absolute_files_below_the_root_become_relative() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("/r/a/b.ts")),
            Path::new("a/b.ts")
        );
    }

    #[test]
    fn absolute_files_elsewhere_are_left_for_locate_to_reject() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("/other/b.ts")),
            Path::new("/other/b.ts")
        );
    }
}
