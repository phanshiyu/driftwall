//! `driftwall brief` and `driftwall neighbors`: read-only maps of the architecture.

use std::path::PathBuf;

use anyhow::{Result, bail};
use driftwall::lint::{self, LintOptions, Observation};
use driftwall::report;
use driftwall::results::Overview;
use driftwall::views;

use crate::args::{BriefArgs, NeighborsArgs, ViewFormat};
use crate::commands::{Run, lint_options, require_directory};
use crate::outcome::Outcome;

/// Describes the whole architecture of the repository at `args.path`.
///
/// # Errors
/// A root that is not a directory, or any failure of the scan.
pub fn brief(args: &BriefArgs) -> Result<Run> {
    let (overview, _) = scan(&args.path, args.contract.as_ref())?;
    let stdout = match args.format {
        ViewFormat::Text => report::render_overview_text(&overview),
        ViewFormat::Json => report::render_overview_json(&overview)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::Clean,
    })
}

/// Describes one node of the repository at `args.path` and the edges that touch it.
///
/// # Errors
/// A node the contract does not declare (the message lists the declared ones), a root that is
/// not a directory, or any failure of the scan.
pub fn neighbors(args: &NeighborsArgs) -> Result<Run> {
    let (overview, observation) = scan(&args.path, args.contract.as_ref())?;
    let Some(view) = views::node_view(
        &overview,
        &observation.ownership,
        &observation.blind_spots,
        &args.node,
    ) else {
        bail!(
            "unknown node `{}`; declared nodes: {}",
            args.node,
            declared_nodes(&overview)
        );
    };
    // why: the interface block is long for a wide node, so it shows only for a node whose contract
    // why: limits it, or when every site is asked for.
    let wanted = args.all_sites || view.node.budget.is_some_and(|b| b.interface.is_some());
    let view = if wanted {
        let none = Vec::new();
        let entries = observation.interfaces.get(&view.node.id).unwrap_or(&none);
        views::with_interface(view, entries)
    } else {
        view
    };
    let stdout = match args.format {
        ViewFormat::Text => {
            let options = report::NodeViewOptions {
                all_sites: args.all_sites,
                files: args.files,
            };
            report::render_node_view_text(&view, options)
        }
        ViewFormat::Json => report::render_node_view_json(&view)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::Clean,
    })
}

/// Scans `path` once and summarises it, keeping the observation the summary came from.
fn scan(path: &std::path::Path, contract: Option<&PathBuf>) -> Result<(Overview, Observation)> {
    require_directory(path)?;
    let options: LintOptions = lint_options(contract);
    let observation = lint::observe(path, &options)?;
    let overview = views::overview(
        &observation.contract,
        &observation.ownership,
        &observation.observed,
        &observation.symbols,
        &observation.report,
        &observation.blind_spots,
    );
    let mut overview = views::with_packages(overview, &observation.packages);
    views::attach_budgets(&mut overview, &observation.budgets);
    Ok((overview, observation))
}

/// The declared node ids, comma separated, or `(none)`.
fn declared_nodes(overview: &Overview) -> String {
    if overview.nodes.is_empty() {
        return "(none)".to_owned();
    }
    overview
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use driftwall::contract::NodeId;
    use driftwall::results::{BlindTally, NodeSummary};

    use super::*;

    fn overview(ids: &[&str]) -> Overview {
        Overview {
            nodes: ids
                .iter()
                .map(|id| NodeSummary {
                    id: NodeId::new(id).unwrap(),
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
                })
                .collect(),
            ..Overview::default()
        }
    }

    #[test]
    fn declared_nodes_are_listed_in_order() {
        assert_eq!(declared_nodes(&overview(&["a", "b", "c"])), "a, b, c");
    }

    #[test]
    fn no_declared_nodes_say_none() {
        assert_eq!(declared_nodes(&overview(&[])), "(none)");
    }
}
