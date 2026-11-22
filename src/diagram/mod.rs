//! Parses and renders the architecture contract (a strict Mermaid flowchart subset).
//!
//! The accepted grammar is specified in `docs/DESIGN.md` under "Contract format". Anything
//! outside it is a hard [`DiagramError`] carrying a line number.

mod error;
mod parse;
mod render;

pub use error::DiagramError;
pub use parse::parse;
pub use render::render;
