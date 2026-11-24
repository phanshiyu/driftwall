//! The implicit allowance of an open layer order, see "Open order" in `docs/DESIGN.md`.

use super::{Contract, Edge, Endpoint, LayerOrder};

/// What the open chains say about a dependency between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAccess<'a> {
    /// The chain allows the pair implicitly, as a solid edge would.
    Allowed(&'a LayerOrder),
    /// The chain lists the importer's boundary above the target's, but the contract declares edges
    /// between the two boundaries, so only those apply and the pair is not among them.
    Narrowed {
        /// The chain.
        order: &'a LayerOrder,
        /// The importer's boundary.
        from: &'a str,
        /// The imported boundary.
        to: &'a str,
        /// How many declared edges run from inside `from` to inside `to`.
        arrows: usize,
    },
}

impl Contract {
    /// How many declared edges, node edges and boundary edges of any kind, run from inside the
    /// boundary `upper` to inside the boundary `lower`.
    #[must_use]
    pub fn arrows_between(&self, upper: &str, lower: &str) -> usize {
        // why: the same member lookup as boundary edges, so a later change to membership applies here.
        let from = Endpoint::Boundary(upper.to_owned());
        let to = Endpoint::Boundary(lower.to_owned());
        let runs = |pair: &Edge| {
            from.contains(&self.nodes, &pair.from) && to.contains(&self.nodes, &pair.to)
        };
        let nodes = self.edges.keys().filter(|edge| runs(edge)).count();
        let boundaries = self
            .boundary_edges
            .iter()
            .filter(|declared| declared.pairs(&self.nodes).iter().any(runs))
            .count();
        nodes + boundaries
    }

    /// What the `%% order open:` chains say about the dependency `edge`, or `None` when no open
    /// chain lists the importer's boundary above the target's. The first chain in file order that
    /// allows the pair speaks; narrowing is a property of the two boundaries, so chains agree on it.
    #[must_use]
    pub fn open_access(&self, edge: &Edge) -> Option<OpenAccess<'_>> {
        let boundary = |node| self.nodes.get(node)?.boundary.as_deref();
        let (from, to) = (boundary(&edge.from)?, boundary(&edge.to)?);
        let mut narrowed = None;
        for order in self.rules.order.iter().filter(|order| order.open) {
            let (Some(upper), Some(lower)) = (order.rank(from), order.rank(to)) else {
                continue;
            };
            if upper >= lower {
                continue;
            }
            let arrows = self.arrows_between(from, to);
            if arrows == 0 {
                return Some(OpenAccess::Allowed(order));
            }
            narrowed.get_or_insert(OpenAccess::Narrowed {
                order,
                from,
                to,
                arrows,
            });
        }
        narrowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BoundaryEdge, EdgeKind, NodeId, NodeInfo};

    fn node(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn pair(from: &str, to: &str) -> Edge {
        Edge {
            from: node(from),
            to: node(to),
        }
    }

    /// `a1`, `a2` in `app`, `f1` in `features`, `s1` in `shared`, `c1`, `c2` in `core`, `x` in no
    /// boundary; one open chain app > features > shared > core on line 3.
    fn contract() -> Contract {
        let mut contract = Contract::default();
        for (id, boundary) in [
            ("a1", Some("app")),
            ("a2", Some("app")),
            ("f1", Some("features")),
            ("s1", Some("shared")),
            ("c1", Some("core")),
            ("c2", Some("core")),
            ("x", None),
        ] {
            contract.nodes.insert(
                node(id),
                NodeInfo {
                    label: None,
                    boundary: boundary.map(str::to_owned),
                },
            );
        }
        contract.rules.order = vec![LayerOrder {
            line: 3,
            open: true,
            boundaries: ["app", "features", "shared", "core"]
                .map(str::to_owned)
                .to_vec(),
        }];
        contract
    }

    #[test]
    fn a_downward_pair_with_no_arrow_is_allowed_across_layers() {
        let contract = contract();
        let order = &contract.rules.order[0];
        assert_eq!(
            contract.open_access(&pair("a1", "c1")),
            Some(OpenAccess::Allowed(order))
        );
        assert_eq!(
            contract.open_access(&pair("f1", "s1")),
            Some(OpenAccess::Allowed(order))
        );
    }

    #[test]
    fn upward_same_boundary_and_unlisted_pairs_are_not_covered() {
        let contract = contract();
        assert_eq!(contract.open_access(&pair("c1", "a1")), None);
        assert_eq!(contract.open_access(&pair("a1", "a2")), None);
        assert_eq!(contract.open_access(&pair("x", "c1")), None);
        assert_eq!(contract.open_access(&pair("a1", "x")), None);
    }

    #[test]
    fn a_plain_chain_grants_nothing() {
        let mut contract = contract();
        contract.rules.order[0].open = false;
        assert_eq!(contract.open_access(&pair("a1", "c1")), None);
    }

    #[test]
    fn any_arrow_between_two_layers_narrows_that_pair_of_layers_only() {
        let mut contract = contract();
        contract.edges.insert(pair("f1", "c1"), EdgeKind::Test);
        assert!(matches!(
            contract.open_access(&pair("f1", "c2")),
            Some(OpenAccess::Narrowed {
                from: "features",
                to: "core",
                arrows: 1,
                ..
            })
        ));
        assert!(matches!(
            contract.open_access(&pair("f1", "s1")),
            Some(OpenAccess::Allowed(_))
        ));
        assert!(matches!(
            contract.open_access(&pair("a1", "c1")),
            Some(OpenAccess::Allowed(_))
        ));
    }

    #[test]
    fn a_boundary_edge_narrows_through_its_pairs() {
        let mut contract = contract();
        contract.boundary_edges.push(BoundaryEdge {
            from: Endpoint::Node(node("a1")),
            to: Endpoint::Boundary("core".to_owned()),
            kind: EdgeKind::Planned,
            line: 9,
        });
        assert_eq!(contract.arrows_between("app", "core"), 1);
        assert!(matches!(
            contract.open_access(&pair("a2", "c1")),
            Some(OpenAccess::Narrowed { arrows: 1, .. })
        ));
        assert_eq!(contract.arrows_between("app", "shared"), 0);
    }
}
