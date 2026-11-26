//! Who owns a file and which rule decided it, and what a path is: what `explain` and `locate` read
//! from the repository.

use std::path::{Path, PathBuf};

use super::{Exclusion, MapSite};
use crate::contract::NodeId;

/// What a walked path is to driftwall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    /// A source file in a language driftwall reads.
    Source,
    /// Any other file, such as `data.json`.
    Asset,
    /// A code file in a language driftwall cannot read, with its census key (`go`, `shebang`).
    Unread(String),
    /// A source file the scan leaves out as vendored, minified or generated.
    Excluded(Exclusion),
    /// A source file only `.driftwallignore` hides.
    Ignored,
    /// A directory.
    Directory,
    /// A path inside a nested repository (a submodule, a clone or a git worktree), which the walk
    /// never enters, so no node owns it.
    NestedRepository,
    /// An entry file that declares nothing and makes no node of its own under `node: *`.
    Folded,
    /// An `.arch` tag file: driftwall's own input, never an asset, and owned by no node.
    TagFile,
    /// The contract file. `ownership::locate` never answers it: the caller that knows the contract path
    /// replaces the kind of that path.
    Contract,
}

/// The answer of `ownership::locate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// What the path is.
    pub kind: FileKind,
    /// Who owns it and which rule decided, by the tag rules of source files, also for an excluded
    /// or ignored file. Unowned for a tag file, a contract, and a file no rule covers. A folded
    /// entry file is [`Owner::Folded`].
    pub owner: Owner,
}

/// The rule that decided a file's owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    /// An `.arch` tag file.
    Tag {
        /// The `.arch` file, relative to the root.
        arch_file: PathBuf,
    },
    /// A `%% map` glob of the contract.
    Map {
        /// The contract, relative to the root.
        contract: PathBuf,
        /// The directive line and glob that decided.
        site: MapSite,
    },
}

impl Rule {
    /// The file the rule is written in: the `.arch` file or the contract.
    #[must_use]
    pub fn file(&self) -> &Path {
        match self {
            Self::Tag { arch_file } => arch_file,
            Self::Map { contract, .. } => contract,
        }
    }

    /// The `%% map` line and glob, when a map decided.
    #[must_use]
    pub const fn site(&self) -> Option<&MapSite> {
        match self {
            Self::Tag { .. } => None,
            Self::Map { site, .. } => Some(site),
        }
    }

    /// The JSON name of the mechanism: `tag` or `map`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Tag { .. } => "tag",
            Self::Map { .. } => "map",
        }
    }
}

/// Who owns a file, and why: the answer to "who owns this path".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// No rule covers the file.
    Unowned,
    /// A rule gives the file a node.
    Owned {
        /// The owning node.
        node: NodeId,
        /// What decided it.
        rule: Rule,
    },
    /// A folded entry file: it declares nothing, so it has no node. Either a rule covers it and no
    /// other module of its `node: *` directory has its name, or no rule covers it at all.
    Folded {
        /// What covered it, `None` when no rule does.
        rule: Option<Rule>,
    },
}

impl Owner {
    /// The owning node, `None` when unowned or folded.
    #[must_use]
    pub const fn node(&self) -> Option<&NodeId> {
        match self {
            Self::Owned { node, .. } => Some(node),
            Self::Unowned | Self::Folded { .. } => None,
        }
    }

    /// The rule that decided, `None` when unowned.
    #[must_use]
    pub const fn rule(&self) -> Option<&Rule> {
        match self {
            Self::Owned { rule, .. } => Some(rule),
            Self::Folded { rule } => rule.as_ref(),
            Self::Unowned => None,
        }
    }

    /// The `.arch` file, or the contract when a `%% map` glob decided.
    #[must_use]
    pub fn arch_file(&self) -> Option<&Path> {
        self.rule().map(Rule::file)
    }

    /// The `%% map` line and glob that decided.
    #[must_use]
    pub fn site(&self) -> Option<&MapSite> {
        self.rule().and_then(Rule::site)
    }

    /// Whether the file is a folded entry file.
    #[must_use]
    pub const fn is_folded(&self) -> bool {
        matches!(self, Self::Folded { .. })
    }

    /// `tag` or `map` for an owned file, `None` when unowned or folded.
    #[must_use]
    pub const fn owned_by(&self) -> Option<&'static str> {
        match self {
            Self::Owned { rule, .. } => Some(rule.name()),
            Self::Unowned | Self::Folded { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str) -> NodeId {
        NodeId::new(name).unwrap()
    }

    fn tag() -> Rule {
        Rule::Tag {
            arch_file: PathBuf::from("a/.arch"),
        }
    }

    fn map() -> Rule {
        Rule::Map {
            contract: PathBuf::from("c.mmd"),
            site: MapSite {
                line: 4,
                glob: "src/".to_owned(),
                each: None,
                child: None,
            },
        }
    }

    #[test]
    fn an_unowned_file_has_nothing() {
        let owner = Owner::Unowned;
        assert_eq!(owner.node(), None);
        assert_eq!(owner.arch_file(), None);
        assert_eq!(owner.site(), None);
        assert_eq!(owner.owned_by(), None);
        assert!(!owner.is_folded());
    }

    #[test]
    fn a_tag_owner_names_the_arch_file_and_no_site() {
        let owner = Owner::Owned {
            node: node("a"),
            rule: tag(),
        };
        assert_eq!(owner.node(), Some(&node("a")));
        assert_eq!(owner.arch_file(), Some(Path::new("a/.arch")));
        assert_eq!(owner.site(), None);
        assert_eq!(owner.owned_by(), Some("tag"));
    }

    #[test]
    fn a_map_owner_names_the_contract_and_the_site() {
        let owner = Owner::Owned {
            node: node("a"),
            rule: map(),
        };
        assert_eq!(owner.arch_file(), Some(Path::new("c.mmd")));
        assert_eq!(owner.site().map(|site| site.line), Some(4));
        assert_eq!(owner.owned_by(), Some("map"));
    }

    #[test]
    fn a_folded_file_keeps_its_rule_but_has_no_node_and_is_not_owned() {
        let owner = Owner::Folded { rule: Some(tag()) };
        assert!(owner.is_folded());
        assert_eq!(owner.node(), None);
        assert_eq!(owner.arch_file(), Some(Path::new("a/.arch")));
        assert_eq!(owner.owned_by(), None);
    }
}
