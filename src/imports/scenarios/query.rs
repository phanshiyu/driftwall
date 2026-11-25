//! The kind of an import written with a bundler query: `?raw`, `?url` and `?inline` of a source
//! file load its text or URL (`embed`), any other query keeps `runtime`.

use std::collections::BTreeSet;

use super::fixtures::repo;
use crate::model::{ImportKind, Symbol};

fn kind_of(specifier: &str) -> ImportKind {
    let r = repo(&[
        ("main.ts", &format!("import x from '{specifier}';")),
        ("w.ts", ""),
    ]);
    let got = r.imports("main.ts");
    assert_eq!(got.resolved.len(), 1, "{specifier}: {got:?}");
    got.resolved[0].kind
}

#[test]
fn raw_url_and_inline_of_a_source_file_are_embeds_without_symbols() {
    for specifier in [
        "./w.ts?raw",
        "./w.ts?url",
        "./w.ts?inline",
        "./w?raw",
        "./w.ts?url&v=2",
        "./w.ts?v=2&raw",
        "./w.ts?raw#frag",
        "./w.ts?raw=1",
    ] {
        assert_eq!(kind_of(specifier), ImportKind::Embed, "{specifier}");
    }
    let r = repo(&[("main.ts", "import x from './w.ts?raw';"), ("w.ts", "")]);
    let got = r.imports("main.ts");
    assert_eq!(got.resolved[0].symbols, BTreeSet::<Symbol>::new());
}

#[test]
fn other_queries_keep_the_import_runtime() {
    for specifier in [
        "./w.ts?worker",
        "./w.ts?sharedworker",
        "./w.ts?worker&inline",
        "./w.ts?inline&worker",
        "./w.ts?v=2",
        "./w.ts?rawish",
        "./w.ts?x=raw",
        "./w.ts#raw",
        "./w.ts",
    ] {
        assert_eq!(kind_of(specifier), ImportKind::Runtime, "{specifier}");
    }
}

#[test]
fn a_runtime_import_keeps_its_symbols_beside_an_embed() {
    let r = repo(&[
        (
            "main.ts",
            "import { a } from './w';\nimport text from './w?raw';",
        ),
        ("w.ts", ""),
    ]);
    let got = r.imports("main.ts");
    let shown: Vec<_> = got
        .resolved
        .iter()
        .map(|i| (i.at.line, i.kind, i.symbols.len()))
        .collect();
    assert_eq!(
        shown,
        [(1, ImportKind::Runtime, 1), (2, ImportKind::Embed, 0)]
    );
}

#[test]
fn an_asset_with_the_query_stays_an_asset_and_a_missing_file_stays_unresolved() {
    let r = repo(&[
        (
            "main.ts",
            "import a from './logo.svg?raw';\nimport b from './gone.ts?raw';",
        ),
        ("logo.svg", ""),
    ]);
    let got = r.imports("main.ts");
    assert_eq!(got.resolved.len(), 1);
    assert_eq!(got.resolved[0].kind, ImportKind::Asset);
    assert_eq!(got.unresolved.len(), 1);
    assert_eq!(got.unresolved[0].specifier, "./gone.ts?raw");
}

#[test]
fn the_query_decides_in_svelte_and_html_too() {
    let r = repo(&[
        (
            "a/App.svelte",
            "<script>\nimport src from './w.ts?raw';\nimport W from './w.ts?worker';\n</script>",
        ),
        (
            "a/index.html",
            "<script type=module>\nimport t from '/w.ts?raw';\nimport u from './w.ts?url';\n</script>",
        ),
        ("a/w.ts", ""),
        ("a/package.json", "{}"),
    ]);
    let kinds = |file: &str| -> Vec<(u32, ImportKind)> {
        r.imports(file)
            .resolved
            .iter()
            .map(|i| (i.at.line, i.kind))
            .collect()
    };
    assert_eq!(
        kinds("a/App.svelte"),
        [(2, ImportKind::Embed), (3, ImportKind::Runtime)]
    );
    assert_eq!(
        kinds("a/index.html"),
        [(2, ImportKind::Embed), (3, ImportKind::Embed)]
    );
}
