//! Shell: the files a script loads with `source` and `.`.

mod resolve;
mod spec;
mod statement;

pub use resolve::resolve;
pub use spec::SPEC;
pub use statement::test_file;
