//! Test isolation: production code may not import test files (see "Test isolation" in
//! `docs/DESIGN.md`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::contract::Contract;
use crate::reading::imports::FileImports;
use crate::results::{Violation, ViolationKind, path_key};

use super::rules::violation;

/// One `production-imports-test` finding per resolved import site that is not test-only and whose
/// target is a test file. Nothing without `%% tests: isolated`.
///
/// Only files that were read can be test files, so an import of an asset or of a file the scan left
/// out is never a finding. Pairs, edges and owners play no part: this is a rule about files.
pub(super) fn findings(
    contract: &Contract,
    imports: &BTreeMap<PathBuf, FileImports>,
) -> Vec<Violation> {
    let Some(rule_line) = contract.rules.tests_isolated else {
        return Vec::new();
    };
    imports
        .values()
        .flat_map(|file_imports| &file_imports.resolved)
        .filter(|import| !import.test_only)
        .filter(|import| {
            imports
                .get(&import.target)
                .is_some_and(|target| target.test_file)
        })
        .map(|import| {
            violation(
                ViolationKind::ProductionImportsTest,
                Some(import.at.clone()),
                format!(
                    "production file `{}` imports test file `{}`, but `%% tests: isolated` on line {rule_line} keeps test code out of production",
                    import.at.file.display(),
                    import.target.display(),
                ),
            )
            .keyed(format!(
                "{} --> {}",
                path_key(&import.at.file),
                path_key(&import.target)
            ))
            .targeting(&import.target)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::ResolvedImport;
    use crate::lint::fixtures::at;

    fn isolated() -> Contract {
        let mut contract = Contract::default();
        contract.rules.tests_isolated = Some(12);
        contract
    }

    /// Files by (path, is a test file), and imports as (from, to, test-only).
    fn found(
        contract: &Contract,
        files: &[(&str, bool)],
        imports: &[(&str, &str, bool)],
    ) -> Vec<Violation> {
        let mut map: BTreeMap<PathBuf, FileImports> = files
            .iter()
            .map(|&(file, test_file)| {
                (
                    PathBuf::from(file),
                    FileImports {
                        test_file,
                        ..FileImports::default()
                    },
                )
            })
            .collect();
        for &(from, to, test_only) in imports {
            map.entry(PathBuf::from(from)).or_default().resolved.push(
                ResolvedImport::new(at(from, 4), PathBuf::from(to)).with_test_only(test_only),
            );
        }
        findings(contract, &map)
    }

    const FILES: [(&str, bool); 3] = [("a/x.ts", false), ("a/y.ts", false), ("a/x.test.ts", true)];

    #[test]
    fn a_production_import_of_a_test_file_is_an_error_at_the_site() {
        let found = found(&isolated(), &FILES, &[("a/y.ts", "a/x.test.ts", false)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ViolationKind::ProductionImportsTest);
        assert_eq!(found[0].severity, crate::results::Severity::Error);
        assert_eq!(found[0].at, Some(at("a/y.ts", 4)));
        assert!(
            found[0]
                .message
                .contains("`a/y.ts` imports test file `a/x.test.ts`")
        );
        assert!(found[0].message.contains("line 12"), "{}", found[0].message);
    }

    #[test]
    fn test_sites_and_imports_of_production_files_are_fine() {
        let imports = [
            ("a/x.test.ts", "a/y.ts", false),
            ("a/y.ts", "a/x.test.ts", true),
            ("a/y.ts", "a/x.ts", false),
        ];
        assert_eq!(found(&isolated(), &FILES, &imports), Vec::new());
    }

    #[test]
    fn nothing_is_checked_without_the_rule_and_unread_targets_are_never_test_files() {
        let imports = [("a/y.ts", "a/x.test.ts", false)];
        assert_eq!(found(&Contract::default(), &FILES, &imports), Vec::new());
        let imports = [("a/y.ts", "a/fixture.json", false)];
        assert_eq!(found(&isolated(), &FILES, &imports), Vec::new());
    }
}
