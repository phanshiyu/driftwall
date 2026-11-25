//! The package rules through [`reconcile`], see "Package rules" in `docs/DESIGN.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::fixtures::{at, contract, node, ownership};
use super::*;
use crate::diagram;
use crate::imports::FileImports;
use crate::model::{PackageKind, PackageUse, Severity, Violation, ViolationKind};

const CONTRACT_FILE: &str = "architecture.mmd";

/// Contract with nodes `db`, `ui` and `core`, and the rule lines in `rules`.
fn rules(rules: &str) -> Contract {
    let src = format!("flowchart LR\n  db\n  ui\n  core\n{rules}");
    diagram::parse(&src).unwrap()
}

/// Package uses from (file, line, name, kind, test only).
fn uses(entries: &[(&str, u32, &str, PackageKind, bool)]) -> BTreeMap<PathBuf, FileImports> {
    let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
    for &(file, line, name, kind, test_only) in entries {
        map.entry(PathBuf::from(file))
            .or_default()
            .packages
            .push(PackageUse {
                at: at(file, line),
                name: name.to_owned(),
                kind,
                test_only,
            });
    }
    map
}

fn owners() -> Ownership {
    ownership(
        &[
            ("db/a.ts", "db"),
            ("db/b.ts", "db"),
            ("ui/a.ts", "ui"),
            ("core/a.ts", "core"),
        ],
        &[],
    )
}

fn lint(contract: &Contract, uses: &BTreeMap<PathBuf, FileImports>) -> Vec<Violation> {
    reconcile(contract, &owners(), uses, Path::new(CONTRACT_FILE)).violations
}

use PackageKind::{Stdlib, ThirdParty};

fn summary(found: &[Violation]) -> Vec<(ViolationKind, Option<(String, u32)>)> {
    found
        .iter()
        .map(|v| {
            (
                v.kind,
                v.at.as_ref()
                    .map(|at| (at.file.display().to_string(), at.line)),
            )
        })
        .collect()
}

#[test]
fn a_node_without_a_rule_is_unrestricted() {
    let found = lint(
        &rules("  %% packages db: sqlx\n"),
        &uses(&[
            ("db/a.ts", 1, "sqlx", ThirdParty, false),
            ("ui/a.ts", 1, "lit", ThirdParty, false),
            ("ui/a.ts", 2, "node:fs", Stdlib, false),
        ]),
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn an_allow_list_forbids_every_third_party_package_it_does_not_list() {
    let found = lint(
        &rules("  %% packages db: sqlx, tokio*, @scope/*\n"),
        &uses(&[
            ("db/a.ts", 1, "sqlx", ThirdParty, false),
            ("db/a.ts", 2, "tokio_util", ThirdParty, false),
            ("db/a.ts", 3, "@scope/x", ThirdParty, false),
            ("db/a.ts", 4, "lit", ThirdParty, false),
            ("db/b.ts", 9, "three", ThirdParty, false),
        ]),
    );
    assert_eq!(
        summary(&found),
        [
            (ViolationKind::ForbiddenPackage, Some(("db/a.ts".into(), 4))),
            (ViolationKind::ForbiddenPackage, Some(("db/b.ts".into(), 9))),
        ]
    );
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(
        found[0].message,
        "`db` imports `lit` (third-party) but architecture.mmd:5 `%% packages db` lists only: sqlx, tokio*, @scope/*"
    );
}

#[test]
fn an_allow_list_does_not_restrict_the_standard_library() {
    let found = lint(
        &rules("  %% packages db: sqlx\n"),
        &uses(&[
            ("db/a.ts", 1, "sqlx", ThirdParty, false),
            ("db/a.ts", 2, "node:fs", Stdlib, false),
            ("db/a.ts", 3, "node:child_process", Stdlib, false),
        ]),
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn an_empty_allow_list_means_no_third_party_package_at_all() {
    let found = lint(
        &rules("  %% packages core:\n"),
        &uses(&[
            ("core/a.ts", 1, "node:fs", Stdlib, false),
            ("core/a.ts", 2, "zod", ThirdParty, false),
        ]),
    );
    assert_eq!(
        summary(&found),
        [(
            ViolationKind::ForbiddenPackage,
            Some(("core/a.ts".into(), 2))
        )]
    );
    assert!(
        found[0]
            .message
            .ends_with("`%% packages core` allows no third-party package"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_deny_list_applies_to_every_kind_and_names_its_line() {
    let found = lint(
        &rules("  %% forbid-packages core: node:child_process, lit*\n"),
        &uses(&[
            ("core/a.ts", 1, "node:child_process", Stdlib, false),
            ("core/a.ts", 2, "lit-html", ThirdParty, false),
            ("core/a.ts", 3, "node:fs", Stdlib, false),
            ("core/a.ts", 4, "zod", ThirdParty, false),
        ]),
    );
    assert_eq!(
        summary(&found),
        [
            (
                ViolationKind::ForbiddenPackage,
                Some(("core/a.ts".into(), 1))
            ),
            (
                ViolationKind::ForbiddenPackage,
                Some(("core/a.ts".into(), 2))
            ),
        ]
    );
    assert_eq!(
        found[0].message,
        "`core` imports `node:child_process` (stdlib) but architecture.mmd:5 `%% forbid-packages core` forbids it"
    );
}

#[test]
fn the_deny_list_wins_over_the_allow_list_and_carves_an_exception_out_of_a_prefix() {
    let found = lint(
        &rules("  %% packages ui: lit*\n  %% forbid-packages ui: lit-ssr\n"),
        &uses(&[
            ("ui/a.ts", 1, "lit", ThirdParty, false),
            ("ui/a.ts", 2, "lit-ssr", ThirdParty, false),
        ]),
    );
    assert_eq!(
        summary(&found),
        [(ViolationKind::ForbiddenPackage, Some(("ui/a.ts".into(), 2)))]
    );
    assert!(
        found[0].message.contains("forbids it"),
        "{}",
        found[0].message
    );
}

#[test]
fn several_lines_of_one_node_add_up() {
    let found = lint(
        &rules(
            "  %% packages db: sqlx\n  %% packages db: tokio\n  %% forbid-packages db: node:fs\n  %% forbid-packages db: node:net\n",
        ),
        &uses(&[
            ("db/a.ts", 1, "sqlx", ThirdParty, false),
            ("db/a.ts", 2, "tokio", ThirdParty, false),
            ("db/a.ts", 3, "node:fs", Stdlib, false),
            ("db/a.ts", 4, "node:net", Stdlib, false),
            ("db/a.ts", 5, "node:os", Stdlib, false),
        ]),
    );
    assert_eq!(
        summary(&found),
        [
            (ViolationKind::ForbiddenPackage, Some(("db/a.ts".into(), 3))),
            (ViolationKind::ForbiddenPackage, Some(("db/a.ts".into(), 4))),
        ]
    );
}

#[test]
fn test_code_is_exempt_from_both_kinds_of_rule() {
    let found = lint(
        &rules("  %% packages db: sqlx\n  %% forbid-packages db: node:fs\n"),
        &uses(&[
            ("db/a.ts", 1, "sqlx", ThirdParty, false),
            ("db/a.ts", 2, "vitest", ThirdParty, true),
            ("db/a.ts", 3, "node:fs", Stdlib, true),
        ]),
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn files_without_a_declared_owner_produce_no_finding() {
    let mut own = owners();
    own.owners.insert("ghost/a.ts".into(), node("ghost"));
    let found = reconcile(
        &rules("  %% packages db:\n"),
        &own,
        &uses(&[
            ("ghost/a.ts", 1, "lit", ThirdParty, false),
            ("loose/a.ts", 1, "lit", ThirdParty, false),
        ]),
        Path::new(CONTRACT_FILE),
    );
    assert_eq!(found.violations.len(), 0, "{:?}", found.violations);
}

#[test]
fn an_allow_entry_no_non_test_use_matches_is_stale_at_its_line() {
    let found = lint(
        &rules("  %% packages db: sqlx, redis, aws*, vitest\n"),
        &uses(&[
            ("db/a.ts", 1, "sqlx", ThirdParty, false),
            ("db/a.ts", 2, "aws-sdk", ThirdParty, false),
            ("db/b.ts", 3, "vitest", ThirdParty, true),
            ("ui/a.ts", 1, "redis", ThirdParty, false),
        ]),
    );
    assert_eq!(
        summary(&found),
        [
            (ViolationKind::StalePackage, Some((CONTRACT_FILE.into(), 5))),
            (ViolationKind::StalePackage, Some((CONTRACT_FILE.into(), 5))),
        ]
    );
    let messages: Vec<&str> = found.iter().map(|v| v.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "`%% packages db` lists `redis` but no code of `db` uses it",
            "`%% packages db` lists `vitest` but no code of `db` uses it",
        ]
    );
    assert_eq!(found[0].severity, Severity::Warning);
}

#[test]
fn a_stdlib_entry_is_kept_alive_by_a_stdlib_use_and_a_deny_entry_is_never_stale() {
    let found = lint(
        &rules("  %% packages db: node:fs\n  %% forbid-packages db: node:child_process\n"),
        &uses(&[("db/a.ts", 1, "node:fs", Stdlib, false)]),
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn a_node_that_owns_no_file_has_every_entry_stale() {
    let found = lint(&rules("  %% packages core: zod\n"), &BTreeMap::new());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, ViolationKind::StalePackage);
}

#[test]
fn a_contract_without_package_rules_reports_nothing_whatever_the_uses() {
    let found = lint(
        &contract(&["db", "ui", "core"], &[]),
        &uses(&[("db/a.ts", 1, "lit", ThirdParty, false)]),
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn node_packages_count_uses_and_test_uses_third_party_first_then_by_name() {
    let summary = packages::node_packages(
        &rules(""),
        &owners(),
        &uses(&[
            ("db/a.ts", 1, "node:fs", Stdlib, false),
            ("db/a.ts", 2, "zod", ThirdParty, false),
            ("db/b.ts", 3, "zod", ThirdParty, true),
            ("db/b.ts", 4, "lit", ThirdParty, false),
            ("loose/c.ts", 1, "lit", ThirdParty, false),
        ]),
    );
    let db: Vec<(&str, &str, usize, usize)> = summary[&node("db")]
        .iter()
        .map(|p| (p.name.as_str(), p.kind.as_str(), p.count, p.tests))
        .collect();
    assert_eq!(
        db,
        [
            ("lit", "third-party", 1, 0),
            ("zod", "third-party", 2, 1),
            ("node:fs", "stdlib", 1, 0),
        ]
    );
    assert_eq!(summary.len(), 1);
}
