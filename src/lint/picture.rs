//! `driftwall render`: reads the contract and draws it, see "Contract picture" in
//! `docs/DESIGN.md`. It needs the contract only, not the code.

use std::path::Path;

use super::{LintError, read_contract};
use crate::diagram;
use crate::model::{MarkdownSync, PictureView};

/// The name the picture comments give the contract: its file name.
fn source_name(contract: &Path) -> String {
    contract
        .file_name()
        .unwrap_or(contract.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The Mermaid picture of the contract at `contract`, relative to `root`, for `view`, with labels
/// cut before their first `:` when `short_labels`.
///
/// # Errors
/// The contract cannot be read or parsed, the depth is 0, or the focus is not a boundary.
pub fn render_picture(
    root: &Path,
    contract: &Path,
    view: &PictureView,
    short_labels: bool,
) -> Result<String, LintError> {
    let parsed = read_contract(root, contract)?;
    Ok(diagram::picture(
        &parsed,
        view,
        short_labels,
        &source_name(contract),
    )?)
}

/// Rewrites the `driftwall render` blocks of `markdown` from the contract at `contract`, relative
/// to `root`, and says which blocks were not current.
///
/// # Errors
/// The contract cannot be read or parsed, or a block or marker is malformed.
pub fn sync_markdown(
    root: &Path,
    contract: &Path,
    markdown: &str,
) -> Result<MarkdownSync, LintError> {
    let parsed = read_contract(root, contract)?;
    Ok(diagram::sync(markdown, &parsed, &source_name(contract))?)
}
