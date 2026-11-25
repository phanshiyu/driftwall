//! Errors raised while scanning a repository for ownership.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::model::{Location, MapSite, ModelError};

/// Why a `.arch` file was rejected.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArchProblem {
    /// The file has no `node:` line.
    #[error("missing `node: <id>` line")]
    MissingNode,
    /// The file has more than one `node:` line. Reported at the second one.
    #[error("more than one `node:` line")]
    DuplicateNode,
    /// The `node:` value is not a valid node id.
    #[error(transparent)]
    InvalidId(#[from] ModelError),
    /// The file has more than one `scope:` line. Reported at the second one.
    #[error("more than one `scope:` line")]
    DuplicateScope,
    /// The `scope:` value is not `here`.
    #[error("invalid scope `{0}`: expected `scope: here`")]
    InvalidScope(String),
    /// A line that is neither blank, a `#` comment, `node: <id>` nor `scope: here`.
    #[error("unrecognised line `{0}`: expected `node: <id>`, `scope: here` or a `#` comment")]
    UnrecognisedLine(String),
}

/// Errors from [`Mechanism::scan`](super::Mechanism::scan) and [`explain`](super::explain).
#[derive(Debug, Error)]
pub enum OwnershipError {
    /// A file could not be read.
    #[error("cannot read {}", path.display())]
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// A `.arch` file does not follow the format.
    #[error("{}:{}: invalid .arch: {problem}", at.file.display(), at.line)]
    InvalidArch {
        /// The `.arch` file and the offending line.
        at: Location,
        /// What is wrong with it.
        problem: ArchProblem,
    },
    /// A `node: *` tag derived a node name that is not a valid node id. Never renamed.
    #[error("{}:{}: `node: *` cannot name {}", at.file.display(), at.line, file.display())]
    InvalidModule {
        /// The `.arch` file and the line of its `node: *`.
        at: Location,
        /// The source file whose derived name is invalid.
        file: PathBuf,
        /// The invalid name.
        source: ModelError,
    },
    /// Two `%% map` globs of different targets map one source file with equal specificity.
    #[error(
        "{c}:{} and {c}:{} both map {} with equal specificity (`{}` and `{}`)",
        first.line, second.line, file.display(), first.glob, second.glob,
        c = contract.display()
    )]
    AmbiguousMap {
        /// The contract path.
        contract: PathBuf,
        /// An example file both globs match.
        file: PathBuf,
        /// The glob on the earlier line.
        first: MapSite,
        /// The glob on the later line.
        second: MapSite,
    },
    /// Walking the directory tree failed, or an ignore file was malformed.
    #[error("cannot walk {}", root.display())]
    Walk {
        /// The root being scanned.
        root: PathBuf,
        /// The underlying error.
        source: ignore::Error,
    },
    /// The path given to `locate` does not exist.
    #[error("no such file: {}", path.display())]
    NoSuchFile {
        /// The path as given, relative to the root.
        path: PathBuf,
    },
    /// The path given to `explain` is not inside the scanned root.
    #[error("{} is not inside {}", path.display(), root.display())]
    OutsideRoot {
        /// The offending path.
        path: PathBuf,
        /// The repository root.
        root: PathBuf,
    },
}
