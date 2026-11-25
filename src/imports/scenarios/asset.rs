//! Assets: imports of existing non-source files, and unresolved imports of missing ones.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::fixtures::{pairs, repo};
use super::*;
use crate::code::{ImportKind, Language, Symbol};

fn kinds(imports: &FileImports) -> Vec<(u32, String, ImportKind, bool)> {
    imports
        .resolved
        .iter()
        .map(|i| {
            (
                i.at.line,
                i.target.display().to_string(),
                i.kind,
                i.test_only,
            )
        })
        .collect()
}

#[test]
fn a_relative_import_of_an_existing_file_is_an_asset_dependency() {
    let r = repo(&[
        (
            "app/main.ts",
            "import data from './data.json';\nimport './style.css';\nimport logo from '../shared/x.svg';",
        ),
        ("app/data.json", "{}"),
        ("app/style.css", ""),
        ("shared/x.svg", ""),
    ]);
    let got = r.imports("app/main.ts");
    assert_eq!(
        kinds(&got),
        [
            (1, "app/data.json".to_owned(), ImportKind::Asset, false),
            (2, "app/style.css".to_owned(), ImportKind::Asset, false),
            (3, "shared/x.svg".to_owned(), ImportKind::Asset, false),
        ]
    );
    assert!(got.resolved.iter().all(|i| i.symbols.is_empty()));
    assert_eq!(r.unresolved("app/main.ts"), pairs(&[]));
}

#[test]
fn an_asset_import_uses_no_symbol_however_it_is_written() {
    let r = repo(&[
        (
            "main.ts",
            "import data, { a } from './data.json';\nimport * as all from './all.json';\nconst x = require('./req.json');",
        ),
        ("data.json", ""),
        ("all.json", ""),
        ("req.json", ""),
    ]);
    let got = r.imports("main.ts");
    assert_eq!(got.resolved.len(), 3);
    for import in &got.resolved {
        assert_eq!(import.kind, ImportKind::Asset);
        assert_eq!(import.symbols, BTreeSet::<Symbol>::new());
    }
}

#[test]
fn an_asset_import_in_a_test_file_or_declaration_file_is_still_an_asset() {
    let r = repo(&[
        ("a.test.ts", "import './d.json';"),
        ("types.d.ts", "import './d.json';"),
        ("d.json", ""),
    ]);
    assert_eq!(
        kinds(&r.imports("a.test.ts")),
        [(1, "d.json".to_owned(), ImportKind::Asset, true)]
    );
    assert_eq!(
        kinds(&r.imports("types.d.ts")),
        [(1, "d.json".to_owned(), ImportKind::Asset, false)]
    );
}

#[test]
fn a_missing_file_of_any_extension_is_unresolved() {
    let r = repo(&[(
        "main.ts",
        "import './missing.json';\nimport './missing.css';\nimport './missing';\nimport './missing.svg?raw';",
    )]);
    assert_eq!(
        r.unresolved("main.ts"),
        pairs(&[
            (1, "./missing.json"),
            (2, "./missing.css"),
            (3, "./missing"),
            (4, "./missing.svg?raw"),
        ])
    );
    assert_eq!(r.edges("main.ts"), pairs(&[]));
}

#[test]
fn a_probe_never_adds_an_asset_extension() {
    let r = repo(&[("main.ts", "import './data';"), ("data.json", "")]);
    assert_eq!(r.unresolved("main.ts"), pairs(&[(1, "./data")]));
}

#[test]
fn a_source_file_wins_over_an_asset_of_the_same_name_stem() {
    let r = repo(&[
        ("main.ts", "import './a';\nimport './a.css';"),
        ("a.ts", ""),
        ("a.css", ""),
    ]);
    assert_eq!(r.edges("main.ts"), pairs(&[(1, "a.ts"), (2, "a.css")]));
}

#[test]
fn query_suffixes_are_stripped_before_an_asset_is_looked_up() {
    let r = repo(&[
        (
            "main.ts",
            "import raw from './logo.svg?raw';\nimport url from './logo.svg?url';\nimport './style.css?inline';",
        ),
        ("logo.svg", ""),
        ("style.css", ""),
    ]);
    assert_eq!(
        r.edges("main.ts"),
        pairs(&[(1, "logo.svg"), (2, "logo.svg"), (3, "style.css")])
    );
    assert_eq!(r.unresolved("main.ts"), pairs(&[]));
}

#[test]
fn an_alias_target_may_be_an_asset() {
    let r = repo(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@assets/*": ["./assets/*"]}}}"#,
        ),
        ("src/main.ts", "import logo from '@assets/logo.svg';"),
        ("assets/logo.svg", ""),
    ]);
    let got = r.imports("src/main.ts");
    assert_eq!(
        kinds(&got),
        [(1, "assets/logo.svg".to_owned(), ImportKind::Asset, false)]
    );
}

#[test]
fn a_package_entry_may_be_an_asset() {
    let r = repo(&[
        ("package.json", r#"{"workspaces": ["packages/*"]}"#),
        (
            "packages/ui/package.json",
            r#"{"name": "ui", "exports": {"./theme.css": "./theme.css"}}"#,
        ),
        ("packages/ui/theme.css", ""),
        ("packages/ui/index.ts", ""),
        ("apps/web/main.ts", "import 'ui/theme.css';"),
    ]);
    let got = r.imports("apps/web/main.ts");
    assert_eq!(
        kinds(&got),
        [(
            1,
            "packages/ui/theme.css".to_owned(),
            ImportKind::Asset,
            false
        )]
    );
}

#[test]
fn collect_without_assets_cannot_resolve_one() {
    let r = repo(&[("main.ts", "import './data.json';"), ("data.json", "")]);
    let files =
        std::collections::BTreeMap::from([(PathBuf::from("main.ts"), Language::TypeScript)]);
    let root = r.root();
    let got = collect(root, &files).expect("collect succeeds");
    let main = got.get(Path::new("main.ts")).expect("main.ts");
    assert_eq!(main.unresolved.len(), 1);
}
