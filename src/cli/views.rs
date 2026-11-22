//! `driftwall brief` and `driftwall neighbors`: read-only maps of the architecture.

use std::path::PathBuf;

use anyhow::{Result, bail};
use driftwall::lint::{self, LintOptions};
use driftwall::model::Overview;
use driftwall::report;

use crate::args::{BriefArgs, NeighborsArgs, ViewFormat};
use crate::commands::{Run, lint_options, require_directory};
use crate::outcome::Outcome;

/// Describes the whole architecture of the repository at `args.path`.
///
/// # Errors
/// A root that is not a directory, or any failure of the scan.
pub fn brief(args: &BriefArgs) -> Result<Run> {
    let overview = scan(&args.path, args.contract.as_ref())?;
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
    let overview = scan(&args.path, args.contract.as_ref())?;
    let Some(view) = lint::node_view(&overview, &args.node) else {
        bail!(
            "unknown node `{}`; declared nodes: {}",
            args.node,
            declared_nodes(&overview)
        );
    };
    let stdout = match args.format {
        ViewFormat::Text => {
            let options = report::NodeViewOptions {
                all_sites: args.all_sites,
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

/// Scans `path` once and summarises it.
fn scan(path: &std::path::Path, contract: Option<&PathBuf>) -> Result<Overview> {
    require_directory(path)?;
    let options: LintOptions = lint_options(contract);
    let observation = lint::observe(path, &options)?;
    Ok(lint::overview(
        &observation.contract,
        &observation.ownership,
        &observation.observed,
        &observation.symbols,
        &observation.report,
    ))
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
    use driftwall::model::{NodeId, NodeSummary};

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
                    exports: None,
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
