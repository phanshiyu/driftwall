//! Renders reports as text, JSON and SARIF.
//!
//! Every renderer is a pure function over [`LintReport`](crate::model::LintReport) that returns
//! a string. Nothing here prints: the CLI owns all output.

mod common;
mod error;
mod json;
mod sarif;
mod text;

#[cfg(test)]
mod fixtures;

pub use error::ReportError;
pub use json::render_json;
pub use sarif::render_sarif;
pub use text::render_text;
