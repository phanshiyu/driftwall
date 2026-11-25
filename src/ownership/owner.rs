//! Who owns a file and which rule decided it: the answer type of `explain` and `locate`.

use std::path::{Path, PathBuf};

use crate::model::{MapSite, NodeId};

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
    /// A rule covers the file, but it is a folded entry file: it declares nothing and no other
    /// module of its `node: *` directory has its name, so it has no node.
    Folded {
        /// What covered it.
        rule: Rule,
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
            Self::Owned { rule, .. } | Self::Folded { rule } => Some(rule),
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
        let owner = Owner::Folded { rule: tag() };
        assert!(owner.is_folded());
        assert_eq!(owner.node(), None);
        assert_eq!(owner.arch_file(), Some(Path::new("a/.arch")));
        assert_eq!(owner.owned_by(), None);
    }
}
