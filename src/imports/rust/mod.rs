//! Rust: `use` paths and `mod` declarations resolved to repository files, and the Cargo manifests.

mod cargo;
mod env;
mod manifest;
mod module;
mod path_modules;
mod reexport;
mod resolve;
mod scope;
mod spec;
mod statement;
mod test_modules;
mod tree;

pub use cargo::cargo_dependencies;
pub use env::Env;
pub use manifest::library_crates;
pub use path_modules::{PathModules, path_modules};
pub use reexport::table as reexport_table;
pub use resolve::resolve;
pub use scope::Bindings;
pub use spec::SPEC;
pub use statement::test_file as is_test_file;
pub use test_modules::test_files;

#[cfg(test)]
mod scenarios;
