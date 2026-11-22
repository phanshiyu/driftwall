//! The pure core of the linter: compares three in-memory views and reports disagreements.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::imports::FileImports;
use crate::model::{Contract, Edge, EdgeKind, LintReport, Location, Ownership, ViolationKind};

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
            violations.push(violation(
                ViolationKind::UnknownNode,
                Some(at.clone()),
                format!(
                    "`.arch` tags `{name}` but {} declares no such node",
                    contract_file.display()
                ),
            ));
        }
    }

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
        if !contract.edges.contains_key(edge) {
            violations.push(violation(
                ViolationKind::UndeclaredEdge,
                Some(import.at.clone()),
                format!(
                    "`{from}` imports `{to}` ({}) but {} declares no edge {from} --> {to}",
                    import.target.display(),
                    contract_file.display(),
                    from = edge.from,
                    to = edge.to,
                ),
            ));
        }
    }
    let realised: BTreeSet<&Edge> = crossings.iter().map(|(edge, _)| edge).collect();

    for file_imports in imports.values() {
        for import in &file_imports.unresolved {
            violations.push(violation(
                ViolationKind::UnresolvedImport,
                Some(import.at.clone()),
                format!("import `{}` resolves to no file", import.specifier),
            ));
        }
    }

    for (edge, kind) in &contract.edges {
        let used = realised.contains(&edge);
        match (kind, used) {
            (EdgeKind::Implemented, false) => violations.push(violation(
                ViolationKind::StaleEdge,
                None,
                format!(
                    "edge {} --> {} is declared but no import realises it",
                    edge.from, edge.to
                ),
            )),
            (EdgeKind::Planned, true) => violations.push(violation(
                ViolationKind::PlannedRealized,
                None,
                format!(
                    "planned edge {} -.-> {} is now realised by code",
                    edge.from, edge.to
                ),
            )),
            (EdgeKind::Implemented, true) | (EdgeKind::Planned, false) => {}
        }
    }

    violations.sort();
    violations.dedup();
    LintReport { violations }
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
        }
    }

    fn ownership(owners: &[(&str, &str)], unmapped: &[&str]) -> Ownership {
        Ownership {
            owners: owners
                .iter()
                .map(|&(file, owner)| (PathBuf::from(file), node(owner)))
                .collect(),
            unmapped: unmapped.iter().map(PathBuf::from).collect(),
            tags: Vec::new(),
        }
    }

    /// Builds imports from (importing file, line, target file) triples.
    fn imports(resolved: &[(&str, u32, &str)]) -> BTreeMap<PathBuf, FileImports> {
        let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
        for &(file, line, target) in resolved {
            map.entry(PathBuf::from(file))
                .or_default()
                .resolved
                .push(ResolvedImport {
                    at: at(file, line),
                    target: PathBuf::from(target),
                });
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
}
