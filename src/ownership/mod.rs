//! Walks the repository and maps every file to its owning node via `.arch` files.

mod language;

pub use language::language_of;
