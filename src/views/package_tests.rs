//! The package summary the overview carries.

use std::collections::BTreeMap;

use super::fixtures::node;
use super::with_packages;
use crate::model::{BlindTally, NodeSummary, Overview, PackageKind, PackageSummary};

#[test]
fn with_packages_fills_each_node_and_leaves_the_others_empty() {
    let summary = BTreeMap::from([(
        node("db"),
        vec![PackageSummary {
            kind: PackageKind::ThirdParty,
            name: "lit".to_owned(),
            count: 1,
            tests: 0,
        }],
    )]);
    let blank = |raw: &str| NodeSummary {
        id: node(raw),
        label: None,
        dirs: Vec::new(),
        files: 0,
        assets: 0,
        excluded: 0,
        exports: None,
        blind_spots: BlindTally::default(),
        layer: None,
        budget: None,
        packages: Vec::new(),
    };
    let overview = Overview {
        nodes: vec![blank("db"), blank("ui")],
        ..Overview::default()
    };
    let filled = with_packages(overview, &summary);
    assert_eq!(filled.nodes[0].packages.len(), 1);
    assert_eq!(filled.nodes[1].packages, Vec::new());
}
