//! The architecture at a glance: nodes, declared edges and observed edges in one value.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::model::{
    BlindSpots, Contract, Edge, EdgeSummary, LintReport, NodeSummary, NodeView, Overview,
    Ownership, Site, Symbol,
};

/// Combines the contract, ownership, observed edges, the symbols they use and the lint report
/// into an [`Overview`].
///
/// Pure. Nodes are the declared ones, sorted by id. Edges are the union of the declared and the
/// observed edges, sorted by `(from, to)`: a declared edge nothing uses has `realised == 0`, and
/// an observed edge the contract lacks has no kind. A node's directories are those holding a
/// `.arch` file that names it; its files are the source files it owns; its exports are its
/// declared surface. An edge keeps every site, with its kind and test flag. Every node carries the
/// tally of its blind spots, and the overview the summary of all of them, and the tally and the
/// items of the ones no node owns.
#[must_use]
pub fn overview(
    contract: &Contract,
    ownership: &Ownership,
    observed: &BTreeMap<Edge, Vec<Site>>,
    symbols: &BTreeMap<Edge, BTreeSet<Symbol>>,
    report: &LintReport,
    blind: &BlindSpots,
) -> Overview {
    let nodes = contract
        .nodes
        .iter()
        .map(|(id, info)| {
            let dirs: BTreeSet<PathBuf> = ownership
                .tags
                .iter()
                .filter(|(_, name)| name == id.as_str())
                .map(|(at, _)| tagged_dir(&at.file))
                .collect();
            NodeSummary {
                id: id.clone(),
                label: info.label.clone(),
                dirs: dirs.into_iter().collect(),
                files: ownership
                    .owners
                    .values()
                    .filter(|owner| *owner == id)
                    .count(),
                exports: contract.surfaces.get(id).cloned(),
                blind_spots: blind.tally(Some(id)),
            }
        })
        .collect();

    let all_edges: BTreeSet<&Edge> = contract.edges.keys().chain(observed.keys()).collect();
    let edges = all_edges
        .into_iter()
        .map(|edge| {
            let sites = observed.get(edge).map_or(&[][..], Vec::as_slice);
            EdgeSummary {
                from: edge.from.clone(),
                to: edge.to.clone(),
                kind: contract.edges.get(edge).copied(),
                realised: sites.len(),
                sites: sites.to_vec(),
                symbols: symbols.get(edge).cloned().unwrap_or_default(),
                module: sites.iter().any(|site| site.module),
            }
        })
        .collect();

    let (errors, warnings) = report.counts();
    Overview {
        nodes,
        edges,
        errors,
        warnings,
        blind_spots: blind.summary(),
        unmapped: blind.tally(None),
        unmapped_items: blind.of_unowned(),
    }
}

/// The node named `id` with every edge that leaves or enters it, the source files it owns and
/// everything it is blind to, or `None` when the overview has no such node. Pure.
#[must_use]
pub fn node_view(
    overview: &Overview,
    ownership: &Ownership,
    blind: &BlindSpots,
    id: &str,
) -> Option<NodeView> {
    let node = overview.nodes.iter().find(|node| node.id.as_str() == id)?;
    let touching = |keep: fn(&EdgeSummary, &str) -> bool| -> Vec<EdgeSummary> {
        overview
            .edges
            .iter()
            .filter(|edge| keep(edge, id))
            .cloned()
            .collect()
    };
    Some(NodeView {
        node: node.clone(),
        outgoing: touching(|edge, id| edge.from.as_str() == id),
        incoming: touching(|edge, id| edge.to.as_str() == id),
        files: ownership
            .owners
            .iter()
            .filter(|(_, owner)| owner.as_str() == id)
            .map(|(file, _)| file.clone())
            .collect(),
        blind_spots: blind.of_node(&node.id),
    })
}

/// The directory a `.arch` file tags: its parent, with the repository root spelled `.`.
fn tagged_dir(arch_file: &Path) -> PathBuf {
    match arch_file.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::fixtures::{at, contract, edge, ownership, with_surface};
    use crate::model::{EdgeKind, NodeId, Severity, SkipReason, Skipped, Violation, ViolationKind};

    fn sites(file: &str, lines: &[u32]) -> Vec<Site> {
        lines
            .iter()
            .map(|&line| Site::plain(at(file, line)))
            .collect()
    }

    fn view_of(overview: &Overview, id: &str) -> Option<NodeView> {
        node_view(overview, &Ownership::default(), &BlindSpots::default(), id)
    }

    fn report(severities: &[Severity]) -> LintReport {
        LintReport {
            violations: severities
                .iter()
                .map(|&severity| Violation {
                    kind: ViolationKind::StaleEdge,
                    severity,
                    at: None,
                    message: String::new(),
                    hint: None,
                })
                .collect(),
        }
    }

    fn sample() -> Overview {
        let c = contract(
            &["a", "b", "c"],
            &[
                ("a", "b", EdgeKind::Implemented),
                ("b", "c", EdgeKind::Planned),
            ],
        );
        let own = ownership(
            &[("a/x.ts", "a"), ("a/y.ts", "a"), ("b/z.ts", "b")],
            &[
                ("a/.arch", 1, "a"),
                ("b/.arch", 1, "b"),
                ("b/sub/.arch", 1, "b"),
            ],
        );
        let mut observed = BTreeMap::new();
        observed.insert(edge("a", "b"), sites("a/x.ts", &[1, 2, 3, 4, 5]));
        observed.insert(edge("c", "a"), sites("c/q.ts", &[7]));
        overview(
            &c,
            &own,
            &observed,
            &BTreeMap::new(),
            &report(&[Severity::Error, Severity::Warning]),
            &BlindSpots::default(),
        )
    }

    #[test]
    fn nodes_carry_label_dirs_and_file_counts() {
        let view = sample();
        let ids: Vec<&str> = view.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        let a = &view.nodes[0];
        assert_eq!(a.label.as_deref(), Some("a label"));
        assert_eq!(a.dirs, vec![PathBuf::from("a")]);
        assert_eq!(a.files, 2);
        assert_eq!(
            view.nodes[1].dirs,
            vec![PathBuf::from("b"), PathBuf::from("b/sub")]
        );
        assert_eq!(view.nodes[2].dirs, Vec::<PathBuf>::new());
        assert_eq!(view.nodes[2].files, 0);
    }

    #[test]
    fn a_tag_at_the_root_names_the_dot_directory() {
        let c = contract(&["a"], &[]);
        let own = ownership(&[("x.ts", "a")], &[(".arch", 1, "a")]);
        let view = overview(
            &c,
            &own,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &BlindSpots::default(),
        );
        assert_eq!(view.nodes[0].dirs, vec![PathBuf::from(".")]);
    }

    #[test]
    fn tags_naming_undeclared_nodes_are_not_nodes() {
        let c = contract(&["a"], &[]);
        let own = ownership(&[], &[("g/.arch", 1, "ghost")]);
        let view = overview(
            &c,
            &own,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &BlindSpots::default(),
        );
        assert_eq!(view.nodes.len(), 1);
    }

    #[test]
    fn edges_are_declared_and_observed_sorted_with_kinds_and_counts() {
        let view = sample();
        let summary: Vec<_> = view
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str(), e.kind, e.realised))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("a", "b", Some(EdgeKind::Implemented), 5),
                ("b", "c", Some(EdgeKind::Planned), 0),
                ("c", "a", None, 1),
            ]
        );
    }

    #[test]
    fn every_site_is_kept_and_counted() {
        let view = sample();
        assert_eq!(view.edges[0].realised, 5);
        assert_eq!(view.edges[0].sites, sites("a/x.ts", &[1, 2, 3, 4, 5]));
        assert_eq!(view.edges[1].sites, Vec::new());
    }

    fn used(names: &[&str]) -> BTreeSet<Symbol> {
        names
            .iter()
            .map(|name| match *name {
                "*" => Symbol::All,
                other => Symbol::Name(other.to_owned()),
            })
            .collect()
    }

    #[test]
    fn edges_carry_the_symbols_over_all_their_sites_not_only_the_kept_ones() {
        let c = contract(&["a", "b"], &[("a", "b", EdgeKind::Implemented)]);
        let mut observed = BTreeMap::new();
        observed.insert(edge("a", "b"), sites("a/x.ts", &[1, 2, 3, 4, 5]));
        let mut symbols = BTreeMap::new();
        symbols.insert(edge("a", "b"), used(&["*", "z", "a"]));
        let view = overview(
            &c,
            &Ownership::default(),
            &observed,
            &symbols,
            &LintReport::default(),
            &BlindSpots::default(),
        );
        assert_eq!(view.edges[0].sites.len(), 5);
        assert_eq!(view.edges[0].symbols, used(&["a", "z", "*"]));
        let listed: Vec<&str> = view.edges[0].symbols.iter().map(Symbol::as_str).collect();
        assert_eq!(listed, ["a", "z", "*"]);
    }

    #[test]
    fn an_edge_nothing_uses_has_no_symbols() {
        assert!(sample().edges.iter().all(|e| e.symbols.is_empty()));
    }

    #[test]
    fn nodes_carry_their_declared_surface_and_others_none() {
        let c = with_surface(contract(&["a", "b"], &[]), "b", &["Client", "fetch"]);
        let view = overview(
            &c,
            &Ownership::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &BlindSpots::default(),
        );
        assert_eq!(view.nodes[0].exports, None);
        assert_eq!(
            view.nodes[1].exports,
            Some(["Client", "fetch"].map(str::to_owned).into())
        );
    }

    #[test]
    fn counts_come_from_the_report() {
        let view = sample();
        assert_eq!((view.errors, view.warnings), (1, 1));
    }

    #[test]
    fn empty_inputs_give_an_empty_overview() {
        let view = overview(
            &Contract::default(),
            &Ownership::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &BlindSpots::default(),
        );
        assert_eq!(view, Overview::default());
    }

    #[test]
    fn node_view_splits_edges_by_direction() {
        let view = view_of(&sample(), "b").unwrap();
        assert_eq!(view.node.id.as_str(), "b");
        let out: Vec<_> = view.outgoing.iter().map(|e| e.to.as_str()).collect();
        let inc: Vec<_> = view.incoming.iter().map(|e| e.from.as_str()).collect();
        assert_eq!(out, ["c"]);
        assert_eq!(inc, ["a"]);
    }

    #[test]
    fn node_view_includes_undeclared_observed_edges() {
        let view = view_of(&sample(), "a").unwrap();
        assert_eq!(view.outgoing.len(), 1);
        assert_eq!(view.incoming.len(), 1);
        assert_eq!(view.incoming[0].from.as_str(), "c");
        assert_eq!(view.incoming[0].kind, None);
    }

    #[test]
    fn node_view_of_an_isolated_node_has_no_edges() {
        let c = contract(&["a", "b"], &[]);
        let view = overview(
            &c,
            &Ownership::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &BlindSpots::default(),
        );
        let found = view_of(&view, "a").unwrap();
        assert!(found.outgoing.is_empty() && found.incoming.is_empty());
    }

    #[test]
    fn node_view_of_an_unknown_node_is_none() {
        assert_eq!(view_of(&sample(), "ghost"), None);
        assert_eq!(view_of(&sample(), ""), None);
    }

    fn blind() -> BlindSpots {
        let ownership = Ownership {
            unread: BTreeMap::from([(
                "swift".to_owned(),
                [PathBuf::from("a/V.swift"), PathBuf::from("loose.swift")].into(),
            )]),
            ..Ownership::default()
        };
        let skipped = [Skipped {
            at: at("b/z.ts", 1),
            specifier: "left-pad".to_owned(),
            reason: SkipReason::UndeclaredPackage,
            message: None,
        }];
        BlindSpots::build(
            &ownership,
            |path| path.starts_with("a").then(|| NodeId::new("a").unwrap()),
            |_| Some(2),
            &skipped,
            &[],
        )
    }

    #[test]
    fn nodes_carry_their_blind_tally_and_the_overview_the_summary_and_the_unmapped_tally() {
        let c = contract(&["a", "b"], &[]);
        let view = overview(
            &c,
            &Ownership::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &blind(),
        );
        assert_eq!(view.nodes[0].blind_spots.unread, 1);
        assert_eq!(view.nodes[1].blind_spots.unread, 0);
        assert_eq!(view.unmapped.unread, 1);
        assert_eq!(view.blind_spots.unread["swift"].count, 2);
        assert_eq!(view.blind_spots.unread["swift"].lines, 4);
    }

    #[test]
    fn a_node_view_lists_the_files_of_its_node_and_only_its_blind_items() {
        let c = contract(&["a", "b"], &[]);
        let own = ownership(
            &[("a/x.ts", "a"), ("a/y.ts", "a"), ("b/z.ts", "b")],
            &[("a/.arch", 1, "a"), ("b/.arch", 1, "b")],
        );
        let spots = blind();
        let all = overview(
            &c,
            &own,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &spots,
        );
        let a = node_view(&all, &own, &spots, "a").unwrap();
        assert_eq!(a.files, [PathBuf::from("a/x.ts"), PathBuf::from("a/y.ts")]);
        assert_eq!(a.blind_spots.unread["swift"].files.len(), 1);
        assert!(a.blind_spots.skipped.is_empty());
        let b = node_view(&all, &own, &spots, "b").unwrap();
        assert_eq!(b.files, [PathBuf::from("b/z.ts")]);
        assert!(b.blind_spots.unread.is_empty());
    }

    #[test]
    fn overview_is_independent_of_construction_order() {
        let c = contract(&["a", "b"], &[]);
        let own = ownership(&[], &[("b/.arch", 1, "b"), ("a/.arch", 1, "a")]);
        let mut own2 = own.clone();
        own2.tags.reverse();
        let observed: BTreeMap<_, _> = [(edge("a", "b"), sites("a/x.ts", &[1]))].into();
        let none = LintReport::default();
        assert_eq!(
            overview(
                &c,
                &own,
                &observed,
                &BTreeMap::new(),
                &none,
                &BlindSpots::default()
            ),
            overview(
                &c,
                &own2,
                &observed,
                &BTreeMap::new(),
                &none,
                &BlindSpots::default()
            )
        );
    }
}
