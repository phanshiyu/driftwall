//! Renders reports as text, JSON and SARIF.
//!
//! Every renderer is a pure function over [`LintReport`](crate::model::LintReport) that returns
//! a string. Nothing here prints: the CLI owns all output.

mod blind;
mod common;
mod diff;
mod error;
mod group;
mod json;
mod locate;
mod overview;
mod sarif;
mod text;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod group_tests;

pub use blind::render_blind_note;
pub use diff::{
    render_diff_json, render_diff_json_with_debt, render_diff_text, render_diff_text_with_debt,
};
pub use error::ReportError;
pub use json::{render_json, render_json_baselined};
pub use locate::{render_where_json, render_where_text};
pub use overview::{
    NodeViewOptions, render_node_view_json, render_node_view_text, render_overview_json,
    render_overview_text,
};
pub use sarif::{render_sarif, render_sarif_baselined};
pub use text::{render_text, render_text_baselined};
