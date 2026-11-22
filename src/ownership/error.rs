//! Errors raised while scanning a repository for ownership.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::model::{Location, ModelError};

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
    /// A line that is neither blank, a `#` comment, nor `node: <id>`.
    #[error("unrecognised line `{0}`: expected `node: <id>` or a `#` comment")]
    UnrecognisedLine(String),
}

/// Errors from [`scan`](super::scan) and [`explain`](super::explain).
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
    /// Walking the directory tree failed, or an ignore file was malformed.
    #[error("cannot walk {}", root.display())]
    Walk {
        /// The root being scanned.
        root: PathBuf,
        /// The underlying error.
        source: ignore::Error,
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
