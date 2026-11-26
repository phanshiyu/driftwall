//! TypeScript `import` then `export` and `export * as ns from`, followed like `export { X } from`.

use std::path::{Path, PathBuf};

use super::fixtures::repo;
use super::reexport::{site, sites};
use super::*;

const SOURCES: &[(&str, &str)] = &[
    (
        "src/a.ts",
        "export const A = 1;\nexport const Z = 2;\nexport default function d() {}\n",
    ),
    ("src/app.ts", ""),
];

fn with(barrel: &str, app: &str) -> Vec<(&'static str, String)> {
    let mut files: Vec<(&str, String)> = SOURCES
        .iter()
        .map(|(path, text)| (*path, (*text).to_owned()))
        .collect();
    files.push(("src/barrel.ts", barrel.to_owned()));
    files.retain(|(path, _)| *path != "src/app.ts");
    files.push(("src/app.ts", app.to_owned()));
    files
}

fn run(barrel: &str, app: &str) -> Vec<(u32, String, String)> {
    let files = with(barrel, app);
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    sites(&borrowed, "src/app.ts")
}

#[test]
fn an_imported_name_exported_again_is_followed() {
    let got = run(
        "import { A } from './a';\nexport { A };\n",
        "import { A } from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/a.ts", "A")]);
}

#[test]
fn a_renamed_export_follows_the_original_name() {
    let got = run(
        "import { A } from './a';\nexport { A as B };\n",
        "import { B } from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/a.ts", "A")]);
    let got = run(
        "import { A as X } from './a';\nexport { X as B };\n",
        "import { B } from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/a.ts", "A")]);
}

#[test]
fn a_default_import_exported_as_default_is_followed() {
    let got = run(
        "import D from './a';\nexport default D;\n",
        "import X from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/a.ts", "default")]);
}

#[test]
fn a_namespace_import_exported_is_a_use_of_the_whole_module() {
    let got = run(
        "import * as ns from './a';\nexport { ns };\n",
        "import { ns } from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/a.ts", "*")]);
}

#[test]
fn export_star_as_is_a_use_of_the_whole_module() {
    let got = run(
        "export * as ns from './a';\n",
        "import { ns } from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/a.ts", "*")]);
}

#[test]
fn the_barrel_keeps_its_own_edge_to_what_it_imports() {
    let files = with(
        "import { A } from './a';\nexport { A };\n",
        "import { A } from './barrel';\n",
    );
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    assert_eq!(
        sites(&borrowed, "src/barrel.ts"),
        [site(1, "src/a.ts", "A")]
    );
}

#[test]
fn a_name_that_is_defined_here_or_not_imported_stays_on_the_barrel() {
    let got = run(
        "import { A } from './a';\nconst B = A;\nexport { B };\nexport const C = A;\n",
        "import { B, C } from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/barrel.ts", "B,C")]);
}

#[test]
fn an_import_of_a_package_exported_again_is_not_followed() {
    let got = run(
        "import { x } from 'pkg';\nexport { x };\n",
        "import { x } from './barrel';\n",
    );
    assert_eq!(got, [site(1, "src/barrel.ts", "x")]);
}

#[test]
fn a_facade_stops_the_walk_at_the_barrel() {
    let files = with(
        "import { A } from './a';\nexport { A };\n",
        "import { A } from './barrel';\n",
    );
    let repo = repo(
        &files
            .iter()
            .map(|(p, t)| (*p, t.as_str()))
            .collect::<Vec<_>>(),
    );
    let facades: Facades = [(PathBuf::from("src/barrel.ts"), "lib".to_owned())].into();
    let mut all = collect_with_facades(
        repo.root(),
        &repo.files(),
        &repo.assets(),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &facades,
    )
    .unwrap();
    let found = all.remove(Path::new("src/app.ts")).unwrap();
    assert_eq!(found.resolved.len(), 1);
    assert_eq!(
        found.resolved[0].target.display().to_string(),
        "src/barrel.ts"
    );
}
