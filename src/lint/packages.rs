//! The package rules: `%% packages` allow-lists and `%% forbid-packages` deny-lists, see "Package
//! rules" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::imports::FileImports;
use crate::model::{
    Contract, Location, NodeId, Ownership, PackageKind, PackagePattern, PackageRule,
    PackageRuleKind, PackageUse, Violation, ViolationKind,
};

use super::observe::declared_owner;
use super::rules::violation;

/// Every package use of the files of a declared node, with the node, in the order of `imports`.
pub(super) fn owned_uses<'a>(
    contract: &'a Contract,
    ownership: &'a Ownership,
    imports: &'a BTreeMap<PathBuf, FileImports>,
) -> impl Iterator<Item = (&'a NodeId, &'a PackageUse)> {
    imports
        .values()
        .flat_map(|found| &found.packages)
        .filter_map(|used| Some((declared_owner(contract, ownership, &used.at.file)?, used)))
}

/// The findings of the package rules: a `forbidden-package` per use that breaks a rule, and a
/// `stale-package` per allow-list entry no use matches.
///
/// A use in test code is exempt, and does not keep an entry alive. A deny-list wins over an
/// allow-list, which restricts third party packages only. Files without a declared owner produce
/// nothing.
pub(super) fn package_findings(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
    contract_file: &Path,
) -> Vec<Violation> {
    if contract.package_rules.is_empty() {
        return Vec::new();
    }
    let mut violations = Vec::new();
    let mut live: BTreeMap<&NodeId, Vec<&PackageUse>> = BTreeMap::new();
    for (node, used) in owned_uses(contract, ownership, imports) {
        if used.test_only {
            continue;
        }
        live.entry(node).or_default().push(used);
        violations.extend(forbidden(contract, node, used, contract_file));
    }
    for rule in &contract.package_rules {
        if rule.kind == PackageRuleKind::Allow {
            let uses = live.get(&rule.node).map_or(&[][..], Vec::as_slice);
            violations.extend(stale_entries(rule, uses, contract_file));
        }
    }
    violations
}

/// The `forbidden-package` finding for `used` written in `node`, if a rule forbids it.
fn forbidden(
    contract: &Contract,
    node: &NodeId,
    used: &PackageUse,
    contract_file: &Path,
) -> Option<Violation> {
    let rules = |kind: PackageRuleKind| {
        contract
            .package_rules
            .iter()
            .filter(move |rule| rule.node == *node && rule.kind == kind)
    };
    let name = &used.name;
    let kind = used.kind.as_str();
    let file = contract_file.display();
    if let Some(rule) = rules(PackageRuleKind::Forbid).find(|rule| rule.matches(used)) {
        return Some(violation(
            ViolationKind::ForbiddenPackage,
            Some(used.at.clone()),
            format!(
                "`{node}` imports `{name}` ({kind}) but {file}:{} `%% forbid-packages {node}` forbids it",
                rule.line
            ),
        ));
    }
    let allowed: Vec<&PackageRule> = rules(PackageRuleKind::Allow).collect();
    let first = allowed.first()?;
    if used.kind != PackageKind::ThirdParty || allowed.iter().any(|rule| rule.matches(used)) {
        return None;
    }
    let listed: Vec<&str> = allowed
        .iter()
        .flat_map(|rule| rule.names.iter().map(PackagePattern::as_str))
        .collect();
    let lists = if listed.is_empty() {
        "allows no third-party package".to_owned()
    } else {
        format!("lists only: {}", listed.join(", "))
    };
    Some(violation(
        ViolationKind::ForbiddenPackage,
        Some(used.at.clone()),
        format!(
            "`{node}` imports `{name}` ({kind}) but {file}:{} `%% packages {node}` {lists}",
            first.line
        ),
    ))
}

/// A `stale-package` per entry of the allow-list `rule` that none of `uses` matches.
fn stale_entries(rule: &PackageRule, uses: &[&PackageUse], contract_file: &Path) -> Vec<Violation> {
    let names: BTreeSet<&str> = uses.iter().map(|one| one.name.as_str()).collect();
    rule.names
        .iter()
        .filter(|entry| !names.iter().any(|name| entry.matches(name)))
        .map(|entry| {
            violation(
                ViolationKind::StalePackage,
                Some(Location {
                    file: contract_file.to_path_buf(),
                    line: rule.line,
                }),
                format!(
                    "`%% packages {}` lists `{entry}` but no code of `{}` uses it",
                    rule.node, rule.node
                ),
            )
        })
        .collect()
}
