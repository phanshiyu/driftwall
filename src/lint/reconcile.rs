//! The pure core of the linter: compares three in-memory views and reports disagreements.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::code::{ImportKind, Location, Ownership, ResolvedImport, Symbol};
use crate::contract::{
    BoundaryEdge, Contract, Cover, Declared, Denial, Edge, EdgeKind, MapTarget, Narrowing,
    OpenAccess,
};
use crate::imports::FileImports;
use crate::results::{
    Declaration, EdgeStatus, LintReport, Seen, Violation, ViolationKind, edge_status, path_key,
};

use super::isolation;
use super::layers;
use super::observe::crossings;
use super::packages::package_findings;
use super::rules::{site_violation, violation};
use super::siblings;

/// Reconciles the contract, ownership and import graph into a sorted, duplicate-free report.
///
/// This function is pure: it touches no filesystem, clock or randomness, and the result does
/// not depend on the order in which the inputs were built. `contract_file` is the path of the
/// contract relative to the repository root; it is only used to name the contract in messages.
///
/// An import counts as an edge only when it crosses between two declared owners (see
/// [`observed_edges`](super::observed_edges), which shares the computation). Anything else (an
/// unmapped file, an owner that names an unknown node, an import within one node) produces no
/// edge findings, so one root cause is reported once. An edge is realised when code uses it,
/// whatever its declared kind.
///
/// An import that crosses into a node with a declared surface (`Contract::surfaces`) is also
/// checked against it: every symbol it uses must be listed, and `*` is private too. The same
/// crossing rules apply, so an unmapped or unknown owner and an import within one node never
/// produce a `private-symbol` finding.
#[must_use]
pub fn reconcile(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
    contract_file: &Path,
) -> LintReport {
    let mut violations = ownership_findings(contract, ownership, contract_file);

    let crossings = crossings(contract, ownership, imports);
    let owning = ownership.owning_nodes();
    for (edge, import) in &crossings {
        // why: a denied site has one finding, and nothing allows it, so nothing else is asked.
        if let Some(denial) = contract.denial(edge) {
            violations.push(denied_site(denial, edge, import, contract_file));
            continue;
        }
        let covers = contract.covers(edge);
        // why: the arrow `undeclared-edge` asks for is a contract error here, so one finding.
        let one_finding = layers::upward_site(contract, edge, import).is_some()
            || siblings::breaches(contract, edge);
        match covers.first() {
            _ if one_finding => {}
            // why: an open chain allows the pair as a solid edge would, until an arrow narrows it.
            None => match contract.open_access(edge, Narrowing::Owned(&owning)) {
                Some(OpenAccess::Allowed(_)) => {}
                access => violations.push(
                    site_violation(
                        ViolationKind::UndeclaredEdge,
                        import,
                        undeclared_message(access, edge, import, contract_file),
                    )
                    .keyed(format!("{} --> {}", edge.from, edge.to)),
                ),
            },
            // why: each covering edge is judged alone, so one that allows the site is enough.
            Some(winner)
                if !covers
                    .iter()
                    .any(|cover| cover.kind.allows(import.kind, import.test_only)) =>
            {
                violations.push(
                    site_violation(
                        ViolationKind::UndeclaredEdge,
                        import,
                        restricted_site_message(
                            winner,
                            covers.len() - 1,
                            edge,
                            import,
                            contract_file,
                        ),
                    )
                    .keyed(format!("{} --> {}", edge.from, edge.to)),
                );
            }
            Some(_) => {}
        }
        violations.extend(private_symbols(contract, edge, import, contract_file));
    }
    for file_imports in imports.values() {
        for import in &file_imports.unresolved {
            violations.push(
                violation(
                    ViolationKind::UnresolvedImport,
                    Some(import.at.clone()),
                    format!("import `{}` resolves to no file", import.specifier),
                )
                .keyed(format!(
                    "{} {}",
                    path_key(&import.at.file),
                    import.specifier
                )),
            );
        }
    }

    violations.extend(edge_findings(contract, &crossings));
    violations.extend(boundary_edge_findings(contract, &crossings));
    violations.extend(redundant_findings(contract));
    violations.extend(layers::upward_findings(contract, &crossings));
    violations.extend(isolation::findings(contract, imports));
    violations.extend(siblings::findings(contract, &crossings, contract_file));
    violations.extend(package_findings(
        contract,
        ownership,
        imports,
        contract_file,
    ));

    violations.sort();
    violations.dedup();
    LintReport { violations }
}

/// The `denied-edge` finding of a site on a pair that `denial` covers.
fn denied_site(
    denial: &Denial,
    edge: &Edge,
    import: &ResolvedImport,
    contract_file: &Path,
) -> Violation {
    let (from, to) = (&edge.from, &edge.to);
    site_violation(
        ViolationKind::DeniedEdge,
        import,
        format!(
            "`{from}` imports `{to}` ({}) but {}:{} `{}` forbids it",
            import.target.display(),
            contract_file.display(),
            denial.line,
            denial.text(),
        ),
    )
    .keyed(format!("{from} --> {to}"))
}

/// The message of a pair nothing covers. `access` says whether an open chain narrowed the pair.
fn undeclared_message(
    access: Option<OpenAccess<'_>>,
    edge: &Edge,
    import: &ResolvedImport,
    contract_file: &Path,
) -> String {
    let (from, to) = (&edge.from, &edge.to);
    let base = format!(
        "`{from}` imports `{to}` ({}) but {} declares no edge {from} --> {to}",
        import.target.display(),
        contract_file.display(),
    );
    match access {
        Some(OpenAccess::Narrowed {
            order,
            from: upper,
            to: lower,
            arrows,
            each: None,
        }) => format!(
            "{base}; the `%% order open` on line {} limits `{upper}` to the edges it declares towards `{lower}` ({arrows} declared) and this pair is not among them",
            order.line
        ),
        Some(OpenAccess::Narrowed {
            each: Some(each), ..
        }) => format!(
            "{base}; the `%% each {}` on line {} limits `{}` to the edges it declares towards `{}` and this pair is not among them",
            each.boundary, each.line, each.from, each.to
        ),
        _ => base,
    }
}

/// The message of a site `edge` does not allow: the pair has an edge, but only of a restricted
/// kind (`test`, `type` or both), and the site is not one it allows. `winner` is the covering edge
/// that wins the precedence of "Boundary edges", and `others` how many more cover the pair.
fn restricted_site_message(
    winner: &Cover<'_>,
    others: usize,
    edge: &Edge,
    import: &ResolvedImport,
    contract_file: &Path,
) -> String {
    let (from, to) = (&edge.from, &edge.to);
    let target = import.target.display();
    let contract = contract_file.display();
    let arrow = winner.kind.arrow();
    let declared = match winner.by {
        Declared::Node => format!("{from} {arrow} {to}"),
        Declared::Boundary(boundary) => format!("{} (line {})", boundary.text(), boundary.line),
    };
    let more = match others {
        0 => String::new(),
        1 => "; one more covering edge does not allow it either".to_owned(),
        n => format!("; {n} more covering edges do not allow it either"),
    };
    match winner.kind {
        EdgeKind::Test | EdgeKind::PlannedTest => format!(
            "`{from}` imports `{to}` ({target}) from production code but {contract} declares only a test edge {declared}, which allows test imports only{more}"
        ),
        EdgeKind::Type | EdgeKind::PlannedType => format!(
            "`{from}` imports `{to}` ({target}) at run time but {contract} declares only a type edge {declared}, which allows type-only imports only{more}"
        ),
        _ => format!(
            "`{from}` imports `{to}` ({target}) at run time from production code but {contract} declares only a type, test edge {declared}, which allows type-only and test imports only{more}"
        ),
    }
}

/// What to do about a planned edge that only tests use.
const PLANNED_TEST_ONLY_HINT: &str = "if tests alone should use it, declare `-.->|test|` or `-->|test|`; otherwise leave the edge dotted until the app uses it";

/// A finding about a declared edge of `edge_kind`. A planned edge only tests use gets its own hint.
fn edge_violation(
    kind: ViolationKind,
    edge_kind: EdgeKind,
    message: String,
    key: String,
) -> Violation {
    let mut found = violation(kind, None, message).keyed(key);
    if kind == ViolationKind::TestOnlyEdge && edge_kind.is_planned() {
        found.hint = Some(PLANNED_TEST_ONLY_HINT.to_owned());
    }
    found
}

/// The findings about declared edges: stale, planned and realised, and solid but realised by tests
/// only. A site an edge does not allow is not counted: it is its own `undeclared-edge`.
fn edge_findings(contract: &Contract, crossings: &[(Edge, &ResolvedImport)]) -> Vec<Violation> {
    let mut seen: BTreeMap<&Edge, Seen> = BTreeMap::new();
    for (edge, import) in crossings {
        if let Some(kind) = contract.edges.get(edge) {
            let entry = seen.entry(edge).or_insert(Seen::Unseen);
            *entry = entry.with_site(*kind, import.kind, import.test_only);
        }
    }
    let mut violations = Vec::new();
    for (edge, kind) in &contract.edges {
        let observed = seen.get(edge).copied().unwrap_or(Seen::Unseen);
        let status = edge_status(Declaration::Edge(*kind), observed);
        let ends = (edge.from.as_str(), edge.to.as_str());
        if let Some((found, message)) = declared_edge_finding(*kind, ends, status) {
            let key = format!("{} {} {}", edge.from, kind.arrow(), edge.to);
            violations.push(edge_violation(found, *kind, message, key));
        }
    }
    violations
}

/// The stale, planned-realised or test-only finding of one declared edge `from` to `to` of `kind`
/// in `status`, if any. Node edges, boundary edges and `%% each` arrows share it. It names every
/// status, so a new one must be placed here.
fn declared_edge_finding(
    kind: EdgeKind,
    (from, to): (&str, &str),
    status: EdgeStatus,
) -> Option<(ViolationKind, String)> {
    let arrow = kind.arrow();
    let sites = kind.allowed_sites();
    let found = status.finding()?;
    let message = match status {
        // why: no declared edge is undeclared; `reconcile` reports those per site.
        EdgeStatus::Ok
        | EdgeStatus::Open
        | EdgeStatus::Planned
        | EdgeStatus::Undeclared
        | EdgeStatus::Denied => {
            return None;
        }
        EdgeStatus::Stale if kind.is_restricted() => {
            format!("edge {from} {arrow} {to} is declared but no {sites} import realises it")
        }
        EdgeStatus::Stale => {
            format!("edge {from} {arrow} {to} is declared but no import realises it")
        }
        EdgeStatus::TestOnly if kind.is_planned() => format!(
            "planned edge {from} {arrow} {to} is used only by test code; the app does not use it yet; declare it {from} -.->|test| {to} or {from} -->|test| {to} if that is intended"
        ),
        EdgeStatus::TestOnly => format!(
            "edge {from} --> {to} is realised only by test code; declare it {from} -->|test| {to}"
        ),
        EdgeStatus::PlannedRealized if kind.is_restricted() => {
            format!("planned edge {from} {arrow} {to} is now realised by {sites} code")
        }
        EdgeStatus::PlannedRealized => {
            format!("planned edge {from} {arrow} {to} is now realised by code")
        }
    };
    Some((found, message))
}

/// What code does on every boundary edge, parallel to `contract.boundary_edges`.
///
/// It covers all the pairs each edge covers. `sites` is every crossing as `(pair, kind, test_only)`. A boundary
/// edge is stale only when no covered pair is realised (see "Boundary edges"). A template arrow is
/// one declaration, so every expansion of it holds the union over all of them.
#[must_use]
pub fn boundary_seen(contract: &Contract, sites: &[(&Edge, ImportKind, bool)]) -> Vec<Seen> {
    let own: Vec<Seen> = contract
        .boundary_edges
        .iter()
        .map(|declared| {
            sites
                .iter()
                .filter(|(edge, ..)| declared.covers(&contract.nodes, edge))
                .fold(Seen::Unseen, |seen, (_, kind, test_only)| {
                    seen.with_site(declared.kind, *kind, *test_only)
                })
        })
        .collect();
    contract
        .boundary_edges
        .iter()
        .map(|declared| {
            contract
                .boundary_edges
                .iter()
                .zip(&own)
                .filter(|(other, _)| std::ptr::eq(declared, *other) || same_arrow(declared, other))
                .fold(Seen::Unseen, |all, (_, seen)| all.merge(*seen))
        })
        .collect()
}

/// Whether `declared` and `other` are expansions of one `%% each` arrow.
fn same_arrow(declared: &BoundaryEdge, other: &BoundaryEdge) -> bool {
    // why: the template differs by child and by which ends are roles, not by the arrow.
    match (&declared.template, &other.template) {
        (Some(a), Some(b)) => {
            other.line == declared.line && a.boundary == b.boundary && a.arrow == b.arrow
        }
        _ => false,
    }
}

/// The same findings for every boundary edge, judged by [`boundary_seen`] and once per arrow.
fn boundary_edge_findings(
    contract: &Contract,
    crossings: &[(Edge, &ResolvedImport)],
) -> Vec<Violation> {
    let sites: Vec<(&Edge, ImportKind, bool)> = crossings
        .iter()
        .map(|(edge, import)| (edge, import.kind, import.test_only))
        .collect();
    let seen = boundary_seen(contract, &sites);
    contract
        .boundary_edges
        .iter()
        .zip(seen)
        .enumerate()
        .filter(|(index, (declared, _))| {
            // why: only the first expansion of an arrow speaks for it.
            !contract
                .boundary_edges
                .iter()
                .take(*index)
                .any(|earlier| same_arrow(declared, earlier))
        })
        .filter_map(|(_, (declared, seen))| {
            let status = edge_status(Declaration::Edge(declared.kind), seen);
            let (from, to) = finding_ends(declared);
            let (kind, message) =
                declared_edge_finding(declared.kind, (from.as_str(), to.as_str()), status)?;
            let ends = format!("{from} {} {to}", declared.kind.arrow());
            let key = declared.template.as_ref().map_or_else(
                || ends.clone(),
                |template| format!("each {}: {ends}", template.boundary),
            );
            Some(edge_violation(
                kind,
                declared.kind,
                template_message(declared, message),
                key,
            ))
        })
        .collect()
}

/// The ends of `declared` as its finding names them: the nodes of an edge written in the contract,
/// the roles of a template arrow.
fn finding_ends(declared: &BoundaryEdge) -> (String, String) {
    declared.template.as_ref().map_or_else(
        || (declared.from.to_string(), declared.to.to_string()),
        |template| (template.from.clone(), template.to.clone()),
    )
}

/// For an expansion, the finding is the template arrow's, over all the children: it names the
/// `%% each` line (see "Edge templates").
fn template_message(declared: &BoundaryEdge, message: String) -> String {
    match &declared.template {
        Some(template) => format!(
            "`%% each {}` on line {}: {message}",
            template.boundary, declared.line
        ),
        None => message,
    }
}

/// A `redundant-edge` warning for every node edge that a boundary edge of a kind at least as wide
/// already covers, naming the first such edge in the precedence order.
fn redundant_findings(contract: &Contract) -> Vec<Violation> {
    contract
        .edges
        .iter()
        .filter_map(|(edge, kind)| {
            let covering = contract.covers(edge).into_iter().find_map(|cover| {
                let Declared::Boundary(declared) = cover.by else {
                    return None;
                };
                let wide = declared.kind.at_least_as_wide_as(*kind);
                (wide && (kind.is_planned() || !declared.kind.is_planned())).then_some(declared)
            })?;
            Some(
                violation(
                    ViolationKind::RedundantEdge,
                    None,
                    format!(
                        "edge {from} {arrow} {to} is redundant: {} (line {}) already covers it",
                        covering.text(),
                        covering.line,
                        from = edge.from,
                        arrow = kind.arrow(),
                        to = edge.to,
                    ),
                )
                .keyed(format!("{} {} {}", edge.from, kind.arrow(), edge.to)),
            )
        })
        .collect()
}

/// One finding per symbol `import` uses that the surface of the node it enters does not list.
/// Nothing when that node declares no surface, or when the import is test code. An import that
/// uses no symbol is never private, and `*` is always private: the importer reaches names that are
/// never seen.
fn private_symbols(
    contract: &Contract,
    edge: &Edge,
    import: &ResolvedImport,
    contract_file: &Path,
) -> Vec<Violation> {
    let Some(surface) = contract.surfaces.get(&edge.to) else {
        return Vec::new();
    };
    // why: a surface is for production callers; tests drive internals, as with package rules.
    if import.test_only {
        return Vec::new();
    }
    let listed = surface.iter().map(String::as_str).collect::<Vec<_>>();
    import
        .symbols
        .iter()
        .filter(|symbol| match symbol {
            Symbol::Name(name) => !surface.contains(name),
            Symbol::All => true,
        })
        .map(|symbol| {
            site_violation(
                ViolationKind::PrivateSymbol,
                import,
                format!(
                    "`{from}` imports `{symbol}` from `{to}` ({}) but {} lets `{to}` export only: {}",
                    import.target.display(),
                    contract_file.display(),
                    listed.join(", "),
                    from = edge.from,
                    to = edge.to,
                ),
            )
            .keyed(format!("{} --> {} {symbol}", edge.from, edge.to))
            .naming(symbol.to_string())
        })
        .collect()
}

/// The findings about `%% map` lines: globs that match nothing, and `.arch` files nobody reads.
fn map_findings(ownership: &Ownership, contract_file: &Path) -> Vec<Violation> {
    let mut violations = Vec::new();
    for site in &ownership.stale_maps {
        let message = match (&site.each, &site.child) {
            (Some(each), Some(child)) => format!(
                "`%% {each}` maps child `{child}` to `{}`, which matches no file",
                site.glob
            ),
            _ => format!("map glob `{}` matches no file", site.glob),
        };
        violations.push(
            violation(
                ViolationKind::StaleMap,
                Some(Location {
                    file: contract_file.to_path_buf(),
                    line: site.line,
                }),
                message,
            )
            .keyed(site.glob.clone()),
        );
    }
    if !ownership.ignored_tags.is_empty() {
        let shown: Vec<String> = ownership
            .ignored_tags
            .iter()
            .take(3)
            .map(|path| path.display().to_string())
            .collect();
        let more = ownership.ignored_tags.len().saturating_sub(shown.len());
        let rest = if more == 0 {
            String::new()
        } else {
            format!(" and {more} more")
        };
        violations.push(
            violation(
                ViolationKind::IgnoredTagFiles,
                None,
                format!(
                    "{} has `%% map` lines, so {} `.arch` files are not read: {}{rest}",
                    contract_file.display(),
                    ownership.ignored_tags.len(),
                    shown.join(", ")
                ),
            )
            .keyed(path_key(contract_file)),
        );
    }
    violations
}

/// The findings about who owns what: tags naming undeclared nodes, `%% map` problems and files no
/// node owns.
fn ownership_findings(
    contract: &Contract,
    ownership: &Ownership,
    contract_file: &Path,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for (at, name) in &ownership.tags {
        if !declared(contract, name) {
            let source = if at.file == contract_file {
                each_source(contract, at.line, name)
                    .unwrap_or_else(|| "`%% map` derives".to_owned())
            } else {
                "`.arch` tags".to_owned()
            };
            violations.push(
                violation(
                    ViolationKind::UnknownNode,
                    Some(at.clone()),
                    format!(
                        "{source} `{name}` but {} declares no such node",
                        contract_file.display()
                    ),
                )
                .keyed(format!("{} {name}", path_key(&at.file))),
            );
        }
    }

    violations.extend(map_findings(ownership, contract_file));

    for file in &ownership.unmapped {
        let mut found = violation(
            ViolationKind::UnmappedFile,
            Some(Location {
                file: file.clone(),
                line: 1,
            }),
            format!("`{}` has no owning node", file.display()),
        )
        .keyed(path_key(file));
        if let Some((boundary, child)) = missing_child(contract, file) {
            found.hint = Some(format!(
                "draw a `{child}` subgraph inside `{boundary}`, or ask the user to approve adding it"
            ));
        }
        violations.push(found);
    }
    violations
}

/// How an `%% each ... map` line at `line` reads in the message about the node `name` it derives:
/// the line and the child it was expanded for, or `None` when no such line expanded to `name`.
fn each_source(contract: &Contract, line: u32, name: &str) -> Option<String> {
    contract.maps.iter().find_map(|rule| {
        let template = rule.template.as_ref().filter(|_| rule.line == line)?;
        let MapTarget::Modules(prefix) = &rule.target else {
            return None;
        };
        name.starts_with(prefix.as_str()).then(|| {
            format!(
                "`%% {}` (child `{}`) derives",
                template.text(),
                template.child
            )
        })
    })
}

/// The boundary and the child name of an `%% each ... map` line whose pattern would cover the
/// unmapped `file` if a child boundary of that name existed.
fn missing_child(contract: &Contract, file: &Path) -> Option<(String, String)> {
    contract.rules.each.iter().find_map(|rule| {
        let child = rule.would_cover(file)?;
        Some((rule.boundary.clone(), child))
    })
}

/// True when `name` is a node declared in the contract.
fn declared(contract: &Contract, name: &str) -> bool {
    contract.nodes.keys().any(|id| id.as_str() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::{ResolvedImport, UnresolvedImport};
    use crate::contract::{NodeId, NodeInfo};
    use crate::results::{Severity, Violation};

    const CONTRACT_FILE: &str = "architecture.mmd";

    fn node(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn at(file: &str, line: u32) -> Location {
        Location {
            file: PathBuf::from(file),
            line,
        }
    }

    fn contract(nodes: &[&str], edges: &[(&str, &str, EdgeKind)]) -> Contract {
        Contract {
            nodes: nodes
                .iter()
                .map(|n| {
                    (
                        node(n),
                        NodeInfo {
                            label: None,
                            boundary: None,
                            outer: Vec::new(),
                        },
                    )
                })
                .collect(),
            edges: edges
                .iter()
                .map(|&(from, to, kind)| {
                    (
                        Edge {
                            from: node(from),
                            to: node(to),
                        },
                        kind,
                    )
                })
                .collect(),
            ..Contract::default()
        }
    }

    fn ownership(owners: &[(&str, &str)], unmapped: &[&str]) -> Ownership {
        Ownership {
            owners: owners
                .iter()
                .map(|&(file, owner)| (PathBuf::from(file), node(owner)))
                .collect(),
            unmapped: unmapped.iter().map(PathBuf::from).collect(),
            ..Ownership::default()
        }
    }

    /// Builds imports from (importing file, line, target file) triples.
    fn imports(resolved: &[(&str, u32, &str)]) -> BTreeMap<PathBuf, FileImports> {
        let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
        for &(file, line, target) in resolved {
            map.entry(PathBuf::from(file))
                .or_default()
                .resolved
                .push(ResolvedImport::new(at(file, line), PathBuf::from(target)));
        }
        map
    }

    fn lint(
        contract: &Contract,
        ownership: &Ownership,
        imports: &BTreeMap<PathBuf, FileImports>,
    ) -> Vec<Violation> {
        reconcile(contract, ownership, imports, Path::new(CONTRACT_FILE)).violations
    }

    fn kinds(violations: &[Violation]) -> Vec<ViolationKind> {
        violations.iter().map(|v| v.kind).collect()
    }

    #[test]
    fn empty_inputs_give_an_empty_report() {
        let report = reconcile(
            &Contract::default(),
            &Ownership::default(),
            &BTreeMap::new(),
            Path::new(CONTRACT_FILE),
        );
        assert_eq!(report, LintReport::default());
    }

    #[test]
    fn unknown_tag_is_an_error_at_the_arch_file() {
        let mut own = ownership(&[], &[]);
        own.tags = vec![
            (at("a/.arch", 1), "a".to_owned()),
            (at("b/.arch", 3), "ghost".to_owned()),
        ];
        let found = lint(&contract(&["a"], &[]), &own, &BTreeMap::new());
        assert_eq!(found.len(), 1);
        let v = &found[0];
        assert_eq!(v.kind, ViolationKind::UnknownNode);
        assert_eq!(v.severity, Severity::Error);
        assert_eq!(v.at, Some(at("b/.arch", 3)));
        assert!(v.message.contains("ghost"), "{}", v.message);
        assert!(v.hint.as_deref().unwrap().contains("approved"));
    }

    #[test]
    fn unmapped_file_is_an_error_at_line_one() {
        let own = ownership(&[], &["x/new.ts"]);
        let found = lint(&contract(&["a"], &[]), &own, &BTreeMap::new());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::UnmappedFile);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].at, Some(at("x/new.ts", 1)));
        assert!(found[0].hint.as_deref().unwrap().contains(".arch"));
    }

    #[test]
    fn undeclared_edge_is_reported_at_the_import() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[]),
            &own,
            &imports(&[("a/x.ts", 7, "b/y.ts")]),
        );
        assert_eq!(found.len(), 1);
        let v = &found[0];
        assert_eq!(v.kind, ViolationKind::UndeclaredEdge);
        assert_eq!(v.severity, Severity::Error);
        assert_eq!(v.at, Some(at("a/x.ts", 7)));
        assert_eq!(
            v.message,
            "`a` imports `b` (b/y.ts) but architecture.mmd declares no edge a --> b"
        );
        let hint = v.hint.as_deref().unwrap();
        assert!(hint.contains("approve"), "{hint}");
    }

    #[test]
    fn edge_direction_matters() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("b", "a", EdgeKind::Implemented)]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
        );
        assert_eq!(
            kinds(&found),
            vec![ViolationKind::UndeclaredEdge, ViolationKind::StaleEdge]
        );
    }

    #[test]
    fn solid_realised_edge_is_clean() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Implemented)]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
        );
        assert_eq!(found, Vec::new());
    }

    #[test]
    fn solid_unrealised_edge_is_stale() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Implemented)]),
            &own,
            &BTreeMap::new(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::StaleEdge);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].at, None);
        assert!(found[0].message.contains("a --> b"), "{}", found[0].message);
    }

    #[test]
    fn planned_edge_permits_the_import_and_reports_planned_realized() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Planned)]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::PlannedRealized);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].at, None);
        assert!(found[0].hint.as_deref().unwrap().contains("-->"));
    }

    #[test]
    fn unrealised_planned_edge_is_tolerated() {
        let own = ownership(&[("a/x.ts", "a")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[("a", "b", EdgeKind::Planned)]),
            &own,
            &BTreeMap::new(),
        );
        assert_eq!(found, Vec::new());
    }

    #[test]
    fn unresolved_import_is_a_warning_naming_the_specifier() {
        let mut map: BTreeMap<PathBuf, FileImports> = BTreeMap::new();
        map.entry(PathBuf::from("a/x.ts"))
            .or_default()
            .unresolved
            .push(UnresolvedImport {
                at: at("a/x.ts", 4),
                specifier: "./gone".to_owned(),
            });
        let found = lint(
            &contract(&["a"], &[]),
            &ownership(&[("a/x.ts", "a")], &[]),
            &map,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::UnresolvedImport);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].at, Some(at("a/x.ts", 4)));
        assert!(found[0].message.contains("./gone"));
    }

    #[test]
    fn same_owner_imports_are_ignored() {
        let own = ownership(&[("a/x.ts", "a"), ("a/y.ts", "a")], &[]);
        let found = lint(
            &contract(&["a"], &[]),
            &own,
            &imports(&[("a/x.ts", 1, "a/y.ts")]),
        );
        assert_eq!(found, Vec::new());
    }

    #[test]
    fn unmapped_file_does_not_cascade_into_edge_findings() {
        let own = ownership(&[("b/y.ts", "b")], &["loose/x.ts"]);
        let found = lint(
            &contract(&["b"], &[]),
            &own,
            &imports(&[("loose/x.ts", 1, "b/y.ts"), ("b/y.ts", 2, "loose/x.ts")]),
        );
        assert_eq!(kinds(&found), vec![ViolationKind::UnmappedFile]);
    }

    #[test]
    fn unknown_node_owner_does_not_cascade_into_edge_findings() {
        let mut own = ownership(&[("g/x.ts", "ghost"), ("b/y.ts", "b")], &[]);
        own.tags = vec![(at("g/.arch", 1), "ghost".to_owned())];
        let found = lint(
            &contract(&["b"], &[]),
            &own,
            &imports(&[("g/x.ts", 1, "b/y.ts"), ("b/y.ts", 2, "g/x.ts")]),
        );
        assert_eq!(kinds(&found), vec![ViolationKind::UnknownNode]);
    }

    #[test]
    fn each_import_site_gets_a_finding_but_exact_repeats_collapse() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let found = lint(
            &contract(&["a", "b"], &[]),
            &own,
            &imports(&[
                ("a/x.ts", 1, "b/y.ts"),
                ("a/x.ts", 1, "b/y.ts"),
                ("a/x.ts", 2, "b/y.ts"),
            ]),
        );
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn output_is_sorted_and_independent_of_construction_order() {
        let c = contract(
            &["a", "b", "c"],
            &[
                ("a", "c", EdgeKind::Implemented),
                ("b", "c", EdgeKind::Planned),
            ],
        );
        let owners = [
            ("a/x.ts", "a"),
            ("b/y.ts", "b"),
            ("c/z.ts", "c"),
            ("g/g.ts", "ghost"),
        ];
        let unmapped = ["u/one.ts", "u/two.ts"];
        let tags = [
            (at("g/.arch", 1), "ghost".to_owned()),
            (at("h/.arch", 2), "phantom".to_owned()),
        ];
        let edges = [
            ("a/x.ts", 3, "b/y.ts"),
            ("b/y.ts", 9, "c/z.ts"),
            ("c/z.ts", 5, "a/x.ts"),
            ("b/y.ts", 2, "a/x.ts"),
        ];

        let mut own = ownership(&owners, &unmapped);
        own.tags = tags.to_vec();
        let forward = lint(&c, &own, &imports(&edges));

        let mut reversed_owners = owners;
        reversed_owners.reverse();
        let mut reversed_unmapped = unmapped;
        reversed_unmapped.reverse();
        let mut reversed_edges = edges;
        reversed_edges.reverse();
        let mut own = ownership(&reversed_owners, &reversed_unmapped);
        own.tags = tags.iter().rev().cloned().collect();
        let backward = lint(&c, &own, &imports(&reversed_edges));

        assert_eq!(forward, backward);
        assert!(forward.len() >= 6, "{forward:#?}");
        let mut sorted = forward.clone();
        sorted.sort();
        assert_eq!(forward, sorted);
    }

    #[test]
    fn contract_file_only_names_the_contract_in_messages() {
        let own = ownership(&[("a/x.ts", "a"), ("b/y.ts", "b")], &[]);
        let report = reconcile(
            &contract(&["a", "b"], &[]),
            &own,
            &imports(&[("a/x.ts", 1, "b/y.ts")]),
            Path::new("docs/arch.mmd"),
        );
        assert!(report.violations[0].message.contains("docs/arch.mmd"));
    }

    mod surfaces {
        use super::*;
        use crate::lint::fixtures::{imports_using, with_surface};

        const MAIN: &str = "a/x.ts";
        const LIB: &str = "b/y.ts";

        fn owners() -> Ownership {
            ownership(
                &[(MAIN, "a"), ("a/z.ts", "a"), (LIB, "b"), ("c/w.ts", "c")],
                &[],
            )
        }

        fn ab() -> Contract {
            contract(&["a", "b", "c"], &[("a", "b", EdgeKind::Implemented)])
        }

        fn unlinked() -> Contract {
            contract(&["a", "b", "c"], &[])
        }

        fn private(violations: &[Violation]) -> Vec<&Violation> {
            violations
                .iter()
                .filter(|v| v.kind == ViolationKind::PrivateSymbol)
                .collect()
        }

        #[test]
        fn an_exported_symbol_is_clean() {
            let c = with_surface(ab(), "b", &["Client", "fetch"]);
            let used = imports_using(&[(MAIN, 3, LIB, &["Client", "fetch"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn a_symbol_outside_the_surface_is_an_error_at_the_import() {
            let c = with_surface(ab(), "b", &["fetch", "Client"]);
            let found = lint(
                &c,
                &owners(),
                &imports_using(&[(MAIN, 7, LIB, &["secret"])]),
            );
            assert_eq!(found.len(), 1, "{found:?}");
            let v = &found[0];
            assert_eq!(v.kind, ViolationKind::PrivateSymbol);
            assert_eq!(v.severity, Severity::Error);
            assert_eq!(v.at, Some(at(MAIN, 7)));
            assert_eq!(
                v.message,
                "`a` imports `secret` from `b` (b/y.ts) but architecture.mmd lets `b` export only: Client, fetch"
            );
            let hint = v.hint.as_deref().unwrap();
            assert!(
                hint.contains("exports") && hint.contains("approve"),
                "{hint}"
            );
        }

        #[test]
        fn each_private_symbol_is_its_own_finding_and_allowed_ones_are_not() {
            let c = with_surface(ab(), "b", &["ok"]);
            let used = imports_using(&[(MAIN, 2, LIB, &["zed", "ok", "alpha"])]);
            let found = lint(&c, &owners(), &used);
            let names: Vec<&str> = found
                .iter()
                .map(|v| v.message.split('`').nth(3).unwrap())
                .collect();
            assert_eq!(names, ["alpha", "zed"]);
            assert!(found.iter().all(|v| v.at == Some(at(MAIN, 2))));
        }

        #[test]
        fn default_must_be_listed_like_any_name() {
            let used = imports_using(&[(MAIN, 1, LIB, &["default"])]);
            let c = with_surface(ab(), "b", &["Client"]);
            assert_eq!(private(&lint(&c, &owners(), &used)).len(), 1);
            let c = with_surface(ab(), "b", &["default"]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn a_side_effect_import_uses_nothing_and_is_never_private() {
            let c = with_surface(ab(), "b", &["Client"]);
            let found = lint(&c, &owners(), &imports(&[(MAIN, 1, LIB)]));
            assert_eq!(found, Vec::new());
        }

        #[test]
        fn an_embed_uses_no_symbol_so_a_surface_does_not_restrict_it() {
            let c = with_surface(ab(), "b", &["Client"]);
            let mut used = imports(&[(MAIN, 1, LIB)]);
            for file in used.values_mut() {
                for import in &mut file.resolved {
                    import.kind = crate::code::ImportKind::Embed;
                }
            }
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn a_node_without_a_surface_is_unrestricted() {
            let c = with_surface(ab(), "c", &["Only"]);
            let used = imports_using(&[(MAIN, 1, LIB, &["anything", "*"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn the_surface_of_the_importing_node_does_not_restrict_its_own_imports() {
            let c = with_surface(ab(), "a", &["Only"]);
            let used = imports_using(&[(MAIN, 1, LIB, &["anything"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn imports_within_one_node_are_never_checked() {
            let c = with_surface(unlinked(), "a", &["Only"]);
            let used = imports_using(&[(MAIN, 1, "a/z.ts", &["hidden", "*"])]);
            assert_eq!(lint(&c, &owners(), &used), Vec::new());
        }

        #[test]
        fn test_code_is_not_held_to_a_surface() {
            let own = ownership(&[("a/x.rs", "a"), ("b/y.rs", "b")], &[]);
            let c = with_surface(ab(), "b", &["Client"]);
            let mut used = imports_using(&[("a/x.rs", 4, "b/y.rs", &["*", "hidden"])]);
            for file in used.values_mut() {
                for import in &mut file.resolved {
                    import.test_only = true;
                }
            }
            let found = lint(&c, &own, &used);
            assert!(
                found.iter().all(|v| v.kind != ViolationKind::PrivateSymbol),
                "{found:?}"
            );
        }

        #[test]
        fn star_is_private_in_every_language() {
            for (file, target) in [
                ("a/x.ts", "b/y.ts"),
                ("a/x.py", "b/y.py"),
                ("a/x.js", "b/y.js"),
                ("a/x.rs", "b/y.rs"),
            ] {
                let own = ownership(&[(file, "a"), (target, "b")], &[]);
                let c = with_surface(ab(), "b", &["Client"]);
                let found = lint(&c, &own, &imports_using(&[(file, 4, target, &["*"])]));
                assert_eq!(found.len(), 1, "{file}: {found:?}");
                assert_eq!(found[0].kind, ViolationKind::PrivateSymbol);
                assert!(
                    found[0].message.contains("imports `*` from `b`"),
                    "{}",
                    found[0].message
                );
                assert_eq!(found[0].at, Some(at(file, 4)));
            }
        }

        #[test]
        fn star_stays_private_even_when_names_from_the_same_target_follow() {
            let c = with_surface(ab(), "b", &["Client"]);
            for (file, target) in [("a/x.ts", "b/y.ts"), ("a/x.rs", "b/y.rs")] {
                let own = ownership(&[(file, "a"), (target, "b")], &[]);
                let used =
                    imports_using(&[(file, 1, target, &["*"]), (file, 2, target, &["Client"])]);
                let found = lint(&c, &own, &used);
                assert_eq!(private(&found).len(), 1, "{file}: {found:?}");
                assert_eq!(found[0].at, Some(at(file, 1)));
            }
        }

        #[test]
        fn a_module_binding_uses_no_symbol_and_each_item_through_it_is_checked_on_its_own() {
            const RS_MAIN: &str = "a/x.rs";
            const RS_LIB: &str = "b/y.rs";
            let own = ownership(&[(RS_MAIN, "a"), (RS_LIB, "b")], &[]);
            let c = with_surface(ab(), "b", &["Client"]);
            let clean =
                imports_using(&[(RS_MAIN, 1, RS_LIB, &[]), (RS_MAIN, 5, RS_LIB, &["Client"])]);
            assert_eq!(lint(&c, &own, &clean), Vec::new());

            let leaky =
                imports_using(&[(RS_MAIN, 1, RS_LIB, &[]), (RS_MAIN, 5, RS_LIB, &["secret"])]);
            let found = lint(&c, &own, &leaky);
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].at, Some(at(RS_MAIN, 5)));
            assert!(
                found[0].message.contains("imports `secret`"),
                "{}",
                found[0].message
            );
        }

        #[test]
        fn unmapped_and_unknown_owners_do_not_cascade_into_private_symbols() {
            let c = with_surface(unlinked(), "b", &["Client"]);
            let mut own = ownership(&[(LIB, "b"), ("g/g.ts", "ghost")], &["loose/u.ts"]);
            own.tags = vec![(at("g/.arch", 1), "ghost".to_owned())];
            let used = imports_using(&[
                ("loose/u.ts", 1, LIB, &["secret"]),
                ("g/g.ts", 2, LIB, &["secret"]),
                (LIB, 3, "loose/u.ts", &["secret"]),
                (LIB, 4, "g/g.ts", &["secret"]),
            ]);
            assert_eq!(
                kinds(&lint(&c, &own, &used)),
                vec![ViolationKind::UnmappedFile, ViolationKind::UnknownNode]
            );
        }

        #[test]
        fn a_private_symbol_is_reported_in_addition_to_an_undeclared_edge() {
            let c = with_surface(contract(&["a", "b"], &[]), "b", &["Client"]);
            let used = imports_using(&[(MAIN, 1, LIB, &["secret"])]);
            assert_eq!(
                kinds(&lint(&c, &owners(), &used)),
                vec![ViolationKind::UndeclaredEdge, ViolationKind::PrivateSymbol]
            );
        }

        #[test]
        fn a_planned_edge_still_enforces_the_surface() {
            let c = with_surface(
                contract(&["a", "b"], &[("a", "b", EdgeKind::Planned)]),
                "b",
                &["Client"],
            );
            let used = imports_using(&[(MAIN, 1, LIB, &["secret"])]);
            assert_eq!(
                kinds(&lint(&c, &owners(), &used)),
                vec![ViolationKind::PlannedRealized, ViolationKind::PrivateSymbol]
            );
        }

        #[test]
        fn exact_repeats_collapse_and_each_site_is_reported() {
            let c = with_surface(ab(), "b", &["Client"]);
            let used = imports_using(&[
                (MAIN, 1, LIB, &["secret"]),
                (MAIN, 1, LIB, &["secret"]),
                (MAIN, 2, LIB, &["secret"]),
            ]);
            assert_eq!(private(&lint(&c, &owners(), &used)).len(), 2);
        }

        #[test]
        fn the_contract_file_names_the_contract_in_the_message() {
            let c = with_surface(ab(), "b", &["Client"]);
            let report = reconcile(
                &c,
                &owners(),
                &imports_using(&[(MAIN, 1, LIB, &["secret"])]),
                Path::new("docs/arch.mmd"),
            );
            assert!(report.violations[0].message.contains("docs/arch.mmd"));
        }
    }
}
