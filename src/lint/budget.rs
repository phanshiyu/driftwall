//! Budgets: the limits of `%% budget` against what a node has (see "Budgets" in `docs/DESIGN.md`).

use std::collections::BTreeMap;
use std::path::Path;

use crate::model::{
    Contract, Edge, InterfaceEntry, Location, Measure, NodeBudget, NodeId, Ownership, Site,
    Violation, ViolationKind,
};

use super::rules::violation;

/// Every node with a `%% budget` line, with its limits and the values it has now.
///
/// Fan-out is the number of observed edges leaving the node with at least one site that is not
/// test-only. Files are the source files `ownership` gives the node, and lines the sum of
/// `line_count` over them, read only for a node whose budget limits lines. The interface is the
/// number of entries `interfaces` holds for the node. Pure given `line_count`.
pub(super) fn measure(
    contract: &Contract,
    ownership: &Ownership,
    observed: &BTreeMap<Edge, Vec<Site>>,
    interfaces: &BTreeMap<NodeId, Vec<InterfaceEntry>>,
    line_count: impl Fn(&Path) -> usize,
) -> BTreeMap<NodeId, NodeBudget> {
    contract
        .rules
        .budgets
        .iter()
        .map(|(node, budget)| {
            let fan_out = || {
                observed
                    .iter()
                    .filter(|(edge, sites)| {
                        edge.from == *node && sites.iter().any(|site| !site.test_only)
                    })
                    .count()
            };
            let owned = || {
                ownership
                    .owners
                    .iter()
                    .filter(|(_, owner)| *owner == node)
                    .map(|(file, _)| file)
            };
            let measure = |limit: Option<usize>, actual: &dyn Fn() -> usize| {
                limit.map(|limit| Measure {
                    limit,
                    actual: actual(),
                })
            };
            let used = NodeBudget {
                line: budget.line,
                fan_out: measure(budget.fan_out, &fan_out),
                files: measure(budget.files, &|| owned().count()),
                lines: measure(budget.lines, &|| owned().map(|file| line_count(file)).sum()),
                interface: measure(budget.interface, &|| {
                    interfaces.get(node).map_or(0, Vec::len)
                }),
            };
            (node.clone(), used)
        })
        .collect()
}

/// One `over-budget` warning per limit a node exceeds, at the node's `%% budget` line.
pub(super) fn findings(
    budgets: &BTreeMap<NodeId, NodeBudget>,
    contract_file: &Path,
) -> Vec<Violation> {
    budgets
        .iter()
        .flat_map(|(node, budget)| {
            budget
                .measures()
                .into_iter()
                .filter(|(_, measure)| measure.over())
                .map(move |(name, measure)| {
                    violation(
                        ViolationKind::OverBudget,
                        Some(Location {
                            file: contract_file.to_path_buf(),
                            line: budget.line,
                        }),
                        format!(
                            "node `{node}` is over its budget: {name} {} exceeds the limit {}",
                            measure.actual, measure.limit
                        ),
                    )
                    .keyed(format!("{node} {name}"))
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::fixtures::{at, edge, node};
    use crate::model::Budget;
    use std::path::PathBuf;

    fn contract(budget: Budget) -> Contract {
        let mut contract = Contract::default();
        contract.rules.budgets.insert(node("a"), budget);
        contract
    }

    fn owned(files: &[(&str, &str)]) -> Ownership {
        Ownership {
            owners: files
                .iter()
                .map(|&(file, owner)| (PathBuf::from(file), node(owner)))
                .collect(),
            ..Ownership::default()
        }
    }

    fn site(test_only: bool) -> Site {
        let mut site = Site::plain(at("a/x.ts", 1));
        site.test_only = test_only;
        site
    }

    fn budget(fan_out: Option<usize>, files: Option<usize>, lines: Option<usize>) -> Budget {
        Budget {
            line: 7,
            fan_out,
            files,
            lines,
            interface: None,
        }
    }

    #[test]
    fn fan_out_counts_distinct_nodes_reached_by_production_sites() {
        let observed = BTreeMap::from([
            (edge("a", "b"), vec![site(false), site(false)]),
            (edge("a", "c"), vec![site(true), site(false)]),
            (edge("a", "d"), vec![site(true)]),
            (edge("b", "a"), vec![site(false)]),
        ]);
        let used = measure(
            &contract(budget(Some(1), None, None)),
            &Ownership::default(),
            &observed,
            &BTreeMap::new(),
            |_| 0,
        );
        let fan_out = used[&node("a")].fan_out.unwrap();
        assert_eq!((fan_out.limit, fan_out.actual), (1, 2));
        assert!(fan_out.over());
    }

    #[test]
    fn files_and_lines_count_the_owned_source_files_only() {
        let ownership = owned(&[("a/x.ts", "a"), ("a/y.ts", "a"), ("b/z.ts", "b")]);
        let used = measure(
            &contract(budget(None, Some(2), Some(100))),
            &ownership,
            &BTreeMap::new(),
            &BTreeMap::new(),
            |file| if file == Path::new("a/x.ts") { 60 } else { 41 },
        );
        let used = &used[&node("a")];
        assert_eq!(used.fan_out, None);
        assert_eq!(
            used.files,
            Some(Measure {
                limit: 2,
                actual: 2
            })
        );
        assert_eq!(
            used.lines,
            Some(Measure {
                limit: 100,
                actual: 101
            })
        );
    }

    #[test]
    fn interface_is_the_entry_count_and_equal_to_the_limit_is_within() {
        let entry = |name: &str| InterfaceEntry {
            file: PathBuf::from("a/x.ts"),
            symbol: Some(name.to_owned()),
            sites: 1,
            users: BTreeMap::new(),
        };
        let interfaces = BTreeMap::from([(node("a"), vec![entry("p"), entry("q")])]);
        let with = |limit| {
            let mut b = budget(None, None, None);
            b.interface = Some(limit);
            let used = measure(
                &contract(b),
                &Ownership::default(),
                &BTreeMap::new(),
                &interfaces,
                |_| 0,
            );
            used[&node("a")].interface.unwrap()
        };
        assert!(!with(2).over());
        assert!(with(1).over());
        assert_eq!(with(1).actual, 2);
    }

    #[test]
    fn lines_are_not_read_without_a_lines_limit() {
        let ownership = owned(&[("a/x.ts", "a")]);
        let reads = std::cell::Cell::new(0_usize);
        let used = measure(
            &contract(budget(None, Some(5), None)),
            &ownership,
            &BTreeMap::new(),
            &BTreeMap::new(),
            |_| {
                reads.set(reads.get() + 1);
                0
            },
        );
        assert_eq!(used[&node("a")].lines, None);
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn only_a_value_above_the_limit_is_a_warning_naming_measure_limit_and_actual() {
        let used = BTreeMap::from([(
            node("a"),
            NodeBudget {
                line: 7,
                fan_out: Some(Measure {
                    limit: 3,
                    actual: 3,
                }),
                files: Some(Measure {
                    limit: 3,
                    actual: 4,
                }),
                lines: Some(Measure {
                    limit: 10,
                    actual: 99,
                }),
                interface: Some(Measure {
                    limit: 5,
                    actual: 9,
                }),
            },
        )]);
        let found = findings(&used, Path::new("c.mmd"));
        let messages: Vec<&str> = found.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "node `a` is over its budget: files 4 exceeds the limit 3",
                "node `a` is over its budget: lines 99 exceeds the limit 10",
                "node `a` is over its budget: interface 9 exceeds the limit 5",
            ]
        );
        assert!(found.iter().all(|v| v.kind == ViolationKind::OverBudget));
        assert_eq!(found[0].at, Some(at("c.mmd", 7)));
        assert_eq!(found[0].severity, crate::model::Severity::Warning);
    }
}
