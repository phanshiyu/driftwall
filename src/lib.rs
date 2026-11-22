//! driftwall: an architecture contract linter.
//!
//! A Mermaid diagram (`architecture.mmd`) declares nodes and the dependencies allowed between
//! them. Directories are tagged to nodes with `.arch` files. The linter extracts the real import
//! graph and reports every place the code disagrees with the diagram.

pub mod diagram;
pub mod imports;
pub mod init;
pub mod lint;
pub mod markup;
pub mod model;
pub mod ownership;
pub mod report;
