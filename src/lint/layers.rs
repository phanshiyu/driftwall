//! The layer order rule: code in a lower boundary may not depend on a higher one.

use crate::model::{Contract, Edge, EdgeKind, ResolvedImport, Violation, ViolationKind};

use super::rules::site_violation;

/// One `upward-edge` finding per import site that goes from a node in a lower listed boundary to a
/// node in a higher one (see "Layer order" in `docs/DESIGN.md`). Nothing without a `%% order`.
///
/// A test-only site is exempt when the pair is declared with a test edge.
pub(super) fn upward_findings(
    contract: &Contract,
    crossings: &[(Edge, &ResolvedImport)],
) -> Vec<Violation> {
    // why: agreeing chains say the same of a pair, so the first upward chain speaks for all.
    crossings
        .iter()
        .filter_map(|(edge, import)| {
            let (order, from_boundary, to_boundary) =
                contract.rules.order.iter().find_map(|order| {
                    let layer = |node| {
                        let boundary = contract.nodes.get(node)?.boundary.as_deref()?;
                        Some((boundary, order.rank(boundary)?))
                    };
                    let (from_boundary, from_rank) = layer(&edge.from)?;
                    let (to_boundary, to_rank) = layer(&edge.to)?;
                    (from_rank > to_rank).then_some((order, from_boundary, to_boundary))
                })?;
            let exempt = import.test_only
                && contract
                    .covers(edge)
                    .iter()
                    .any(|cover| matches!(cover.kind, EdgeKind::Test | EdgeKind::PlannedTest));
            (!exempt).then(|| {
                site_violation(
                    ViolationKind::UpwardEdge,
                    import,
                    format!(
                        "`{from}` (boundary `{from_boundary}`) imports `{to}` (boundary `{to_boundary}`, {}) but the `%% order` on line {} puts `{to_boundary}` above `{from_boundary}`",
                        import.target.display(),
                        order.line,
                        from = edge.from,
                        to = edge.to,
                    ),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::FileImports;
    use crate::lint::fixtures::{at, edge, node};
    use crate::model::{LayerOrder, NodeInfo, Ownership, Severity};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    /// `top` in `up`, `low` in `down`, `side` in `down`, `free` in no boundary, order up > down.
    fn layered(edges: &[(&str, &str, EdgeKind)]) -> Contract {
        let boundary = |name: Option<&str>| NodeInfo {
            label: None,
            boundary: name.map(str::to_owned),
        };
        let mut contract = Contract::default();
        for (id, boundary_of) in [
            ("top", Some("up")),
            ("low", Some("down")),
            ("side", Some("down")),
            ("free", None),
            ("east", Some("e")),
            ("west", Some("w")),
        ] {
            contract.nodes.insert(node(id), boundary(boundary_of));
        }
        for &(from, to, kind) in edges {
            contract.edges.insert(edge(from, to), kind);
        }
        contract.rules.order = vec![LayerOrder {
            line: 9,
            open: false,
            boundaries: vec!["up".to_owned(), "down".to_owned()],
        }];
        contract
    }

    fn owners() -> Ownership {
        Ownership {
            owners: [
                ("top/a.ts", "top"),
                ("low/a.ts", "low"),
                ("side/a.ts", "side"),
                ("free/a.ts", "free"),
                ("east/a.ts", "east"),
                ("west/a.ts", "west"),
            ]
            .into_iter()
            .map(|(file, owner)| (PathBuf::from(file), node(owner)))
            .collect(),
            ..Ownership::default()
        }
    }

    /// The findings of one import of `to` from `from`, test-only or not.
    fn found(contract: &Contract, from: &str, to: &str, test_only: bool) -> Vec<Violation> {
        let import = ResolvedImport::new(
            at(&format!("{from}/a.ts"), 3),
            PathBuf::from(format!("{to}/a.ts")),
        )
        .with_test_only(test_only);
        let imports = BTreeMap::from([(
            PathBuf::from(format!("{from}/a.ts")),
            FileImports {
                resolved: vec![import],
                ..FileImports::default()
            },
        )]);
        let report = crate::lint::reconcile(contract, &owners(), &imports, Path::new("c.mmd"));
        report
            .violations
            .into_iter()
            .filter(|v| v.kind == ViolationKind::UpwardEdge)
            .collect()
    }

    #[test]
    fn a_lower_boundary_importing_a_higher_one_is_an_error_at_the_site() {
        let contract = layered(&[]);
        let found = found(&contract, "low", "top", false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].at, Some(at("low/a.ts", 3)));
        let message = &found[0].message;
        assert!(
            message.contains("`low` (boundary `down`) imports `top` (boundary `up`"),
            "{message}"
        );
        assert!(message.contains("line 9"), "{message}");
    }

    #[test]
    fn a_higher_boundary_importing_a_lower_one_is_fine() {
        assert_eq!(found(&layered(&[]), "top", "low", false), Vec::new());
    }

    #[test]
    fn the_same_boundary_and_unlisted_nodes_are_unconstrained() {
        let contract = layered(&[]);
        assert_eq!(found(&contract, "low", "side", false), Vec::new());
        assert_eq!(found(&contract, "free", "top", false), Vec::new());
        assert_eq!(found(&contract, "low", "free", false), Vec::new());
    }

    #[test]
    fn an_arrow_does_not_excuse_an_upward_import() {
        // why: the parser rejects such an arrow, but the pure core must not trust the contract.
        let contract = layered(&[("low", "top", EdgeKind::Implemented)]);
        assert_eq!(found(&contract, "low", "top", false).len(), 1);
    }

    #[test]
    fn a_test_site_is_exempt_only_on_a_test_edge() {
        let with_test = layered(&[("low", "top", EdgeKind::Test)]);
        assert_eq!(found(&with_test, "low", "top", true), Vec::new());
        assert_eq!(found(&with_test, "low", "top", false).len(), 1);
        let planned = layered(&[("low", "top", EdgeKind::PlannedTest)]);
        assert_eq!(found(&planned, "low", "top", true), Vec::new());
        assert_eq!(found(&layered(&[]), "low", "top", true).len(), 1);
    }

    #[test]
    fn each_chain_constrains_only_the_boundaries_it_lists() {
        let mut contract = layered(&[]);
        contract.rules.order.push(LayerOrder {
            line: 10,
            open: false,
            boundaries: vec!["e".to_owned(), "w".to_owned()],
        });
        let upward = found(&contract, "west", "east", false);
        assert_eq!(upward.len(), 1);
        assert!(
            upward[0].message.contains("line 10"),
            "{}",
            upward[0].message
        );
        assert_eq!(found(&contract, "east", "west", false), Vec::new());
        // why: no chain lists `down` and `e` together.
        assert_eq!(found(&contract, "low", "east", false), Vec::new());
        assert_eq!(found(&contract, "east", "low", false), Vec::new());
        assert_eq!(found(&contract, "low", "top", false).len(), 1);
    }

    #[test]
    fn a_site_upward_in_two_agreeing_chains_is_one_finding() {
        let mut contract = layered(&[]);
        contract.rules.order.push(LayerOrder {
            line: 10,
            open: false,
            boundaries: vec!["up".to_owned(), "down".to_owned()],
        });
        assert_eq!(found(&contract, "low", "top", false).len(), 1);
    }

    #[test]
    fn no_order_means_no_findings() {
        let mut contract = layered(&[]);
        contract.rules.order.clear();
        assert_eq!(found(&contract, "low", "top", false), Vec::new());
    }
}
