//! In-memory builders shared by the tests of the pure lint functions.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::imports::FileImports;
use crate::model::{
    Contract, Edge, EdgeKind, Location, NodeId, NodeInfo, Ownership, ResolvedImport,
};

pub fn node(raw: &str) -> NodeId {
    NodeId::new(raw).unwrap()
}

pub fn edge(from: &str, to: &str) -> Edge {
    Edge {
        from: node(from),
        to: node(to),
    }
}

pub fn at(file: &str, line: u32) -> Location {
    Location {
        file: PathBuf::from(file),
        line,
    }
}

pub fn contract(nodes: &[&str], edges: &[(&str, &str, EdgeKind)]) -> Contract {
    Contract {
        nodes: nodes
            .iter()
            .map(|n| {
                (
                    node(n),
                    NodeInfo {
                        label: Some(format!("{n} label")),
                        boundary: None,
                    },
                )
            })
            .collect(),
        edges: edges
            .iter()
            .map(|&(from, to, kind)| (edge(from, to), kind))
            .collect(),
    }
}

/// Ownership from (file, owner) pairs and `.arch` tags given as (arch file, line, node name).
pub fn ownership(owners: &[(&str, &str)], tags: &[(&str, u32, &str)]) -> Ownership {
    Ownership {
        owners: owners
            .iter()
            .map(|&(file, owner)| (PathBuf::from(file), node(owner)))
            .collect(),
        tags: tags
            .iter()
            .map(|&(file, line, name)| (at(file, line), name.to_owned()))
            .collect(),
        ..Ownership::default()
    }
}

/// Imports from (importing file, line, target file) triples.
pub fn imports(resolved: &[(&str, u32, &str)]) -> BTreeMap<PathBuf, FileImports> {
    let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
    for &(file, line, target) in resolved {
        map.entry(PathBuf::from(file))
            .or_default()
            .resolved
            .push(ResolvedImport::new(at(file, line), PathBuf::from(target)));
    }
    map
}
