//! Boundary edges in the views: the overview names the winning boundary edge per pair and counts
//! the realised pairs, and the contract diff reports boundary edges by their ends.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::fixtures::{at, node};
use super::{contract_diff, overview};
use crate::diagram;
use crate::model::{
    BlindSpots, Contract, Edge, EdgeKind, ImportKind, LintReport, Overview, Ownership,
    ResolvedImport, Site,
};

/// Two nodes in each of `ga` and `gb`.
fn double(arrows: &str) -> Contract {
    diagram::parse(&format!(
        "flowchart LR\n  subgraph ga[\"ga\"]\n    a1\n    a2\n  end\n  subgraph gb[\"gb\"]\n    b1\n    b2\n  end\n{arrows}"
    ))
    .unwrap()
}

/// One import per `(from file, line, to file, kind, test_only)`.
type Imports<'a> = &'a [(&'a str, u32, &'a str, ImportKind, bool)];

fn view(contract: &Contract, owned: &[(&str, &str)], imports: Imports<'_>) -> Overview {
    let ownership = Ownership {
        owners: owned
            .iter()
            .map(|&(file, owner)| (PathBuf::from(file), node(owner)))
            .collect(),
        ..Ownership::default()
    };
    let node_of = |file: &str| ownership.owners.get(&PathBuf::from(file)).unwrap().clone();
    let mut observed: BTreeMap<Edge, Vec<Site>> = BTreeMap::new();
    for &(from, line, to, kind, test_only) in imports {
        let import = ResolvedImport::new(at(from, line), PathBuf::from(to))
            .with_kind(kind)
            .with_test_only(test_only);
        let pair = Edge {
            from: node_of(from),
            to: node_of(to),
        };
        observed.entry(pair).or_default().push(Site::of(&import));
    }
    overview(
        contract,
        &ownership,
        &observed,
        &BTreeMap::new(),
        &LintReport::default(),
        &BlindSpots::default(),
    )
}

#[test]
fn the_overview_names_the_winning_boundary_edge_per_pair_and_counts_realised_pairs() {
    let contract = double("  ga -->|test| gb\n  a1 --> b1\n  ga -.-> b2\n");
    let owned = [
        ("a1/x.ts", "a1"),
        ("a2/x.ts", "a2"),
        ("b1/y.ts", "b1"),
        ("b2/y.ts", "b2"),
    ];
    let imports = [
        ("a1/x.ts", 1, "b1/y.ts", ImportKind::Runtime, false),
        ("a1/x.ts", 2, "b2/y.ts", ImportKind::Runtime, false),
        ("a2/x.ts", 3, "b2/y.ts", ImportKind::Runtime, true),
    ];
    let overview = view(&contract, &owned, &imports);
    let by_pair = |from: &str, to: &str| {
        overview
            .edges
            .iter()
            .find(|e| e.from.as_str() == from && e.to.as_str() == to)
            .unwrap()
    };
    // why: the solid node edge beats the test boundary edge, so there is nothing to name.
    assert_eq!(by_pair("a1", "b1").kind, Some(EdgeKind::Implemented));
    assert_eq!(by_pair("a1", "b1").via, None);
    // why: the dotted unrestricted cover beats the test cover (most permissive first).
    let via = by_pair("a1", "b2").via.clone().unwrap();
    assert_eq!((via.edge.as_str(), via.line), ("ga -.-> b2", 12));
    assert_eq!(by_pair("a1", "b2").kind, Some(EdgeKind::Planned));
    let via = by_pair("a2", "b2").via.clone().unwrap();
    assert_eq!(via.line, 12);
    let summary: Vec<(String, usize, usize)> = overview
        .boundary_edges
        .iter()
        .map(|e| (e.kind.arrow().to_owned(), e.covers, e.realised))
        .collect();
    assert_eq!(
        summary,
        [("-->|test|".to_owned(), 4, 1), ("-.->".to_owned(), 2, 2),]
    );
}

#[test]
fn a_pair_no_edge_covers_has_no_kind_and_no_via() {
    let contract = double("  ga --> gb\n");
    let owned = [("a1/x.ts", "a1"), ("b1/y.ts", "b1")];
    let imports = [("b1/y.ts", 1, "a1/x.ts", ImportKind::Runtime, false)];
    let overview = view(&contract, &owned, &imports);
    assert_eq!(overview.edges.len(), 1);
    assert_eq!(
        (overview.edges[0].kind, &overview.edges[0].via),
        (None, &None)
    );
}

#[test]
fn diff_reports_boundary_edges_by_their_ends() {
    let old = "flowchart LR\n  subgraph g[\"g\"]\n    a\n  end\n  subgraph h[\"h\"]\n    b\n  end\n  g --> h\n  h --> g\n  a --> h\n";
    let new = "flowchart LR\n  subgraph g[\"g\"]\n    a\n  end\n  subgraph h[\"h\"]\n    b\n  end\n  g -.-> h\n  g --> g\n  a --> h\n";
    let diff = contract_diff(old, new).unwrap();
    let text = |ends: &crate::model::EdgeEnds| format!("{} {}", ends.from, ends.to);
    assert_eq!(
        diff.added_boundary_edges
            .iter()
            .map(|(e, k)| (text(e), *k))
            .collect::<Vec<_>>(),
        [("g g".to_owned(), EdgeKind::Implemented)]
    );
    assert_eq!(
        diff.removed_boundary_edges
            .iter()
            .map(|(e, k)| (text(e), *k))
            .collect::<Vec<_>>(),
        [("h g".to_owned(), EdgeKind::Implemented)]
    );
    assert_eq!(
        diff.changed_boundary_edges
            .iter()
            .map(|(e, o, n)| (text(e), *o, *n))
            .collect::<Vec<_>>(),
        [("g h".to_owned(), EdgeKind::Implemented, EdgeKind::Planned)]
    );
    assert!(diff.added_edges.is_empty() && diff.removed_edges.is_empty());
    assert!(!diff.is_empty());
    assert!(contract_diff(old, old).unwrap().is_empty());
}
