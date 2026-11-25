//! The answer of `driftwall where`.

use std::path::PathBuf;

use crate::code::{Located, PackageUse};

/// Everything `driftwall where` knows about one path: what it is, who owns it, and which packages
/// it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhereAnswer {
    /// The path as the user gave it.
    pub file: PathBuf,
    /// What the path is and who owns it.
    pub located: Located,
    /// The package uses of the file, empty unless it is a source file in a repository with a valid
    /// contract.
    pub packages: Vec<PackageUse>,
}
