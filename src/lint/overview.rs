//! The architecture at a glance: nodes, declared edges and observed edges in one value.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::model::{
    BlindSpots, BoundaryEdgeSummary, Contract, Declared, DirTag, Edge, EdgeKind, EdgeSummary,
    ImportKind, LintReport, MapTarget, NodeBudget, NodeId, NodeSummary, NodeView, OpenAccess,
    Overview, Ownership, PackageSummary, Scope, Site, Symbol, Tag, ViaEdge,
};

/// Combines the contract, ownership, observed edges, the symbols they use and the lint report
/// into an [`Overview`].
///
/// Pure. Nodes are the declared ones, sorted by id. Edges are the union of the declared and the
/// observed edges, sorted by `(from, to)`: a declared edge nothing uses has `realised == 0`, and
/// an observed edge the contract lacks has no kind. A node's directories are those holding a
/// `.arch` file that names it, and the module paths of its files that a `node: *` tag owns (see
/// "Node directories" in `docs/DESIGN.md`); its files are the source files it owns; its exports are its
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
    let mapped = !contract.maps.is_empty();
    let mut modules = module_files(contract, ownership);
    let mut globs = map_globs(contract);
    let mut assets: BTreeMap<&NodeId, BTreeSet<PathBuf>> = BTreeMap::new();
    for (file, owner) in &ownership.asset_owners {
        assets.entry(owner).or_default().insert(file.clone());
    }
    let mut excluded: BTreeMap<&NodeId, BTreeSet<PathBuf>> = BTreeMap::new();
    for (file, owner) in &ownership.excluded_owners {
        excluded.entry(owner).or_default().insert(file.clone());
    }
    let nodes = contract
        .nodes
        .iter()
        .map(|(id, info)| {
            let mut dirs: BTreeSet<PathBuf> = if mapped {
                globs.remove(id).unwrap_or_default()
            } else {
                ownership
                    .tags
                    .iter()
                    .filter(|(at, name)| {
                        name == id.as_str() && !derives_modules(ownership, &at.file)
                    })
                    .map(|(at, _)| tagged_dir(&at.file))
                    .collect()
            };
            dirs.extend(modules.remove(id).into_iter().flatten());
            let owned: BTreeSet<&PathBuf> = assets
                .get(id)
                .into_iter()
                .chain(excluded.get(id))
                .flatten()
                .collect();
            // why: a node that owns only assets (`app.css`) must still say where it lives.
            if dirs.is_empty() {
                dirs.extend(
                    owned
                        .iter()
                        .filter_map(|file| module_path(&ownership.tag_dirs, file)),
                );
            }
            NodeSummary {
                id: id.clone(),
                label: info.label.clone(),
                dirs: dirs.into_iter().collect(),
                files: ownership
                    .owners
                    .values()
                    .filter(|owner| *owner == id)
                    .count(),
                assets: assets.get(id).map_or(0, BTreeSet::len),
                excluded: excluded.get(id).map_or(0, BTreeSet::len),
                exports: contract.surfaces.get(id).cloned(),
                blind_spots: blind.tally(Some(id)),
                layer: contract.rules.layer(info),
                budget: None,
                packages: Vec::new(),
            }
        })
        .collect();

    let edges = edge_summaries(contract, observed, symbols);
    let boundary_edges = boundary_edge_summaries(contract, observed);

    let (errors, warnings) = report.counts();
    Overview {
        nodes,
        edges,
        boundary_edges,
        errors,
        warnings,
        blind_spots: blind.summary(),
        unmapped: blind.tally(None),
        unmapped_items: blind.of_unowned(),
        skipped_references: blind.skipped.clone(),
        rules: contract.rules.summary(&contract.nodes),
    }
}

/// Every declared or observed edge as the overview presents it, sorted by `(from, to)`.
fn edge_summaries(
    contract: &Contract,
    observed: &BTreeMap<Edge, Vec<Site>>,
    symbols: &BTreeMap<Edge, BTreeSet<Symbol>>,
) -> Vec<EdgeSummary> {
    let all_edges: BTreeSet<&Edge> = contract.edges.keys().chain(observed.keys()).collect();
    all_edges
        .into_iter()
        .map(|edge| {
            let sites = observed.get(edge).map_or(&[][..], Vec::as_slice);
            // why: the winner of the precedence of "Boundary edges" speaks for the pair.
            let winner = contract.covers(edge).into_iter().next();
            // why: an open chain allows the pair as a solid edge, named by its line, not declared.
            let open = winner
                .is_none()
                .then(|| contract.open_access(edge))
                .flatten()
                .and_then(|access| match access {
                    OpenAccess::Allowed(order) => Some(ViaEdge {
                        edge: format!("order open: {}", order.boundaries.join(" > ")),
                        line: order.line,
                    }),
                    OpenAccess::Narrowed { .. } => None,
                });
            let kind = winner
                .map(|cover| cover.kind)
                .or_else(|| open.as_ref().map(|_| EdgeKind::Implemented));
            EdgeSummary {
                from: edge.from.clone(),
                to: edge.to.clone(),
                kind,
                realised: sites.len(),
                sites: sites.to_vec(),
                symbols: symbols.get(edge).cloned().unwrap_or_default(),
                module: sites.iter().any(|site| site.module),
                type_candidate: winner.is_some()
                    && kind == Some(EdgeKind::Implemented)
                    && !sites.is_empty()
                    && sites.iter().all(|site| site.kind == ImportKind::TypeOnly),
                via: winner
                    .and_then(|cover| match cover.by {
                        Declared::Boundary(declared) => Some(ViaEdge {
                            edge: declared.text(),
                            line: declared.line,
                        }),
                        Declared::Node => None,
                    })
                    .or(open),
            }
        })
        .collect()
}

/// Every boundary edge with the number of pairs it covers and the number code realises.
fn boundary_edge_summaries(
    contract: &Contract,
    observed: &BTreeMap<Edge, Vec<Site>>,
) -> Vec<BoundaryEdgeSummary> {
    contract
        .boundary_edges
        .iter()
        .map(|declared| {
            let pairs = declared.pairs(&contract.nodes);
            BoundaryEdgeSummary {
                from: declared.from.clone(),
                to: declared.to.clone(),
                kind: declared.kind,
                line: declared.line,
                covers: pairs.len(),
                realised: pairs
                    .iter()
                    .filter(|pair| {
                        observed.get(*pair).is_some_and(|sites| {
                            sites
                                .iter()
                                .any(|site| declared.kind.allows(site.kind, site.test_only))
                        })
                    })
                    .count(),
            }
        })
        .collect()
}

/// Gives every node of `overview` that has a `%% budget` its limits and current values.
pub fn attach_budgets(overview: &mut Overview, budgets: &BTreeMap<NodeId, NodeBudget>) {
    for node in &mut overview.nodes {
        node.budget = budgets.get(&node.id).copied();
    }
}

/// `overview` with each node's `packages` set from `packages`, which [`Observation`](super::Observation)
/// holds. A node `packages` has no entry for uses none. Pure.
#[must_use]
pub fn with_packages(
    mut overview: Overview,
    packages: &BTreeMap<NodeId, Vec<PackageSummary>>,
) -> Overview {
    for node in &mut overview.nodes {
        node.packages = packages.get(&node.id).cloned().unwrap_or_default();
    }
    overview
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
        asset_files: owned_by(&ownership.asset_owners, id),
        excluded_files: owned_by(&ownership.excluded_owners, id),
        blind_spots: blind.of_node(&node.id),
    })
}

/// The files of `owners` that the node `id` owns, sorted.
fn owned_by(owners: &BTreeMap<PathBuf, NodeId>, id: &str) -> Vec<PathBuf> {
    owners
        .iter()
        .filter(|(_, owner)| owner.as_str() == id)
        .map(|(file, _)| file.clone())
        .collect()
}

/// The module path of every source file a module tag or module-form map owns, by node.
fn module_files<'a>(
    contract: &Contract,
    ownership: &'a Ownership,
) -> BTreeMap<&'a NodeId, BTreeSet<PathBuf>> {
    let module_lines: BTreeSet<u32> = contract
        .maps
        .iter()
        .filter(|rule| matches!(rule.target, MapTarget::Modules(_)))
        .map(|rule| rule.line)
        .collect();
    let mut modules: BTreeMap<&NodeId, BTreeSet<PathBuf>> = BTreeMap::new();
    for (file, owner) in &ownership.owners {
        // why: a plain glob inside a module directory owns its file as a whole, not as a module.
        let by_module = ownership
            .map_sites
            .get(file)
            .is_none_or(|site| module_lines.contains(&site.line));
        if !by_module {
            continue;
        }
        if let Some(path) = module_path(&ownership.tag_dirs, file) {
            modules.entry(owner).or_default().insert(path);
        }
    }
    modules
}

/// The globs of the plain `%% map` lines, by node: with maps they are the `dirs` of a node.
fn map_globs(contract: &Contract) -> BTreeMap<&NodeId, BTreeSet<PathBuf>> {
    let mut globs: BTreeMap<&NodeId, BTreeSet<PathBuf>> = BTreeMap::new();
    for rule in &contract.maps {
        if let MapTarget::Node(id) = &rule.target {
            globs
                .entry(id)
                .or_default()
                .extend(rule.globs.iter().map(|glob| PathBuf::from(glob.as_str())));
        }
    }
    globs
}

/// The module `file` is in when a `node: *` tag owns it: the tagged directory joined with the
/// first segment below it, so a file in a subdirectory gives the directory and a file directly in
/// the tagged directory gives itself. `None` when no tag owns `file` or the tag names one node.
fn module_path(tags: &BTreeMap<PathBuf, DirTag>, file: &Path) -> Option<PathBuf> {
    // why: the nearest tag that covers the file decides, as in `ownership::owner_in`.
    let (dir, found) = file
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .find_map(|dir| {
            let found = tags.get(dir)?;
            (found.scope == Scope::Subtree || file.parent() == Some(dir)).then_some((dir, found))
        })?;
    if !matches!(found.tag, Tag::Modules(_)) {
        return None;
    }
    let first = file.strip_prefix(dir).ok()?.components().next()?;
    Some(dir.join(first))
}

/// Whether the `.arch` file `arch_file` is a `node: *` tag. The scan lists the node names such a
/// tag derives as tags of the file, but the directory is not where those nodes live.
fn derives_modules(ownership: &Ownership, arch_file: &Path) -> bool {
    let dir = arch_file.parent().unwrap_or_else(|| Path::new(""));
    ownership
        .tag_dirs
        .get(dir)
        .is_some_and(|found| matches!(found.tag, Tag::Modules(_)))
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
    use crate::lint::fixtures::{at, contract, edge, node, ownership, with_surface};
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
                    test_only: false,
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

    fn module_tags(entries: &[(&str, Tag, Scope)]) -> BTreeMap<PathBuf, DirTag> {
        entries
            .iter()
            .map(|(dir, tag, scope)| {
                let found = DirTag {
                    tag: tag.clone(),
                    scope: *scope,
                    peers: BTreeSet::new(),
                };
                (PathBuf::from(dir), found)
            })
            .collect()
    }

    #[test]
    fn a_module_path_is_the_directory_module_or_the_file_itself() {
        let modules = Tag::Modules(crate::model::Prefix::default());
        let tags = module_tags(&[
            ("src", modules.clone(), Scope::Subtree),
            ("flat", modules.clone(), Scope::Here),
            ("", modules, Scope::Here),
            ("src/ui/own", Tag::Node(node("own")), Scope::Subtree),
        ]);
        let path = |file: &str| module_path(&tags, Path::new(file));
        assert_eq!(path("src/ui/a.ts"), Some(PathBuf::from("src/ui")));
        assert_eq!(path("src/ui/deep/b.ts"), Some(PathBuf::from("src/ui")));
        assert_eq!(path("src/gitlab.rs"), Some(PathBuf::from("src/gitlab.rs")));
        assert_eq!(path("flat/a.ts"), Some(PathBuf::from("flat/a.ts")));
        assert_eq!(path("flat/sub/a.ts"), None);
        assert_eq!(path("main.rs"), Some(PathBuf::from("main.rs")));
        assert_eq!(path("src/ui/own/x.ts"), None);
        assert_eq!(path("elsewhere/x.ts"), None);
    }

    #[test]
    fn a_node_that_owns_only_assets_shows_them() {
        let c = contract(&["app", "ui"], &[]);
        let mut own = ownership(&[("src/ui.ts", "ui")], &[("src/.arch", 1, "app")]);
        own.asset_owners
            .insert(PathBuf::from("src/app.css"), node("app"));
        own.excluded_owners
            .insert(PathBuf::from("src/app.min.js"), node("app"));
        own.tag_dirs = module_tags(&[(
            "src",
            Tag::Modules(crate::model::Prefix::default()),
            Scope::Subtree,
        )]);
        let view = overview(
            &c,
            &own,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &BlindSpots::default(),
        );
        let app = &view.nodes[0];
        assert_eq!(app.files, 0);
        assert_eq!((app.assets, app.excluded), (1, 1));
        assert_eq!(
            app.dirs,
            [
                PathBuf::from("src/app.css"),
                PathBuf::from("src/app.min.js")
            ]
        );
        let ui = &view.nodes[1];
        assert_eq!((ui.assets, ui.dirs.len()), (0, 1));
        let listed = node_view(&view, &own, &BlindSpots::default(), "app").unwrap();
        assert_eq!(listed.asset_files, [PathBuf::from("src/app.css")]);
        assert_eq!(listed.excluded_files, [PathBuf::from("src/app.min.js")]);
    }

    #[test]
    fn derived_nodes_show_their_module_paths_and_plain_tags_their_directory() {
        let c = contract(&["ui", "gitlab", "group", "tool"], &[]);
        let mut own = ownership(
            &[
                ("src/ui/a.ts", "ui"),
                ("src/ui/b/c.ts", "ui"),
                ("src/gitlab.rs", "gitlab"),
                ("src/gitlab/x.rs", "gitlab"),
                ("g/a.ts", "group"),
                ("tools/t.ts", "tool"),
            ],
            &[
                ("src/.arch", 1, "ui"),
                ("src/.arch", 1, "gitlab"),
                ("g/.arch", 1, "group"),
                ("tools/.arch", 1, "tool"),
            ],
        );
        own.tag_dirs = module_tags(&[
            (
                "src",
                Tag::Modules(crate::model::Prefix::default()),
                Scope::Subtree,
            ),
            ("g", Tag::Node(node("group")), Scope::Here),
            ("tools", Tag::Node(node("tool")), Scope::Subtree),
        ]);
        let view = overview(
            &c,
            &own,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &LintReport::default(),
            &BlindSpots::default(),
        );
        let dirs: Vec<(&str, Vec<String>)> = view
            .nodes
            .iter()
            .map(|n| {
                (
                    n.id.as_str(),
                    n.dirs.iter().map(|d| d.display().to_string()).collect(),
                )
            })
            .collect();
        assert_eq!(
            dirs,
            [
                ("gitlab", vec!["src/gitlab", "src/gitlab.rs"]),
                ("group", vec!["g"]),
                ("tool", vec!["tools"]),
                ("ui", vec!["src/ui"]),
            ]
            .map(|(id, dirs)| (id, dirs.into_iter().map(str::to_owned).collect()))
        );
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
            target: None,
            test_only: false,
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
