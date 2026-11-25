//! The implicit allowance of an open layer order, see "Open order" in `docs/DESIGN.md`.

use std::collections::BTreeSet;

use super::{Contract, Edge, Endpoint, LayerOrder, NodeId, Template};

/// What the open chains say about a dependency between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAccess<'a> {
    /// The chain allows the pair implicitly, as a solid edge would.
    Allowed(&'a LayerOrder),
    /// The chain lists the importer's boundary above the target's, but the contract declares edges
    /// between the two boundaries that apply to this pair, so only those do and the pair is not
    /// among them.
    Narrowed {
        /// The chain.
        order: &'a LayerOrder,
        /// The importer's boundary.
        from: &'a str,
        /// The imported boundary.
        to: &'a str,
        /// How many declared edges narrow this pair.
        arrows: usize,
        /// Set when only `%% each` expansions narrow the pair: the template that speaks.
        each: Option<EachLimit<'a>>,
    },
}

/// The `%% each` line that narrows a pair, and the names its message uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EachLimit<'a> {
    /// The boundary the template names.
    pub boundary: &'a str,
    /// The line of the template.
    pub line: u32,
    /// What is limited: the child on the importing role side, else the importer's boundary.
    pub from: &'a str,
    /// What it is limited towards: the child on the imported role side, else the target boundary.
    pub to: &'a str,
}

/// Whether narrowing by `%% each` expansions reads the code.
#[derive(Debug, Clone, Copy)]
pub enum Narrowing<'a> {
    /// The design as drawn: every expansion counts as if its role nodes own files.
    Design,
    /// Lint's view: an expansion narrows only when its role nodes are in this set of nodes that own
    /// at least one file.
    Owned(&'a BTreeSet<NodeId>),
}

impl Contract {
    /// How many declared edges narrow the pair `edge` of the layers `upper` above `lower`, and the
    /// first expansion among them when no written edge narrows it. A written edge, node or
    /// boundary, narrows the whole pair of layers. An expansion narrows only a pair with an end
    /// inside its child on the role side, and, under [`Narrowing::Owned`], only when its role
    /// nodes own a file.
    fn narrowing_arrows<'a>(
        &'a self,
        upper: &'a str,
        lower: &'a str,
        edge: &Edge,
        narrowing: Narrowing<'_>,
    ) -> (usize, Option<EachLimit<'a>>) {
        // why: the same member lookup as boundary edges, so a later change to membership applies here.
        let from = Endpoint::Boundary(upper.to_owned());
        let to = Endpoint::Boundary(lower.to_owned());
        let runs = |pair: &Edge| {
            from.contains(&self.nodes, &pair.from) && to.contains(&self.nodes, &pair.to)
        };
        let mut written = self.edges.keys().filter(|pair| runs(pair)).count();
        let mut expansions = 0;
        let mut first = None;
        for declared in &self.boundary_edges {
            let pairs = declared.pairs(&self.nodes);
            let Some(template) = &declared.template else {
                written += usize::from(pairs.iter().any(runs));
                continue;
            };
            let narrows = pairs
                .iter()
                .any(|pair| runs(pair) && self.expansion_narrows(template, pair, edge, narrowing));
            if narrows {
                expansions += 1;
                first.get_or_insert(EachLimit {
                    boundary: &template.boundary,
                    line: declared.line,
                    from: if template.from_role {
                        &template.child
                    } else {
                        upper
                    },
                    to: if template.to_role {
                        &template.child
                    } else {
                        lower
                    },
                });
            }
        }
        (
            written + expansions,
            if written == 0 { first } else { None },
        )
    }

    /// Whether the expansion `template`, which declares `declared`, narrows the pair `edge`.
    fn expansion_narrows(
        &self,
        template: &Template,
        declared: &Edge,
        edge: &Edge,
        narrowing: Narrowing<'_>,
    ) -> bool {
        let inside = |node: &NodeId| {
            self.nodes
                .get(node)
                .is_some_and(|info| info.within(&template.child))
        };
        let near =
            (template.from_role && inside(&edge.from)) || (template.to_role && inside(&edge.to));
        near && match narrowing {
            Narrowing::Design => true,
            Narrowing::Owned(owned) => {
                (!template.from_role || owned.contains(&declared.from))
                    && (!template.to_role || owned.contains(&declared.to))
            }
        }
    }

    /// What the `%% order open:` chains say about the dependency `edge`, or `None` when no open
    /// chain lists the importer's boundary above the target's. The first chain in file order that
    /// allows the pair speaks.
    #[must_use]
    pub fn open_access(&self, edge: &Edge, narrowing: Narrowing<'_>) -> Option<OpenAccess<'_>> {
        let (from_info, to_info) = (self.nodes.get(&edge.from)?, self.nodes.get(&edge.to)?);
        let mut narrowed = None;
        for order in self.rules.order.iter().filter(|order| order.open) {
            // why: a node counts as in the boundary the chain lists around it, at any depth.
            let (Some((from, upper)), Some((to, lower))) =
                (order.place(from_info), order.place(to_info))
            else {
                continue;
            };
            if upper >= lower {
                continue;
            }
            let (arrows, each) = self.narrowing_arrows(from, to, edge, narrowing);
            if arrows == 0 {
                return Some(OpenAccess::Allowed(order));
            }
            narrowed.get_or_insert(OpenAccess::Narrowed {
                order,
                from,
                to,
                arrows,
                each,
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
                    outer: Vec::new(),
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
            contract.open_access(&pair("a1", "c1"), Narrowing::Design),
            Some(OpenAccess::Allowed(order))
        );
        assert_eq!(
            contract.open_access(&pair("f1", "s1"), Narrowing::Design),
            Some(OpenAccess::Allowed(order))
        );
    }

    #[test]
    fn upward_same_boundary_and_unlisted_pairs_are_not_covered() {
        let contract = contract();
        assert_eq!(
            contract.open_access(&pair("c1", "a1"), Narrowing::Design),
            None
        );
        assert_eq!(
            contract.open_access(&pair("a1", "a2"), Narrowing::Design),
            None
        );
        assert_eq!(
            contract.open_access(&pair("x", "c1"), Narrowing::Design),
            None
        );
        assert_eq!(
            contract.open_access(&pair("a1", "x"), Narrowing::Design),
            None
        );
    }

    /// `o1` in `orders` inside `features`, which the chain lists.
    fn nested(contract: &mut Contract) {
        contract.nodes.insert(
            node("o1"),
            NodeInfo {
                label: None,
                boundary: Some("orders".to_owned()),
                outer: vec!["features".to_owned()],
            },
        );
    }

    #[test]
    fn a_node_in_a_boundary_inside_a_listed_one_counts_as_in_the_listed_one() {
        let mut contract = contract();
        nested(&mut contract);
        let order = &contract.rules.order[0];
        assert_eq!(
            contract.open_access(&pair("o1", "c1"), Narrowing::Design),
            Some(OpenAccess::Allowed(order))
        );
        assert_eq!(
            contract.open_access(&pair("a1", "o1"), Narrowing::Design),
            Some(OpenAccess::Allowed(order))
        );
        assert_eq!(
            contract.open_access(&pair("o1", "a1"), Narrowing::Design),
            None
        );
        assert_eq!(
            contract.open_access(&pair("o1", "f1"), Narrowing::Design),
            None
        );
    }

    #[test]
    fn an_arrow_from_a_nested_node_narrows_the_listed_layers() {
        let mut contract = contract();
        nested(&mut contract);
        contract
            .edges
            .insert(pair("o1", "c2"), EdgeKind::Implemented);
        let order = &contract.rules.order[0];
        assert_eq!(
            contract.open_access(&pair("f1", "c1"), Narrowing::Design),
            Some(OpenAccess::Narrowed {
                order,
                from: "features",
                to: "core",
                arrows: 1,
                each: None
            })
        );
    }

    #[test]
    fn a_plain_chain_grants_nothing() {
        let mut contract = contract();
        contract.rules.order[0].open = false;
        assert_eq!(
            contract.open_access(&pair("a1", "c1"), Narrowing::Design),
            None
        );
    }

    #[test]
    fn any_arrow_between_two_layers_narrows_that_pair_of_layers_only() {
        let mut contract = contract();
        contract.edges.insert(pair("f1", "c1"), EdgeKind::Test);
        assert!(matches!(
            contract.open_access(&pair("f1", "c2"), Narrowing::Design),
            Some(OpenAccess::Narrowed {
                from: "features",
                to: "core",
                arrows: 1,
                ..
            })
        ));
        assert!(matches!(
            contract.open_access(&pair("f1", "s1"), Narrowing::Design),
            Some(OpenAccess::Allowed(_))
        ));
        assert!(matches!(
            contract.open_access(&pair("a1", "c1"), Narrowing::Design),
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
            template: None,
        });
        assert!(matches!(
            contract.open_access(&pair("a2", "c1"), Narrowing::Design),
            Some(OpenAccess::Narrowed { arrows: 1, .. })
        ));
        assert!(matches!(
            contract.open_access(&pair("a2", "s1"), Narrowing::Design),
            Some(OpenAccess::Allowed(_))
        ));
    }
}
