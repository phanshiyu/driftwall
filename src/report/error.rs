//! Errors raised while rendering a report.

use thiserror::Error;

/// Errors raised by the structured renderers.
#[derive(Debug, Error)]
pub enum ReportError {
    /// The report could not be serialised, for example a path that is not valid UTF-8.
    #[error("failed to serialise report")]
    Serialize(#[from] serde_json::Error),
}
