//! In-memory builders shared by the tests of the pure lint functions.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::code::{Location, Ownership, ResolvedImport, Symbol};
use crate::contract::{Contract, Edge, EdgeKind, NodeId, NodeInfo};
use crate::imports::FileImports;

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
                        outer: Vec::new(),
                    },
                )
            })
            .collect(),
        edges: edges
            .iter()
            .map(|&(from, to, kind)| (edge(from, to), kind))
            .collect(),
        ..Contract::default()
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

/// `contract` where `node` exports exactly `names`.
pub fn with_surface(mut contract: Contract, node_id: &str, names: &[&str]) -> Contract {
    contract.surfaces.insert(
        node(node_id),
        names.iter().map(|n| (*n).to_owned()).collect(),
    );
    contract
}

/// Imports from (importing file, line, target file, symbols used) tuples. A symbol written `*` is
/// [`Symbol::All`], any other is a named symbol. Each tuple is one import.
pub fn imports_using(resolved: &[(&str, u32, &str, &[&str])]) -> BTreeMap<PathBuf, FileImports> {
    let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
    for &(file, line, target, symbols) in resolved {
        let used = symbols.iter().map(|symbol| match *symbol {
            "*" => Symbol::All,
            name => Symbol::Name(name.to_owned()),
        });
        map.entry(PathBuf::from(file))
            .or_default()
            .resolved
            .push(ResolvedImport::new(at(file, line), PathBuf::from(target)).with_symbols(used));
    }
    map
}
