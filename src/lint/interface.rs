//! The interface of a node: the symbols of it that code outside uses (see "Budgets" in
//! `docs/DESIGN.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::code::{ImportKind, Ownership, ResolvedImport, Symbol};
use crate::contract::{Contract, NodeId};
use crate::imports::FileImports;
use crate::results::InterfaceEntry;

use super::observe::crossings;

/// What one entry is: the file that defines it and the symbol, `None` for a whole module.
type Key = (PathBuf, Option<String>);

/// The interface of every declared node that code outside uses, sorted by site count (most first),
/// then file and symbol.
///
/// Only production sites count: a test-only import, an embed and an asset do not, and neither
/// does an import within the node. An import's target is already the defining file (re-exports are
/// followed, a facade stops the walk), so an entry is that target and a name. An import that names
/// no symbol, or uses `*`, is one whole-module entry for its target, except a Rust module binding
/// whose importing file also uses a named symbol of the same target: those symbols are the
/// entries. Pure.
#[must_use]
pub(super) fn interfaces(
    contract: &Contract,
    ownership: &Ownership,
    imports: &BTreeMap<PathBuf, FileImports>,
) -> BTreeMap<NodeId, Vec<InterfaceEntry>> {
    let mut found: BTreeMap<NodeId, BTreeMap<Key, InterfaceEntry>> = BTreeMap::new();
    for (edge, import) in crossings(contract, ownership, imports) {
        if import.test_only || !matches!(import.kind, ImportKind::Runtime | ImportKind::TypeOnly) {
            continue;
        }
        let entries = found.entry(edge.to).or_default();
        for symbol in used(import, imports) {
            let entry = entries
                .entry((import.target.clone(), symbol.clone()))
                .or_insert_with(|| InterfaceEntry {
                    file: import.target.clone(),
                    symbol,
                    sites: 0,
                    users: BTreeMap::new(),
                });
            entry.sites += 1;
            *entry.users.entry(edge.from.clone()).or_default() += 1;
        }
    }
    found
        .into_iter()
        .map(|(node, entries)| {
            let mut entries: Vec<InterfaceEntry> = entries.into_values().collect();
            entries.sort_by(|a, b| {
                b.sites
                    .cmp(&a.sites)
                    .then_with(|| a.file.cmp(&b.file))
                    .then_with(|| a.symbol.cmp(&b.symbol))
            });
            (node, entries)
        })
        .collect()
}

/// The entries `import` uses: each named symbol, and `None` once when it also uses the module as
/// a whole.
fn used(import: &ResolvedImport, imports: &BTreeMap<PathBuf, FileImports>) -> Vec<Option<String>> {
    let mut entries: Vec<Option<String>> = import
        .symbols
        .iter()
        .filter_map(|symbol| match symbol {
            Symbol::Name(name) => Some(Some(name.clone())),
            Symbol::All => None,
        })
        .collect();
    let whole = import.symbols.is_empty() || import.symbols.contains(&Symbol::All);
    if whole && !covered_binding(import, imports) {
        entries.push(None);
    }
    entries
}

/// Whether `import` is a Rust module binding (`use crate::store;`) whose importing file also uses
/// a named symbol of the same target in production code.
// why: a Rust module binding records no symbol, and the items reached through it are recorded at
// why: their own sites, so counting the binding as well would count the module twice.
fn covered_binding(import: &ResolvedImport, imports: &BTreeMap<PathBuf, FileImports>) -> bool {
    let rust = Path::new(&import.at.file)
        .extension()
        .is_some_and(|ext| ext == "rs");
    rust && import.symbols.is_empty()
        && imports.get(&import.at.file).is_some_and(|file| {
            file.resolved.iter().any(|other| {
                other.target == import.target
                    && !other.test_only
                    && other
                        .symbols
                        .iter()
                        .any(|symbol| matches!(symbol, Symbol::Name(_)))
            })
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::lint::fixtures::{contract, imports_using, node, ownership};

    fn owners() -> Ownership {
        ownership(
            &[
                ("store/db.ts", "store"),
                ("store/cache.ts", "store"),
                ("store/index.ts", "store"),
                ("billing/a.ts", "billing"),
                ("billing/b.ts", "billing"),
                ("api/c.ts", "api"),
            ],
            &[],
        )
    }

    fn labels(entries: &[InterfaceEntry]) -> BTreeSet<String> {
        entries
            .iter()
            .map(|e| {
                format!(
                    "{} {}",
                    e.file.display(),
                    e.symbol.as_deref().unwrap_or("(whole module)")
                )
            })
            .collect()
    }

    fn store_of(
        sites: &[(&str, u32, &str, &[&str])],
        tweak: impl Fn(&mut BTreeMap<PathBuf, FileImports>),
    ) -> Vec<InterfaceEntry> {
        let mut imports = imports_using(sites);
        tweak(&mut imports);
        let all = interfaces(
            &contract(&["store", "billing", "api"], &[]),
            &owners(),
            &imports,
        );
        all.get(&node("store")).cloned().unwrap_or_default()
    }

    #[test]
    fn distinct_symbols_across_importers_count_once_each() {
        let entries = store_of(
            &[
                ("billing/a.ts", 1, "store/db.ts", &["open", "close"]),
                ("billing/b.ts", 2, "store/db.ts", &["open"]),
                ("api/c.ts", 3, "store/db.ts", &["open"]),
                ("api/c.ts", 4, "store/cache.ts", &["open"]),
            ],
            |_| {},
        );
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].symbol.as_deref(), Some("open"));
        assert_eq!(entries[0].sites, 3);
        assert_eq!(entries[0].users[&node("billing")], 2);
        assert_eq!(entries[0].users[&node("api")], 1);
        assert_eq!(entries[1].file, PathBuf::from("store/cache.ts"));
        assert_eq!(entries[2].symbol.as_deref(), Some("close"));
    }

    #[test]
    fn internal_and_test_only_sites_do_not_count() {
        let entries = store_of(
            &[
                ("store/index.ts", 1, "store/db.ts", &["open"]),
                ("billing/a.ts", 2, "store/db.ts", &["close"]),
            ],
            |imports| {
                if let Some(file) = imports.get_mut(Path::new("billing/a.ts")) {
                    file.resolved[0].test_only = true;
                }
            },
        );
        assert!(entries.is_empty(), "{entries:?}");
    }

    #[test]
    fn the_target_is_the_definer_so_a_followed_reexport_and_a_facade_differ_only_by_target() {
        // why: re-exports are followed, or stopped at a facade, before this runs: the import's
        // why: target is what that walk decided, and the interface trusts it.
        let followed = store_of(&[("billing/a.ts", 1, "store/db.ts", &["open"])], |_| {});
        let at_facade = store_of(&[("billing/a.ts", 1, "store/index.ts", &["open"])], |_| {});
        assert_eq!(
            labels(&followed),
            BTreeSet::from(["store/db.ts open".to_owned()])
        );
        assert_eq!(
            labels(&at_facade),
            BTreeSet::from(["store/index.ts open".to_owned()])
        );
    }

    #[test]
    fn a_whole_module_import_is_one_entry_per_file() {
        let entries = store_of(
            &[
                ("billing/a.ts", 1, "store/db.ts", &["*"]),
                ("billing/b.ts", 2, "store/db.ts", &[]),
                ("api/c.ts", 3, "store/cache.ts", &["get"]),
            ],
            |_| {},
        );
        assert_eq!(
            labels(&entries),
            BTreeSet::from([
                "store/cache.ts get".to_owned(),
                "store/db.ts (whole module)".to_owned()
            ])
        );
        let whole = entries.iter().find(|e| e.symbol.is_none());
        assert_eq!(whole.map(|e| e.sites), Some(2));
    }

    #[test]
    fn a_rust_module_binding_is_covered_by_the_items_used_through_it() {
        let owners = ownership(
            &[
                ("store/db.rs", "store"),
                ("billing/a.rs", "billing"),
                ("billing/b.rs", "billing"),
            ],
            &[],
        );
        let imports = imports_using(&[
            ("billing/a.rs", 1, "store/db.rs", &[]),
            ("billing/a.rs", 5, "store/db.rs", &["open"]),
            ("billing/b.rs", 1, "store/db.rs", &[]),
        ]);
        let all = interfaces(&contract(&["store", "billing"], &[]), &owners, &imports);
        assert_eq!(
            labels(&all[&node("store")]),
            BTreeSet::from([
                "store/db.rs (whole module)".to_owned(),
                "store/db.rs open".to_owned()
            ])
        );
        let sites: usize = all[&node("store")].iter().map(|e| e.sites).sum();
        assert_eq!(sites, 2);
    }
}
