//! Renders reports as text, JSON and SARIF.
//!
//! Every renderer is a pure function over [`LintReport`](crate::model::LintReport) that returns
//! a string. Nothing here prints: the CLI owns all output.

mod common;
mod diff;
mod error;
mod json;
mod overview;
mod sarif;
mod text;

#[cfg(test)]
mod fixtures;

pub use diff::{render_diff_json, render_diff_text};
pub use error::ReportError;
pub use json::render_json;
pub use overview::{
    render_node_view_json, render_node_view_text, render_overview_json, render_overview_text,
};
pub use sarif::render_sarif;
pub use text::render_text;
