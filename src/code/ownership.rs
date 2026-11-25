//! What the scan read of the repository's own files: tags, owners and the files it left out.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::Serialize;

use crate::contract::{NodeId, Prefix};

use super::Location;

/// The `node:` value of a `.arch` file that asks for one node per module.
pub const MODULES_MARK: &str = "*";

/// What a `.arch` file says about its directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    /// `node: <id>`: the directory and everything below it is one node.
    Node(NodeId),
    /// `node: <prefix>*`: every module directly in the directory is its own node, named by its
    /// position and the prefix (empty for a plain `node: *`).
    Modules(Prefix),
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(id) => id.fmt(f),
            Self::Modules(prefix) => write!(f, "{prefix}{MODULES_MARK}"),
        }
    }
}

/// How far a `.arch` file reaches below its own directory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// The default: the tag covers the directory and every subdirectory, until a nearer `.arch`.
    #[default]
    Subtree,
    /// `scope: here`: the tag covers only the files directly in its directory.
    Here,
}

/// The value of `scope:` that limits a tag to its own directory.
pub const SCOPE_HERE: &str = "here";

/// What one `.arch` file says: the tag and how far it reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirTag {
    /// What the directory is tagged with.
    pub tag: Tag,
    /// How far the tag reaches below the directory.
    pub scope: Scope,
    /// For a `node: *` tag: the names of the non-test modules directly in the directory (see
    /// `ownership::peers_in`), which decide the node of a test file there. Empty for any other tag.
    pub peers: BTreeSet<String>,
}

/// Why the scan left a source file out of the repository's own source. Serialized as the word
/// [`Exclusion::as_str`] gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Exclusion {
    /// The file is below a directory named `vendor` or `third_party`.
    Vendored,
    /// The file name has the form `<name>.min.<ext>`.
    Minified,
    /// The file is below a directory named `dist`, or announces itself as generated in its first
    /// lines.
    Generated,
}

impl Exclusion {
    /// The reason as a lowercase word, for reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vendored => "vendored",
            Self::Minified => "minified",
            Self::Generated => "generated",
        }
    }
}

/// Which node owns each source file, plus the tags that named unknown nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ownership {
    /// Owner per repository-relative source file. Files absent here are unmapped.
    pub owners: BTreeMap<PathBuf, NodeId>,
    /// Source files with no inherited tag.
    pub unmapped: BTreeSet<PathBuf>,
    /// Tags as (file, line, raw node name), for validating against the contract: `.arch` files,
    /// or the contract itself for the nodes a module-form `%% map` derives.
    pub tags: Vec<(Location, String)>,
    /// What each `.arch` file the scan saw says, by the repository-relative directory it is in
    /// (empty for the root).
    pub tag_dirs: BTreeMap<PathBuf, DirTag>,
    /// With `%% map` lines: the rule and glob that decided every owned file (source, asset or
    /// excluded). Empty when `.arch` files decide.
    pub map_sites: BTreeMap<PathBuf, MapSite>,
    /// With `%% map` lines: every glob that matches no walked file.
    pub stale_maps: Vec<MapSite>,
    /// With `%% map` lines: the `.arch` files that exist and are not read, sorted.
    pub ignored_tags: Vec<PathBuf>,
    /// Source files the scan left out, by reason. They are neither owned nor unmapped.
    pub excluded: BTreeMap<Exclusion, BTreeSet<PathBuf>>,
    /// Owner per excluded file that a tag covers, by the same rules as source files and assets.
    /// An excluded file is still an import target, so an import of it is a dependency on this
    /// node. It stays in [`Ownership::excluded`] and in no other field: one that no tag covers is
    /// in no map here.
    pub excluded_owners: BTreeMap<PathBuf, NodeId>,
    /// Owner per repository-relative asset: every other walked file (`data.json`, `style.css`),
    /// by the same tag rules as source files. Assets are not source, so they are in no other field
    /// and count in no file total.
    pub asset_owners: BTreeMap<PathBuf, NodeId>,
    /// Assets with no inherited tag, or whose module name under `node: *` is not a node id. Never
    /// a finding: they only let an import of them resolve.
    pub asset_unmapped: BTreeSet<PathBuf>,
    /// Walked code files driftwall has no language for, by key: the extension (`swift`, `sh`) or
    /// `shebang` for an extensionless script. Neither owned nor unmapped, and never a finding.
    pub unread: BTreeMap<String, BTreeSet<PathBuf>>,
    /// Entry files that declare nothing and that no other module of a `node: *` directory shares a
    /// name with (see "Folded entry files" in `docs/DESIGN.md`). They are neither owned nor
    /// unmapped, but they are known files, so an import of one resolves.
    pub folded: BTreeSet<PathBuf>,
    /// Source files that only `.driftwallignore` hides. Never owned or unmapped, and never a
    /// finding.
    pub ignored: BTreeSet<PathBuf>,
}

impl Ownership {
    /// The nodes that own at least one source file.
    #[must_use]
    pub fn owning_nodes(&self) -> BTreeSet<NodeId> {
        self.owners.values().cloned().collect()
    }
}

/// The rule and glob that decided a file's owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapSite {
    /// Line of the `%% map` directive.
    pub line: u32,
    /// The glob as written.
    pub glob: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_display_as_written_in_the_arch_file() {
        let modules = |prefix: &str| Tag::Modules(Prefix::new(prefix).unwrap_or_default());
        assert_eq!(modules("").to_string(), "*");
        assert_eq!(modules("ui-").to_string(), "ui-*");
        assert_eq!(Tag::Node(NodeId::new("core").unwrap()).to_string(), "core");
        assert_eq!(Scope::default(), Scope::Subtree);
    }
}
