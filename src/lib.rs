//! driftwall: an architecture contract linter.
//!
//! A Mermaid diagram (`architecture.mmd`) declares nodes and the dependencies allowed between
//! them. `%% map` lines in the contract (or `.arch` tag files) say which node owns each file. The linter extracts the real import
//! graph and reports every place the code disagrees with the diagram.

pub mod code;
pub mod contract;
pub mod diagram;
pub mod imports;
pub mod init;
pub mod lint;
pub mod markup;
pub mod ownership;
pub mod report;
pub mod results;
pub mod views;
