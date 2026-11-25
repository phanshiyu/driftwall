//! The read-only views of the architecture: `brief`, `neighbors`, `where` and the contract `diff`.
//!
//! Each view takes what [`lint`](crate::lint) observed, or the contract and ownership read
//! directly, and returns a value of [`model`](crate::model) types for [`report`](crate::report) to
//! render. Nothing here renders or prints.

mod diff;
mod error;
mod locate;
mod moves;
mod overview;

#[cfg(test)]
mod boundary_edge_tests;
#[cfg(test)]
mod each_tests;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod package_tests;

pub use diff::{EMPTY_CONTRACT, contract_diff};
pub use error::ViewsError;
pub use locate::locate;
pub use moves::{Owners, moved, owners};
pub use overview::{attach_budgets, node_view, overview, with_interface, with_packages};
