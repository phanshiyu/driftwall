//! Isolated boundaries: the children of a `%% isolated` boundary may not use each other (see
//! "Isolated boundaries" in `docs/DESIGN.md`).

use std::path::Path;

use crate::model::{
    Contract, Edge, ResolvedImport, Violation, ViolationKind, children, children_apart,
};

use super::rules::{site_violation, violation};

/// One `isolation-breach` per import site that goes from a node in one child of an isolated
/// boundary to a node in a different child, and one `stale-isolation` per isolated boundary with
/// fewer than two children. Nothing without `%% isolated`.
///
/// Every kind of site counts, test-only ones included, and no arrow excuses one: such an arrow is
/// a contract error, so none exists.
pub(super) fn findings(
    contract: &Contract,
    crossings: &[(Edge, &ResolvedImport)],
    contract_file: &Path,
) -> Vec<Violation> {
    let mut found = Vec::new();
    for (boundary, line) in &contract.rules.isolated {
        let count = children(&contract.nodes, boundary).len();
        if count < 2 {
            found.push(violation(
                ViolationKind::StaleIsolation,
                None,
                format!(
                    "`%% isolated` on line {line} of {} names `{boundary}`, which has {} so there is nothing to keep apart",
                    contract_file.display(),
                    if count == 0 {
                        "no child".to_owned()
                    } else {
                        "one child".to_owned()
                    },
                ),
            ));
        }
        for (edge, import) in crossings {
            let Some((first, second)) =
                children_apart(&contract.nodes, boundary, &edge.from, &edge.to)
            else {
                continue;
            };
            found.push(site_violation(
                ViolationKind::IsolationBreach,
                import,
                format!(
                    "boundary `{boundary}` is isolated (`%% isolated` on line {line}): `{from}` (in `{first}`) imports `{to}` (in `{second}`, {}), and `{first}` and `{second}` may not depend on each other",
                    import.target.display(),
                    from = edge.from,
                    to = edge.to,
                ),
            ));
        }
    }
    found
}
