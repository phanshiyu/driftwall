//! What `driftwall render` draws and what syncing a Markdown file answers.

/// What `driftwall render` draws of a contract, see "Contract picture" in `docs/DESIGN.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PictureView {
    /// Every boundary, node and edge.
    Full,
    /// Boundaries at this depth (top level is 1) as one box each, contents hidden.
    Depth(u32),
    /// One boundary in full, with what it touches outside as the outermost boundary holding it.
    Focus(String),
}

/// The result of syncing the `driftwall render` blocks of a Markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownSync {
    /// The file with every block rewritten, everything else byte-identical.
    pub text: String,
    /// The 1-indexed line of the start marker of each block that was not current, in file order.
    pub stale: Vec<u32>,
}
