//! Swift: module imports resolved to the `SwiftPM` targets of the repository.

mod package;
mod resolve;
mod spec;
mod statement;
mod system;

pub use package::{SwiftTarget, swift_targets};
pub use resolve::{SwiftModules, resolve};
pub use spec::SPEC;
pub use statement::test_file;
