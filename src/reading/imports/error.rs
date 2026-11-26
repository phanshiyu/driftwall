//! Errors raised while extracting imports.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Everything that can go wrong while extracting imports.
#[derive(Debug, Error)]
pub enum ImportsError {
    /// A source file could not be read, or was not valid UTF-8.
    #[error("cannot read `{}`", path.display())]
    Io {
        /// The repository-relative path that failed.
        path: PathBuf,
        /// The underlying IO error.
        source: io::Error,
    },
    /// A tree-sitter grammar was rejected by the linked tree-sitter runtime.
    #[error("tree-sitter rejected the grammar")]
    Grammar(#[from] tree_sitter::LanguageError),
    /// An import query did not compile against its grammar.
    #[error("import query does not compile")]
    Query(#[from] tree_sitter::QueryError),
}
