//! The package rules of a contract: `%% packages` and `%% forbid-packages` lines.

use std::fmt;

use super::NodeId;

/// Which of the two package directives a rule is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageRuleKind {
    /// `%% packages`: the third-party packages the node may use.
    Allow,
    /// `%% forbid-packages`: the packages, of any kind, the node may not use.
    Forbid,
}

impl PackageRuleKind {
    /// The word after `%%` that introduces the directive.
    #[must_use]
    pub const fn directive(self) -> &'static str {
        match self {
            Self::Allow => "packages",
            Self::Forbid => "forbid-packages",
        }
    }
}

/// One entry of a package rule: a package name, or a prefix when it ends in `*`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackagePattern(String);

impl PackagePattern {
    /// Validates and wraps `raw`, which is trimmed first.
    ///
    /// # Errors
    /// A sentence saying what is wrong: the name is empty, holds whitespace or a comma, or has a
    /// `*` anywhere but last.
    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        let name = raw.trim();
        if name.is_empty() {
            return Err("empty name in the list");
        }
        if name.chars().any(|c| c.is_whitespace() || c == ',') {
            return Err("a name has no whitespace and no comma");
        }
        if name.trim_end_matches('*').contains('*') || name.ends_with("**") {
            return Err("`*` is only valid as the last character of a name");
        }
        Ok(Self(name.to_owned()))
    }

    /// The pattern as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the package called `name` matches: equal to the pattern, or, for a pattern that ends
    /// in `*`, starting with everything before it.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        self.0
            .strip_suffix('*')
            .map_or_else(|| self.0 == name, |prefix| name.starts_with(prefix))
    }
}

impl fmt::Display for PackagePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One `%% packages` or `%% forbid-packages` line.
#[derive(Debug, Clone)]
pub struct PackageRule {
    /// 1-indexed line of the contract. Not part of equality: a rendered contract has other lines.
    pub line: u32,
    /// The node the rule restricts.
    pub node: NodeId,
    /// Allow-list or deny-list.
    pub kind: PackageRuleKind,
    /// The names, in the order written. Empty only for an allow-list.
    pub names: Vec<PackagePattern>,
}

impl PartialEq for PackageRule {
    fn eq(&self, other: &Self) -> bool {
        // why: render moves lines around, and a round trip must still compare equal.
        self.node == other.node && self.kind == other.kind && self.names == other.names
    }
}

impl Eq for PackageRule {}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(raw: &str) -> PackagePattern {
        PackagePattern::parse(raw).unwrap()
    }

    #[test]
    fn a_plain_name_matches_only_itself() {
        assert!(pattern("lit").matches("lit"));
        assert!(!pattern("lit").matches("lit-html"));
        assert!(!pattern("lit").matches("li"));
        assert!(pattern("node:child_process").matches("node:child_process"));
    }

    #[test]
    fn a_trailing_star_is_a_prefix_that_the_bare_name_matches_too() {
        assert!(pattern("@scope/*").matches("@scope/pkg"));
        assert!(!pattern("@scope/*").matches("@other/pkg"));
        assert!(pattern("serde*").matches("serde"));
        assert!(pattern("serde*").matches("serde_json"));
        assert!(!pattern("serde*").matches("rayon"));
        assert!(pattern("*").matches("anything"));
        assert!(pattern("node:*").matches("node:fs"));
    }

    #[test]
    fn names_are_trimmed_and_bad_ones_are_refused() {
        assert_eq!(pattern("  lit ").as_str(), "lit");
        for bad in ["", "  ", "a b", "a,b", "a*b", "*a", "a**", "**"] {
            assert!(PackagePattern::parse(bad).is_err(), "{bad:?}");
        }
    }
}
