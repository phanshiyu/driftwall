//! Boundary edges: edges with a boundary (`subgraph`) at one end, see "Boundary edges" in
//! `docs/DESIGN.md`.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Serialize, Serializer};

use super::{Contract, Edge, EdgeKind, NodeId, NodeInfo};

/// One end of an edge: a node, or every node of a boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endpoint {
    /// A single node.
    Node(NodeId),
    /// A boundary: every node declared inside its `subgraph`.
    Boundary(String),
}

impl Endpoint {
    /// The id as written in the contract.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Node(id) => id.as_str(),
            Self::Boundary(id) => id,
        }
    }

    /// The nodes this end stands for, in id order.
    #[must_use]
    pub fn members<'a>(&self, nodes: &'a BTreeMap<NodeId, NodeInfo>) -> Vec<&'a NodeId> {
        nodes
            .iter()
            .filter(|(id, _)| self.contains(nodes, id))
            .map(|(id, _)| id)
            .collect()
    }

    /// Whether `node` is one of the nodes this end stands for.
    #[must_use]
    pub fn contains(&self, nodes: &BTreeMap<NodeId, NodeInfo>, node: &NodeId) -> bool {
        match self {
            Self::Node(id) => id == node,
            Self::Boundary(boundary) => nodes.get(node).is_some_and(|info| info.within(boundary)),
        }
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Endpoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The child of `boundary` that `node` is in, or `None` when it is not a member of `boundary`.
///
/// The child is the node itself when it is written directly in `boundary`, else the boundary
/// written directly in `boundary` that holds it. See "Isolated boundaries" in `docs/DESIGN.md`.
#[must_use]
pub fn child_of<'a>(node: &'a NodeId, info: &'a NodeInfo, boundary: &str) -> Option<&'a str> {
    let mut chain = info.boundaries().skip_while(|id| *id != boundary);
    chain.next()?;
    Some(chain.next().unwrap_or_else(|| node.as_str()))
}

/// The children of `boundary` that hold something: the ids of its direct child boundaries that
/// have a node at some depth, and of the nodes written directly in it. Sorted.
#[must_use]
pub fn children(nodes: &BTreeMap<NodeId, NodeInfo>, boundary: &str) -> Vec<String> {
    let mut found: Vec<String> = nodes
        .iter()
        .filter_map(|(id, info)| child_of(id, info, boundary))
        .map(str::to_owned)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The two different children of `boundary` that `a` and `b` are in, when both are members of
/// `boundary` and in different children.
#[must_use]
pub fn children_apart<'a>(
    nodes: &'a BTreeMap<NodeId, NodeInfo>,
    boundary: &str,
    a: &'a NodeId,
    b: &'a NodeId,
) -> Option<(&'a str, &'a str)> {
    let of = |id| child_of(id, nodes.get(id)?, boundary);
    let (first, second) = (of(a)?, of(b)?);
    (first != second).then_some((first, second))
}

/// The two ends of an edge that has a boundary at one of them: what identifies it in a diff.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct EdgeEnds {
    /// The importing end.
    pub from: Endpoint,
    /// The imported end.
    pub to: Endpoint,
}

/// An edge with a boundary at one end or both: shorthand for the node edges of every pair it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryEdge {
    /// The importing end.
    pub from: Endpoint,
    /// The imported end.
    pub to: Endpoint,
    /// What the arrow and its label say.
    pub kind: EdgeKind,
    /// The line of the edge in the contract.
    pub line: u32,
}

impl BoundaryEdge {
    /// What identifies the edge: its two ends.
    #[must_use]
    pub fn ends(&self) -> EdgeEnds {
        EdgeEnds {
            from: self.from.clone(),
            to: self.to.clone(),
        }
    }

    /// The edge as written, `features --> core`, label included.
    #[must_use]
    pub fn text(&self) -> String {
        format!("{} {} {}", self.from, self.kind.arrow(), self.to)
    }

    /// Whether the edge covers the pair `edge`: its source is on the importing side, its target on
    /// the imported side, and the two nodes differ.
    #[must_use]
    pub fn covers(&self, nodes: &BTreeMap<NodeId, NodeInfo>, edge: &Edge) -> bool {
        edge.from != edge.to
            && self.from.contains(nodes, &edge.from)
            && self.to.contains(nodes, &edge.to)
    }

    /// Every pair the edge covers, sorted.
    #[must_use]
    pub fn pairs(&self, nodes: &BTreeMap<NodeId, NodeInfo>) -> Vec<Edge> {
        let targets = self.to.members(nodes);
        self.from
            .members(nodes)
            .into_iter()
            .flat_map(|from| {
                targets
                    .iter()
                    .filter(move |to| **to != from)
                    .map(move |to| Edge {
                        from: from.clone(),
                        to: (*to).clone(),
                    })
            })
            .collect()
    }
}

/// Where a covering edge was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declared<'a> {
    /// A node edge between the two nodes of the pair.
    Node,
    /// A boundary edge.
    Boundary(&'a BoundaryEdge),
}

/// One edge that covers a pair: its kind and where it was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cover<'a> {
    /// What the edge allows.
    pub kind: EdgeKind,
    /// The node edge or the boundary edge.
    pub by: Declared<'a>,
}

impl Cover<'_> {
    /// The position in the precedence order, smaller wins: unrestricted, `type, test`, `type`,
    /// `test`; then solid before dotted, a node edge before a boundary edge, the lower line.
    const fn key(&self) -> (u8, bool, u8, u32) {
        let restriction = match (self.kind.is_type(), self.kind.is_test()) {
            (false, false) => 0,
            (true, true) => 1,
            (true, false) => 2,
            (false, true) => 3,
        };
        let (boundary, line) = match self.by {
            Declared::Node => (0, 0),
            Declared::Boundary(edge) => (1, edge.line),
        };
        (restriction, self.kind.is_planned(), boundary, line)
    }
}

impl Contract {
    /// Every edge that covers the pair `edge`, the node edge first when there is one and then the
    /// boundary edges, sorted by the precedence of "Boundary edges": the first is the winner.
    #[must_use]
    pub fn covers(&self, edge: &Edge) -> Vec<Cover<'_>> {
        let node = self.edges.get(edge).map(|kind| Cover {
            kind: *kind,
            by: Declared::Node,
        });
        let mut covers: Vec<Cover<'_>> = node
            .into_iter()
            .chain(
                self.boundary_edges
                    .iter()
                    .filter(|declared| declared.covers(&self.nodes, edge))
                    .map(|declared| Cover {
                        kind: declared.kind,
                        by: Declared::Boundary(declared),
                    }),
            )
            .collect();
        covers.sort_by_key(Cover::key);
        covers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    /// `f1`, `f2`, `f3` in `features`, `c1`, `c2` in `core`, `loose` in none.
    fn contract(boundary_edges: Vec<BoundaryEdge>) -> Contract {
        let info = |boundary: Option<&str>| NodeInfo {
            label: None,
            boundary: boundary.map(str::to_owned),
            outer: Vec::new(),
        };
        let mut contract = Contract::default();
        for (id, boundary) in [
            ("f1", Some("features")),
            ("f2", Some("features")),
            ("f3", Some("features")),
            ("c1", Some("core")),
            ("c2", Some("core")),
            ("loose", None),
        ] {
            contract.nodes.insert(node(id), info(boundary));
        }
        contract.boundary_edges = boundary_edges;
        contract
    }

    fn declared(from: Endpoint, to: Endpoint, kind: EdgeKind, line: u32) -> BoundaryEdge {
        BoundaryEdge {
            from,
            to,
            kind,
            line,
        }
    }

    fn sub(id: &str) -> Endpoint {
        Endpoint::Boundary(id.to_owned())
    }

    fn pair(from: &str, to: &str) -> Edge {
        Edge {
            from: node(from),
            to: node(to),
        }
    }

    #[test]
    fn a_boundary_to_a_boundary_covers_every_member_pair() {
        let contract = contract(vec![declared(
            sub("features"),
            sub("core"),
            EdgeKind::Implemented,
            5,
        )]);
        assert_eq!(contract.boundary_edges[0].pairs(&contract.nodes).len(), 6);
        assert_eq!(contract.covers(&pair("f2", "c1")).len(), 1);
        assert_eq!(contract.covers(&pair("c1", "f2")), Vec::new());
        assert_eq!(contract.covers(&pair("f1", "loose")), Vec::new());
    }

    #[test]
    fn a_boundary_to_itself_covers_distinct_members_only() {
        let contract = contract(vec![declared(
            sub("features"),
            sub("features"),
            EdgeKind::Implemented,
            5,
        )]);
        assert_eq!(contract.boundary_edges[0].pairs(&contract.nodes).len(), 6);
        assert_eq!(contract.covers(&pair("f1", "f2")).len(), 1);
        assert_eq!(contract.covers(&pair("f1", "f1")), Vec::new());
        assert_eq!(contract.covers(&pair("f1", "c1")), Vec::new());
    }

    #[test]
    fn a_node_end_covers_one_side_of_the_pairs() {
        let contract = contract(vec![
            declared(
                Endpoint::Node(node("loose")),
                sub("core"),
                EdgeKind::Implemented,
                5,
            ),
            declared(
                sub("core"),
                Endpoint::Node(node("loose")),
                EdgeKind::Planned,
                6,
            ),
            declared(
                Endpoint::Node(node("f1")),
                sub("features"),
                EdgeKind::Implemented,
                7,
            ),
        ]);
        assert_eq!(contract.covers(&pair("loose", "c2")).len(), 1);
        assert_eq!(contract.covers(&pair("c2", "loose")).len(), 1);
        assert_eq!(contract.covers(&pair("f1", "f3")).len(), 1);
        assert_eq!(contract.covers(&pair("f2", "f1")), Vec::new());
        assert_eq!(contract.boundary_edges[2].pairs(&contract.nodes).len(), 2);
    }

    #[test]
    fn the_most_permissive_cover_wins_then_solid_then_node_then_line() {
        let mut contract = contract(vec![
            declared(sub("features"), sub("core"), EdgeKind::Test, 4),
            declared(sub("features"), sub("core"), EdgeKind::Type, 5),
            declared(
                Endpoint::Node(node("f1")),
                sub("core"),
                EdgeKind::TypeTest,
                6,
            ),
            declared(
                Endpoint::Node(node("f1")),
                Endpoint::Node(node("c1")),
                EdgeKind::Planned,
                7,
            ),
        ]);
        let order = |contract: &Contract| -> Vec<EdgeKind> {
            contract
                .covers(&pair("f1", "c1"))
                .iter()
                .map(|cover| cover.kind)
                .collect()
        };
        assert_eq!(
            order(&contract),
            vec![
                EdgeKind::Planned,
                EdgeKind::TypeTest,
                EdgeKind::Type,
                EdgeKind::Test
            ]
        );
        // why: a solid node edge beats a dotted cover of the same restriction.
        contract
            .edges
            .insert(pair("f1", "c1"), EdgeKind::Implemented);
        let covers = contract.covers(&pair("f1", "c1"));
        assert_eq!(covers[0].by, Declared::Node);
        assert_eq!(covers[0].kind, EdgeKind::Implemented);
        assert_eq!(covers.len(), 5);
    }

    #[test]
    fn a_node_edge_beats_a_boundary_edge_of_equal_rank() {
        let mut contract = contract(vec![declared(
            sub("features"),
            sub("core"),
            EdgeKind::Implemented,
            3,
        )]);
        contract
            .edges
            .insert(pair("f1", "c1"), EdgeKind::Implemented);
        assert_eq!(contract.covers(&pair("f1", "c1"))[0].by, Declared::Node);
        assert_eq!(
            contract.covers(&pair("f2", "c1"))[0].by,
            Declared::Boundary(&contract.boundary_edges[0])
        );
    }

    #[test]
    fn text_names_the_arrow_and_the_ends() {
        let edge = declared(sub("features"), sub("core"), EdgeKind::TypeTest, 3);
        assert_eq!(edge.text(), "features -->|type, test| core");
        assert_eq!(edge.ends().to.to_string(), "core");
    }
}
