//! Python: imports resolved against import roots, with the standard library and re-exports.

mod bindings;
mod call;
mod config;
mod layout;
mod reexport;
mod resolve;
mod spec;
mod statement;
mod stdlib;

pub use bindings::Bindings;
pub use config::python_roots;
pub use layout::PythonLayout;
pub use reexport::table as reexport_table;
pub use resolve::resolve;
pub use spec::SPEC;
pub use statement::test_file;

#[cfg(test)]
mod scenarios;
