//! Uses of code outside the repository.

use serde::Serialize;

use crate::contract::PackageRule;

use super::Location;

/// Where the code a package use names comes from. Serialized in kebab-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageKind {
    /// Code the repository depends on and does not contain.
    ThirdParty,
    /// What the language or platform ships: a Node builtin, the Python standard library, `std`.
    Stdlib,
}

impl PackageKind {
    /// The kind as the contract and the reports write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ThirdParty => "third-party",
            Self::Stdlib => "stdlib",
        }
    }
}

/// One import of code outside the repository, see "Package uses" in `docs/DESIGN.md`.
///
/// Serialized flat: `file`, `line`, `name`, `kind`, `test_only`. Uses sort by location, then name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PackageUse {
    /// Where the import statement starts.
    #[serde(flatten)]
    pub at: Location,
    /// The package, as the language writes it: `lit`, `@scope/pkg`, `node:fs`, `yaml`, `serde_json`.
    pub name: String,
    /// Third party or standard library.
    pub kind: PackageKind,
    /// Whether the import is written in test code.
    pub test_only: bool,
}

impl PackageRule {
    /// Whether a name of this rule matches `package`.
    #[must_use]
    pub fn matches(&self, package: &PackageUse) -> bool {
        self.names.iter().any(|name| name.matches(&package.name))
    }
}
