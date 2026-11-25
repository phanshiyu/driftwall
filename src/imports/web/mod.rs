//! ECMAScript: TypeScript, JavaScript, Svelte and HTML scripts, with `tsconfig`, Vite and workspaces.

mod alias;
mod env;
mod jsdoc;
mod markup;
mod package;
mod project;
mod reexport;
mod resolve;
mod spec;
mod statement;
mod subpath;
mod symbols;
mod tsconfig;
mod vite;
mod vite_alias;
mod workspace;

#[cfg(test)]
pub use alias::{AliasConfig, Pattern, nearest};
pub use env::Env;
#[cfg(test)]
pub use markup::has_script;
pub use markup::{resolve_inline, resolve_load};
pub use package::package_dependencies;
pub use reexport::table as reexport_table;
pub use resolve::{ecmascript as resolve, specifier};
pub use spec::{HTML, JAVASCRIPT, SVELTE, TYPESCRIPT};
pub use statement::test_file;
pub use tsconfig::{alias_configs, project_configs};
pub use vite::vite_roots;
pub use vite_alias::vite_aliases;
#[cfg(test)]
pub use workspace::Workspaces;
pub use workspace::discover as workspaces;

#[cfg(test)]
pub use tsconfig::{MAX_EXTENDS_DEPTH, discover};

#[cfg(test)]
mod scenarios;
