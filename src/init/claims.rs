//! Rule 2 of `init`: what the code units decide, before the automatic cut sees the rest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::layout::{Draft, Owner};
use super::units::{CRATE_SOURCE, Unit, UnitKind};

/// Every kind of unit, each decided on its own.
const KINDS: [UnitKind; 3] = [UnitKind::Crate, UnitKind::Package, UnitKind::PythonPackage];

/// Lets the units of the repository decide the files in `fresh` they cover, and returns the files
/// nobody decided yet.
///
/// Two or more units of a kind become one directory node each, covering the whole directory (a
/// crate that contains another crate covers only its `src/`). Exactly one Rust crate is
/// decomposed: `node: *` in its `src/` when that directly holds two or more modules. Exactly one
/// unit of another kind decides nothing. Units without a file in `fresh` do not count.
pub(super) fn claim<'a>(draft: &mut Draft, fresh: &[&'a Path], units: &[Unit]) -> Vec<&'a Path> {
    let mut regions: BTreeSet<PathBuf> = BTreeSet::new();
    let mut taken: BTreeSet<&Path> = BTreeSet::new();
    for kind in KINDS {
        let dirs: BTreeSet<&Path> = units
            .iter()
            .filter(|unit| unit.kind == kind)
            .map(|unit| unit.dir.as_path())
            .filter(|dir| fresh.iter().any(|file| file.starts_with(dir)))
            .collect();
        if dirs.len() >= 2 {
            regions.extend(dirs.iter().map(|dir| region(kind, dir, &dirs)));
        } else if let (UnitKind::Crate, Some(dir)) = (kind, dirs.first()) {
            let source = dir.join(CRATE_SOURCE);
            let inside: Vec<&Path> = fresh
                .iter()
                .copied()
                .filter(|file| file.starts_with(&source))
                .collect();
            if draft.try_modules(&source, &inside) {
                taken.extend(inside);
            }
        }
    }
    let open: Vec<&Path> = fresh
        .iter()
        .copied()
        .filter(|file| !taken.contains(file))
        .collect();
    for file in open {
        let nearest = regions
            .iter()
            .filter(|region| file.starts_with(region))
            .max_by_key(|region| region.components().count());
        if let Some(region) = nearest {
            draft.directories.insert(region.clone());
            draft
                .owners
                .insert(file.to_path_buf(), Owner::Directory(region.clone()));
            taken.insert(file);
        }
    }
    fresh
        .iter()
        .copied()
        .filter(|file| !taken.contains(file))
        .collect()
}

/// The directory a unit's node covers: the unit directory, or for a crate that contains another
/// crate (a root package next to its members) just its `src/`.
fn region(kind: UnitKind, dir: &Path, siblings: &BTreeSet<&Path>) -> PathBuf {
    let contains_another = siblings
        .iter()
        .any(|other| *other != dir && other.starts_with(dir));
    if kind == UnitKind::Crate && contains_another {
        dir.join(CRATE_SOURCE)
    } else {
        dir.to_path_buf()
    }
}
