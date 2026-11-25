//! What TypeScript and JavaScript resolution reads besides the repository files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::alias::AliasConfig;
use super::project::Project;
use super::vite_alias::ViteAlias;
use super::workspace::Workspaces;

/// The configuration the repository gives to its TypeScript, JavaScript, Svelte and HTML files.
#[derive(Debug, Clone, Copy)]
pub struct Env<'a> {
    /// Repository-relative directory holding a `tsconfig.json` or `jsconfig.json` to the alias
    /// configuration it defines. A file is governed by the nearest directory at or above it, see
    /// [`nearest`](super::nearest).
    pub aliases: &'a BTreeMap<PathBuf, AliasConfig>,
    /// Repository-relative directory holding a readable `package.json` to the package names that
    /// file declares in `dependencies`, `devDependencies`, `peerDependencies` and
    /// `optionalDependencies`. A file is governed by the nearest directory at or above it.
    pub packages: &'a BTreeMap<PathBuf, BTreeSet<String>>,
    /// The workspace members of the repository by package name, and the `imports` of its
    /// `package.json` files.
    pub workspaces: &'a Workspaces,
    /// Repository-relative directory holding a solution-style `tsconfig.json` to the projects its
    /// `references` name, in the order written.
    pub projects: &'a BTreeMap<PathBuf, Vec<Project>>,
    /// Repository-relative directory holding a `vite.config.*` file to the Vite root that config
    /// gives, see [`vite_roots`](super::vite_roots). Only the resolution of root-absolute paths in
    /// HTML reads it.
    pub vite_roots: &'a BTreeMap<PathBuf, PathBuf>,
    /// Repository-relative directory holding a `vite.config.*` file to the `resolve.alias` entries
    /// that config gives, in the order written, see [`vite_aliases`](super::vite_aliases). A file
    /// is governed by every config at or above it.
    pub vite_aliases: &'a BTreeMap<PathBuf, Vec<ViteAlias>>,
}
