//! What can stop a view from being made.

use thiserror::Error;

use crate::diagram::DiagramError;
use crate::lint::LintError;
use crate::ownership::OwnershipError;

/// Everything that can stop a view from being made.
///
/// A variant's message names only its own context. The cause is reachable through `source()`.
#[derive(Debug, Error)]
pub enum ViewsError {
    /// The scan or the contract behind the view failed.
    #[error(transparent)]
    Lint(#[from] LintError),
    /// Locating a path failed.
    #[error(transparent)]
    Ownership(#[from] OwnershipError),
    /// The old side of a contract comparison is not a valid contract.
    #[error("the old contract is not valid")]
    OldContract {
        /// What is wrong with it, including the line.
        source: DiagramError,
    },
    /// The new side of a contract comparison is not a valid contract.
    #[error("the new contract is not valid")]
    NewContract {
        /// What is wrong with it, including the line.
        source: DiagramError,
    },
}
