//! The pure core of the linter: compares three in-memory views and reports disagreements.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::imports::FileImports;
use crate::model::{
    Contract, Edge, EdgeKind, LintReport, Location, Ownership, ResolvedImport, Symbol, Violation,
    ViolationKind,
};

use super::observe::crossings;
use super::rules::violation;

/// Reconciles the contract, ownership and import graph into a sorted, duplicate-free report.
///
/// This function is pure: it touches no filesystem, clock or randomness, and the result does
/// not depend on the order in which the inputs were built. `contract_file` is the path of the
/// contract relative to the repository root; it is only used to name the contract in messages.
///
/// An import counts as an edge only when it crosses between two declared owners (see
/// [`observed_edges`](super::observed_edges), which shares the computation). Anything else (an
/// unmapped file, an owner that names an unknown node, an import within one node) produces no
/// edge findings, so one root cause is reported once. An edge is realised when code uses it,
/// whatever its declared kind.
///
/// An import that crosses into a node with a declared surface (`Contract::surfaces`) is also
/// checked against it: every symbol it uses must be listed, and `*` is private too. The same
/// crossing rules apply, so an unmapped or unknown owner and an import within one node never
/// produce a `private-symbol` finding.
#[must_use]
pub fn reconcile(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
    contract_file: &Path,
) -> LintReport {
    let mut violations = Vec::new();

    for (at, name) in &ownership.tags {
        if !declared(contract, name) {
            let source = if at.file == contract_file {
                "`%% map` derives"
            } else {
                "`.arch` tags"
            };
            violations.push(violation(
                ViolationKind::UnknownNode,
                Some(at.clone()),
                format!(
                    "{source} `{name}` but {} declares no such node",
                    contract_file.display()
                ),
            ));
        }
    }

    violations.extend(map_findings(ownership, contract_file));

    for file in &ownership.unmapped {
        violations.push(violation(
            ViolationKind::UnmappedFile,
            Some(Location {
                file: file.clone(),
                line: 1,
            }),
            format!("`{}` has no owning node", file.display()),
        ));
    }

    let crossings = crossings(contract, ownership, imports);
    for (edge, import) in &crossings {
        match contract.edges.get(edge) {
            None => violations.push(violation(
                ViolationKind::UndeclaredEdge,
                Some(import.at.clone()),
                format!(
                    "`{from}` imports `{to}` ({}) but {} declares no edge {from} --> {to}",
                    import.target.display(),
                    contract_file.display(),
                    from = edge.from,
                    to = edge.to,
                ),
            )),
            Some(kind) if !kind.allows(import.kind, import.test_only) => {
                violations.push(violation(
                    ViolationKind::UndeclaredEdge,
                    Some(import.at.clone()),
                    restricted_site_message(*kind, edge, import, contract_file),
                ));
            }
            Some(_) => {}
        }
        violations.extend(private_symbols(contract, edge, import, contract_file));
    }
    for file_imports in imports.values() {
        for import in &file_imports.unresolved {
            violations.push(violation(
                ViolationKind::UnresolvedImport,
                Some(import.at.clone()),
                format!("import `{}` resolves to no file", import.specifier),
            ));
        }
    }

    violations.extend(edge_findings(contract, &crossings));

    violations.sort();
    violations.dedup();
    LintReport { violations }
}

/// The message of a site `edge` does not allow: the pair has an edge, but only of a restricted
/// kind (`test`, `type` or both), and the site is not one it allows.
fn restricted_site_message(
    kind: EdgeKind,
    edge: &Edge,
    import: &ResolvedImport,
    contract_file: &Path,
) -> String {
    let (from, to) = (&edge.from, &edge.to);
    let target = import.target.display();
    let contract = contract_file.display();
    let arrow = kind.arrow();
    match kind {
        EdgeKind::Test | EdgeKind::PlannedTest => format!(
            "`{from}` imports `{to}` ({target}) from production code but {contract} declares only a test edge {from} {arrow} {to}, which allows test imports only"
        ),
        EdgeKind::Type | EdgeKind::PlannedType => format!(
            "`{from}` imports `{to}` ({target}) at run time but {contract} declares only a type edge {from} {arrow} {to}, which allows type-only imports only"
        ),
        _ => format!(
            "`{from}` imports `{to}` ({target}) at run time from production code but {contract} declares only a type, test edge {from} {arrow} {to}, which allows type-only and test imports only"
        ),
    }
}

/// The findings about declared edges: stale, planned and realised, and solid but realised by tests
/// only. A site an edge does not allow is not counted: it is its own `undeclared-edge`.
fn edge_findings(contract: &Contract, crossings: &[(Edge, &ResolvedImport)]) -> Vec<Violation> {
    let realised: BTreeSet<&Edge> = crossings.iter().map(|(edge, _)| edge).collect();
    let allowed: BTreeSet<&Edge> = crossings
        .iter()
        .filter(|(edge, import)| {
            contract
                .edges
                .get(edge)
                .is_some_and(|kind| kind.allows(import.kind, import.test_only))
        })
        .map(|(edge, _)| edge)
        .collect();
    let production: BTreeSet<&Edge> = crossings
        .iter()
        .filter(|(_, import)| !import.test_only)
        .map(|(edge, _)| edge)
        .collect();
    let mut violations = Vec::new();
    for (edge, kind) in &contract.edges {
        let (from, to) = (&edge.from, &edge.to);
        let arrow = kind.arrow();
        let sites = kind.allowed_sites();
        let finding = match kind {
            EdgeKind::Implemented if !realised.contains(edge) => Some((
                ViolationKind::StaleEdge,
                format!("edge {from} {arrow} {to} is declared but no import realises it"),
            )),
            EdgeKind::Implemented if !production.contains(edge) => Some((
                ViolationKind::TestOnlyEdge,
                format!(
                    "edge {from} --> {to} is realised only by test code; declare it {from} -->|test| {to}"
                ),
            )),
            EdgeKind::Test | EdgeKind::Type | EdgeKind::TypeTest if !allowed.contains(edge) => {
                Some((
                    ViolationKind::StaleEdge,
                    format!(
                        "edge {from} {arrow} {to} is declared but no {sites} import realises it"
                    ),
                ))
            }
            EdgeKind::Planned if realised.contains(edge) => Some((
                ViolationKind::PlannedRealized,
                format!("planned edge {from} {arrow} {to} is now realised by code"),
            )),
            EdgeKind::PlannedTest | EdgeKind::PlannedType | EdgeKind::PlannedTypeTest
                if allowed.contains(edge) =>
            {
                Some((
                    ViolationKind::PlannedRealized,
                    format!("planned edge {from} {arrow} {to} is now realised by {sites} code"),
                ))
            }
            _ => None,
        };
        if let Some((kind, message)) = finding {
            violations.push(violation(kind, None, message));
        }
    }
    violations
}

/// One finding per symbol `import` uses that the surface of the node it enters does not list.
/// Nothing when that node declares no surface. An import that uses no symbol is never private,
/// and `*` is always private: the importer reaches names that are never seen.
fn private_symbols(
    contract: &Contract,
    edge: &Edge,
    import: &ResolvedImport,
    contract_file: &Path,
) -> Vec<Violation> {
    let Some(surface) = contract.surfaces.get(&edge.to) else {
        return Vec::new();
    };
    let listed = surface.iter().map(String::as_str).collect::<Vec<_>>();
    import
        .symbols
        .iter()
        .filter(|symbol| match symbol {
            Symbol::Name(name) => !surface.contains(name),
            Symbol::All => true,
        })
        .map(|symbol| {
            violation(
                ViolationKind::PrivateSymbol,
                Some(import.at.clone()),
                format!(
                    "`{from}` imports `{symbol}` from `{to}` ({}) but {} lets `{to}` export only: {}",
                    import.target.display(),
                    contract_file.display(),
                    listed.join(", "),
                    from = edge.from,
                    to = edge.to,
                ),
            )
        })
        .collect()
}

/// The findings about `%% map` lines: globs that match nothing, and `.arch` files nobody reads.
fn map_findings(ownership: &Ownership, contract_file: &Path) -> Vec<Violation> {
    let mut violations = Vec::new();
    for site in &ownership.stale_maps {
        violations.push(violation(
            ViolationKind::StaleMap,
            Some(Location {
                file: contract_file.to_path_buf(),
                line: site.line,
            }),
            format!("map glob `{}` matches no file", site.glob),
        ));
    }
    if !ownership.ignored_tags.is_empty() {
        let shown: Vec<String> = ownership
            .ignored_tags
            .iter()
            .take(3)
            .map(|path| path.display().to_string())
            .collect();
        let more = ownership.ignored_tags.len().saturating_sub(shown.len());
        let rest = if more == 0 {
            String::new()
        } else {
            format!(" and {more} more")
        };
        violations.push(violation(
            ViolationKind::IgnoredTagFiles,
            None,
            format!(
                "{} has `%% map` lines, so {} `.arch` files are not read: {}{rest}",
                contract_file.display(),
                ownership.ignored_tags.len(),
                shown.join(", ")
            ),
        ));
    }
    violations
}

/// True when `name` is a node declared in the contract.
fn declared(contract: &Contract, name: &str) -> bool {
    contract.nodes.keys().any(|id| id.as_str() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NodeId, NodeInfo, ResolvedImport, Severity, UnresolvedImport, Violation};

    const CONTRACT_FILE: &str = "architecture.mmd";

    fn node(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn at(file: &str, line: u32) -> Location {
        Location {
            file: PathBuf::from(file),
            line,
        }
    }

    fn contract(nodes: &[&str], edges: &[(&str, &str, EdgeKind)]) -> Contract {
        Contract {
            nodes: nodes
                .iter()
                .map(|n| {
                    (
                        node(n),
                        NodeInfo {
                            label: None,
                            boundary: None,
                        },
                    )
                })
                .collect(),
            edges: edges
                .iter()
                .map(|&(from, to, kind)| {
                    (
                        Edge {
                            from: node(from),
                            to: node(to),
                        },
                        kind,
                    )
                })
                .collect(),
            ..Contract::default()
        }
    }

    fn ownership(owners: &[(&str, &str)], unmapped: &[&str]) -> Ownership {
        Ownership {
            owners: owners
                .iter()
                .map(|&(file, owner)| (PathBuf::from(file), node(owner)))
                .collect(),
            unmapped: unmapped.iter().map(PathBuf::from).collect(),
            ..Ownership::default()
        }
    }

    /// Builds imports from (importing file, line, target file) triples.
    fn imports(resolved: &[(&str, u32, &str)]) -> BTreeMap<PathBuf, FileImports> {
        let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
        for &(file, line, target) in resolved {
            map.entry(PathBuf::from(file))
                .or_default()
                .resolved
                .push(ResolvedImport::new(at(file, line), PathBuf::from(target)));
        }
        map
    }

    fn lint(
        contract: &Contract,
        ownership: &Ownership,
        imports: &BTreeMap<PathBuf, FileImports>,
    ) -> Vec<Violation> {
        reconcile(contract, ownership, imports, Path::new(CONTRACT_FILE)).violations
    }

    fn kinds(violations: &[Violation]) -> Vec<ViolationKind> {
        violations.iter().map(|v| v.kind).collect()
    }

    #[test]
    fn empty_inputs_give_an_empty_report() {
        let report = reconcile(
            &Contract::default(),
            &Ownership::default(),
            &BTreeMap::new(),
            Path::new(CONTRACT_FILE),
        );
        assert_eq!(report, LintReport::default());
    }

    #[test]
    fn unknown_tag_is_an_error_at_the_arch_file() {
        let mut own = ownership(&[], &[]);
        own.tags = vec![
            (at("a/.arch", 1), "a".to_owned()),
            (at("b/.arch", 3), "ghost".to_owned()),
        ];
        let found = lint(&contract(&["a"], &[]), &own, &BTreeMap::new());
        assert_eq!(found.len(), 1);
        let v = &found[0];
        assert_eq!(v.kind, ViolationKind::UnknownNode);
        assert_eq!(v.severity, Severity::Error);
        assert_eq!(v.at, Some(at("b/.arch", 3)));
        assert!(v.message.contains("ghost"), "{}", v.message);
        assert!(v.hint.as_deref().unwrap().contains("approved"));
    }

    #[test]
    fn unmapped_file_is_an_error_at_line_one() {
        let own = ownership(&[], &["x/new.ts"]);
        let found = lint(&contract(&["a"], &[]), &own, &BTreeMap::new());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::UnmappedFile);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].at, Some(at("x/new.ts", 1)));
        assert!(found[0].hint.as_deref().unwrap().contains(".arch"));
    }

    #[test]
    fn undeclared_edge_is_reported_at_the_import() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[]),
            &own,
            &imports(&[("a/x.ts", 7, "b/y.ts")]),
        );
        assert_eq!(found.len(), 1);
        let v = &found[0];
        assert_eq!(v.kind, ViolationKind::UndeclaredEdge);
        assert_eq!(v.severity, Severity::Error);
        assert_eq!(v.at, Some(at("a/x.ts", 7)));
        assert_eq!(
            v.message,
            "`a` imports `b` (b/y.ts) but architecture.mmd declares no edge a --> b"
        );
        let hint = v.hint.as_deref().unwrap();
        assert!(hint.contains("approve"), "{hint}");
    }

    #[test]
    fn edge_direction_matters() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("b", "a", EdgeKind::Implemented)]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
        );
        assert_eq!(
            kinds(&found),
            vec![ViolationKind::UndeclaredEdge, ViolationKind::StaleEdge]
        );
    }

    #[test]
    fn solid_realised_edge_is_clean() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Implemented)]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
        );
        assert_eq!(found, Vec::new());
    }

    #[test]
    fn solid_unrealised_edge_is_stale() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Implemented)]),
            &own,
            &BTreeMap::new(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::StaleEdge);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].at, None);
        assert!(found[0].message.contains("a --> b"), "{}", found[0].message);
    }

    #[test]
    fn planned_edge_permits_the_import_and_reports_planned_realized() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Planned)]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::PlannedRealized);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].at, None);
        assert!(found[0].hint.as_deref().unwrap().contains("-->"));
    }

    #[test]
    fn unrealised_planned_edge_is_tolerated() {
        let own = ownership(&[("a/x.ts", "a")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Planned)]),
            &own,
            &BTreeMap::new(),
        );
        assert_eq!(found, Vec::new());
    }

    #[test]
    fn unresolved_import_is_a_warning_naming_the_specifier() {
        let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
        map.entry(PathBuf::from("a/x.ts"))
            .or_default()
            .unresolved
            .push(UnresolvedImport {
                at: at("a/x.ts", 4),
                specifier: "./gone".to_owned(),
            });
        let found = lint(
            &contract(&["a"], &[]),
            &ownership(&[("a/x.ts", "a")], &[]),
            &map,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::UnresolvedImport);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].at, Some(at("a/x.ts", 4)));
        assert!(found[0].message.contains("./gone"));
    }

    #[test]
    fn same_owner_imports_are_ignored() {
        let own = ownership(&[("a/x.ts", "a"), ("a/y.ts", "a")], &[]);
        let found = lint(
            &contract(&["a"], &[]),
            &own,
            &imports(&[("a/x.ts", 1, "a/y.ts")]),
        );
        assert_eq!(found, Vec::new());
    }

    #[test]
    fn unmapped_file_does_not_cascade_into_edge_findings() {
        let own = ownership(&[("b/y.ts", "b")], &["loose/x.ts"]);
        let found = lint(
            &contract(&["b"], &[]),
            &own,
            &imports(&[("loose/x.ts", 1, "b/y.ts"), ("b/y.ts", 2, "loose/x.ts")]),
        );
        assert_eq!(kinds(&found), vec![ViolationKind::UnmappedFile]);
    }

    #[test]
    fn unknown_node_owner_does_not_cascade_into_edge_findings() {
        let mut own = ownership(&[("g/x.ts", "ghost"), ("b/y.ts", "b")], &[]);
        own.tags = vec![(at("g/.arch", 1), "ghost".to_owned())];
        let found = lint(
            &contract(&["b"], &[]),
            &own,
            &imports(&[("g/x.ts", 1, "b/y.ts"), ("b/y.ts", 2, "g/x.ts")]),
        );
        assert_eq!(kinds(&found), vec![ViolationKind::UnknownNode]);
    }

    #[test]
    fn each_import_site_gets_a_finding_but_exact_repeats_collapse() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[]),
            &own,
            &imports(&[
                ("a/x.ts", 1, "b/y.ts"),
                ("a/x.ts", 1, "b/y.ts"),
                ("a/x.ts", 2, "b/y.ts"),
            ]),
        );
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn output_is_sorted_and_independent_of_construction_order() {
        let c = contract(
            &["a", "b", "c"],
            &[
                ("a", "c", EdgeKind::Implemented),
                ("b", "c", EdgeKind::Planned),
            ],
        );
        let owners = [
            ("a/x.ts", "a"),
            ("b/y.ts", "b"),
            ("c/z.ts", "c"),
            ("g/g.ts", "ghost"),
        ];
        let unmapped = ["u/one.ts", "u/two.ts"];
        let tags = [
            (at("g/.arch", 1), "ghost".to_owned()),
            (at("h/.arch", 2), "phantom".to_owned()),
        ];
        let edges = [
            ("a/x.ts", 3, "b/y.ts"),
            ("b/y.ts", 9, "c/z.ts"),
            ("c/z.ts", 5, "a/x.ts"),
            ("b/y.ts", 2, "a/x.ts"),
        ];

        let mut own = ownership(&owners, &unmapped);
        own.tags = tags.to_vec();
        let forward = lint(&c, &own, &imports(&edges));

        let mut reversed_owners = owners;
        reversed_owners.reverse();
        let mut reversed_unmapped = unmapped;
        reversed_unmapped.reverse();
        let mut reversed_edges = edges;
        reversed_edges.reverse();
        let mut own = ownership(&reversed_owners, &reversed_unmapped);
        own.tags = tags.iter().rev().cloned().collect();
        let backward = lint(&c, &own, &imports(&reversed_edges));

        assert_eq!(forward, backward);
        assert!(forward.len() >= 6, "{forward:#?}");
        let mut sorted = forward.clone();
        sorted.sort();
        assert_eq!(forward, sorted);
    }

    #[test]
    fn contract_file_only_names_the_contract_in_messages() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let report = reconcile(
            &contract(&["a", "b"], &[]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
            Path::new("docs/arch.mmd"),
        );
        assert!(report.violations[0].message.contains("docs/arch.mmd"));
    }

    mod surfaces {
        use super::*;
        use crate::lint::fixtures::{imports_using, with_surface};

        const MAIN: &str = "a/x.ts";
        const LIB: &str = "b/y.ts";

        fn owners() -> Ownership {
            ownership(
                &[(MAIN, "a"), ("a/z.ts", "a"), (LIB, "b"), ("c/w.ts", "c")],
                &[],
            )
        }

        fn ab() -> Contract {
            contract(&["a", "b", "c"], &[("a", "b", EdgeKind::Implemented)])
        }

        fn unlinked() -> Contract {
            contract(&["a", "b", "c"], &[])
        }

        fn private(violations: &[Violation]) -> Vec<&Violation> {
            violations
                .iter()
                .filter(|v| v.kind == ViolationKind::PrivateSymbol)
                .collect()
        }

        #[test]
        fn an_exported_symbol_is_clean() {
            let c = with_surface(ab(), "b", &["Client", "fetch"]);
            let used = imports_using(&[(MAIN, 3, LIB, &["Client", "fetch"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn a_symbol_outside_the_surface_is_an_error_at_the_import() {
            let c = with_surface(ab(), "b", &["fetch", "Client"]);
            let found = lint(
                &c,
                &owners(),
                &imports_using(&[(MAIN, 7, LIB, &["secret"])]),
            );
            assert_eq!(found.len(), 1, "{found:?}");
            let v = &found[0];
            assert_eq!(v.kind, ViolationKind::PrivateSymbol);
            assert_eq!(v.severity, Severity::Error);
            assert_eq!(v.at, Some(at(MAIN, 7)));
            assert_eq!(
                v.message,
                "`a` imports `secret` from `b` (b/y.ts) but architecture.mmd lets `b` export only: Client, fetch"
            );
            let hint = v.hint.as_deref().unwrap();
            assert!(
                hint.contains("exports") && hint.contains("approve"),
                "{hint}"
            );
        }

        #[test]
        fn each_private_symbol_is_its_own_finding_and_allowed_ones_are_not() {
            let c = with_surface(ab(), "b", &["ok"]);
            let used = imports_using(&[(MAIN, 2, LIB, &["zed", "ok", "alpha"])]);
            let found = lint(&c, &owners(), &used);
            let names: Vec<&str> = found
                .iter()
                .map(|v| v.message.split('`').nth(3).unwrap())
                .collect();
            assert_eq!(names, ["alpha", "zed"]);
            assert!(found.iter().all(|v| v.at == Some(at(MAIN, 2))));
        }

        #[test]
        fn default_must_be_listed_like_any_name() {
            let used = imports_using(&[(MAIN, 1, LIB, &["default"])]);
            let c = with_surface(ab(), "b", &["Client"]);
            assert_eq!(private(&lint(&c, &owners(), &used)).len(), 1);
            let c = with_surface(ab(), "b", &["default"]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn a_side_effect_import_uses_nothing_and_is_never_private() {
            let c = with_surface(ab(), "b", &["Client"]);
            let found = lint(&c, &owners(), &imports(&[(MAIN, 1, LIB)]));
            assert_eq!(found, Vec::new());
        }

        #[test]
        fn an_embed_uses_no_symbol_so_a_surface_does_not_restrict_it() {
            let c = with_surface(ab(), "b", &["Client"]);
            let mut used = imports(&[(MAIN, 1, LIB)]);
            for file in used.values_mut() {
                for import in &mut file.resolved {
                    import.kind = crate::model::ImportKind::Embed;
                }
            }
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn a_node_without_a_surface_is_unrestricted() {
            let c = with_surface(ab(), "c", &["Only"]);
            let used = imports_using(&[(MAIN, 1, LIB, &["anything", "*"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn the_surface_of_the_importing_node_does_not_restrict_its_own_imports() {
            let c = with_surface(ab(), "a", &["Only"]);
            let used = imports_using(&[(MAIN, 1, LIB, &["anything"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn imports_within_one_node_are_never_checked() {
            let c = with_surface(unlinked(), "a", &["Only"]);
            let used = imports_using(&[(MAIN, 1, "a/z.ts", &["hidden", "*"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn star_is_private_in_every_language() {
            for (file, target) in [
                ("a/x.ts", "b/y.ts"),
                ("a/x.py", "b/y.py"),
                ("a/x.js", "b/y.js"),
                ("a/x.rs", "b/y.rs"),
            ] {
                let own = ownership(&[(file, "a"), (target, "b")], &[]);
                let c = with_surface(ab(), "b", &["Client"]);
                let found = lint(&c, &own, &imports_using(&[(file, 4, target, &["*"])]));
                assert_eq!(found.len(), 1, "{file}: {found:?}");
                assert_eq!(found[0].kind, ViolationKind::PrivateSymbol);
                assert!(
                    found[0].message.contains("imports `*` from `b`"),
                    "{}",
                    found[0].message
                );
                assert_eq!(found[0].at, Some(at(file, 4)));
            }
        }

        #[test]
        fn star_stays_private_even_when_names_from_the_same_target_follow() {
            let c = with_surface(ab(), "b", &["Client"]);
            for (file, target) in [("a/x.ts", "b/y.ts"), ("a/x.rs", "b/y.rs")] {
                let own = ownership(&[(file, "a"), (target, "b")], &[]);
                let used =
                    imports_using(&[(file, 1, target, &["*"]), (file, 2, target, &["Client"])]);
                let found = lint(&c, &own, &used);
                assert_eq!(private(&found).len(), 1, "{file}: {found:?}");
                assert_eq!(found[0].at, Some(at(file, 1)));
            }
        }

        #[test]
        fn a_module_binding_uses_no_symbol_and_each_item_through_it_is_checked_on_its_own() {
            const RS_MAIN: &str = "a/x.rs";
            const RS_LIB: &str = "b/y.rs";
            let own = ownership(&[(RS_MAIN, "a"), (RS_LIB, "b")], &[]);
            let c = with_surface(ab(), "b", &["Client"]);
            let clean =
                imports_using(&[(RS_MAIN, 1, RS_LIB, &[]), (RS_MAIN, 5, RS_LIB, &["Client"])]);
            assert_eq!(lint(&c, &own, &clean), Vec::new());

            let leaky =
                imports_using(&[(RS_MAIN, 1, RS_LIB, &[]), (RS_MAIN, 5, RS_LIB, &["secret"])]);
            let found = lint(&c, &own, &leaky);
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].at, Some(at(RS_MAIN, 5)));
            assert!(
                found[0].message.contains("imports `secret`"),
                "{}",
                found[0].message
            );
        }

        #[test]
        fn unmapped_and_unknown_owners_do_not_cascade_into_private_symbols() {
            let c = with_surface(unlinked(), "b", &["Client"]);
            let mut own = ownership(&[(LIB, "b"), ("g/g.ts", "ghost")], &["loose/u.ts"]);
            own.tags = vec![(at("g/.arch", 1), "ghost".to_owned())];
            let used = imports_using(&[
                ("loose/u.ts", 1, LIB, &["secret"]),
                ("g/g.ts", 2, LIB, &["secret"]),
                (LIB, 3, "loose/u.ts", &["secret"]),
                (LIB, 4, "g/g.ts", &["secret"]),
            ]);
            assert_eq!(
                kinds(&lint(&c, &own, &used)),
                vec![ViolationKind::UnmappedFile, ViolationKind::UnknownNode]
            );
        }

        #[test]
        fn a_private_symbol_is_reported_in_addition_to_an_undeclared_edge() {
            let c = with_surface(contract(&["a", "b"], &[]), "b", &["Client"]);
            let used = imports_using(&[(MAIN, 1, LIB, &["secret"])]);
            assert_eq!(
                kinds(&lint(&c, &owners(), &used)),
                vec![ViolationKind::UndeclaredEdge, ViolationKind::PrivateSymbol]
            );
        }

        #[test]
        fn a_planned_edge_still_enforces_the_surface() {
            let c = with_surface(
                contract(&["a", "b"], &[("a", "b", EdgeKind::Planned)]),
                "b",
                &["Client"],
            );
            let used = imports_using(&[(MAIN, 1, LIB, &["secret"])]);
            assert_eq!(
                kinds(&lint(&c, &owners(), &used)),
                vec![ViolationKind::PlannedRealized, ViolationKind::PrivateSymbol]
            );
        }

        #[test]
        fn exact_repeats_collapse_and_each_site_is_reported() {
            let c = with_surface(ab(), "b", &["Client"]);
            let used = imports_using(&[
                (MAIN, 1, LIB, &["secret"]),
                (MAIN, 1, LIB, &["secret"]),
                (MAIN, 2, LIB, &["secret"]),
            ]);
            assert_eq!(private(&lint(&c, &owners(), &used)).len(), 2);
        }

        #[test]
        fn the_contract_file_names_the_contract_in_the_message() {
            let c = with_surface(ab(), "b", &["Client"]);
            let report = reconcile(
                &c,
                &owners(),
                &imports_using(&[(MAIN, 1, LIB, &["secret"])]),
                Path::new("docs/arch.mmd"),
            );
            assert!(report.violations[0].message.contains("docs/arch.mmd"));
        }
    }
}
