//! Tests of aliased specifiers going through [`extract_in`] with a hand-built configuration:
//! probing, fallbacks, and what is ignored.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;
use crate::model::Language;

fn config(base: &str, bare_imports: bool, entries: &[(&str, &[&str])]) -> AliasConfig {
    AliasConfig {
        base: PathBuf::from(base),
        bare_imports,
        patterns: entries
            .iter()
            .filter_map(|(key, targets)| {
                let targets: Vec<String> = targets.iter().map(|t| (*t).to_owned()).collect();
                Pattern::parse(key, &targets)
            })
            .collect(),
    }
}

/// Extracts `source` as `src/app/main.ts` in a repository of `repo`, governed by `config` at the
/// repository root.
fn run(source: &str, repo: &[&str], config: AliasConfig) -> FileImports {
    let known: BTreeSet<PathBuf> = repo.iter().map(PathBuf::from).collect();
    let crates = BTreeMap::new();
    let aliases = BTreeMap::from([(PathBuf::new(), config)]);
    let env = Environment {
        known: &known,
        assets: &BTreeSet::new(),
        unowned: &BTreeSet::new(),
        crates: &crates,
        aliases: &aliases,
        packages: &BTreeMap::new(),
        workspaces: &Workspaces::default(),
        projects: &BTreeMap::new(),
        python: &PythonLayout::default(),
        swift: &SwiftModules::default(),
        vite_roots: &BTreeMap::new(),
        vite_aliases: &BTreeMap::new(),
        test_files: &BTreeSet::new(),
        rust_paths: &PathModules::default(),
        rust_dependencies: &BTreeMap::new(),
    };
    extract_in(
        source,
        Language::TypeScript,
        Path::new("src/app/main.ts"),
        &env,
    )
    .expect("extract succeeds")
}

fn targets(imports: &FileImports) -> Vec<String> {
    imports
        .resolved
        .iter()
        .map(|r| r.target.display().to_string())
        .collect()
}

#[test]
fn simple_alias_resolves_with_extension_probing() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    let got = run(
        "import x from '@/foo/bar';",
        &["src/foo/bar.ts", "src/app/main.ts"],
        c,
    );
    assert_eq!(targets(&got), ["src/foo/bar.ts"]);
    assert_eq!(got.unresolved, []);
}

#[test]
fn alias_resolves_to_a_directory_index() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    let got = run("import x from '@/foo';", &["src/foo/index.tsx"], c);
    assert_eq!(targets(&got), ["src/foo/index.tsx"]);
}

#[test]
fn alias_with_a_js_extension_maps_to_the_ts_file() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    let got = run("import x from '@/foo.js';", &["src/foo.ts"], c);
    assert_eq!(targets(&got), ["src/foo.ts"]);
}

#[test]
fn a_second_target_is_the_fallback_for_a_missing_first() {
    let c = config("", false, &[("@/*", &["./generated/*", "./src/*"])]);
    let got = run("import x from '@/a';", &["src/a.ts"], c.clone());
    assert_eq!(targets(&got), ["src/a.ts"]);
    let both = run("import x from '@/a';", &["src/a.ts", "generated/a.ts"], c);
    assert_eq!(targets(&both), ["generated/a.ts"]);
}

#[test]
fn exact_pattern_resolves() {
    let c = config("", false, &[("config", &["./src/config/index"])]);
    let got = run("import c from 'config';", &["src/config/index.ts"], c);
    assert_eq!(targets(&got), ["src/config/index.ts"]);
}

#[test]
fn longest_prefix_pattern_is_the_only_one_tried() {
    let c = config(
        "",
        false,
        &[("@/*", &["./src/*"]), ("@/components/*", &["./ui/*"])],
    );
    let got = run(
        "import a from '@/components/b';\nimport c from '@/util';",
        &["ui/b.ts", "src/util.ts", "src/components/b.ts"],
        c.clone(),
    );
    assert_eq!(targets(&got), ["ui/b.ts", "src/util.ts"]);
    let only_wide = run(
        "import a from '@/components/b';",
        &["src/components/b.ts"],
        c,
    );
    assert_eq!(only_wide, FileImports::default());
}

#[test]
fn star_in_the_middle_of_a_target() {
    let c = config("", false, &[("~/*", &["./src/*/index"])]);
    let got = run("import a from '~/foo';", &["src/foo/index.ts"], c);
    assert_eq!(targets(&got), ["src/foo/index.ts"]);
}

#[test]
fn bare_import_uses_base_url_after_paths_fail() {
    let c = config("src", true, &[("@/*", &["./nowhere/*"])]);
    let got = run(
        "import a from 'utils/x';\nimport b from '@/y';",
        &["src/utils/x.ts", "src/@/y.ts"],
        c,
    );
    assert_eq!(targets(&got), ["src/utils/x.ts", "src/@/y.ts"]);
}

#[test]
fn a_matched_pattern_with_no_file_is_ignored_not_unresolved() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    let got = run("import a from '@/missing';", &["src/other.ts"], c);
    assert_eq!(got, FileImports::default());
}

#[test]
fn a_specifier_matching_no_pattern_is_not_an_alias_so_it_is_a_package() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    let got = run(
        "import a from 'react';\nimport b from '@scope/pkg';",
        &["src/react.ts"],
        c,
    );
    assert_eq!(got.resolved, Vec::new());
    let names: Vec<&str> = got.skipped.iter().map(|s| s.specifier.as_str()).collect();
    assert_eq!(names, ["react", "@scope/pkg"]);
}

#[test]
fn a_bare_package_name_is_not_an_alias_when_base_url_finds_nothing() {
    let c = config("src", true, &[]);
    let got = run("import a from 'react';", &["src/other.ts"], c);
    assert_eq!(got.resolved, Vec::new());
    assert_eq!(got.skipped.len(), 1);
}

#[test]
fn a_target_outside_the_repository_is_ignored() {
    let c = config("", false, &[("@/*", &["../outside/*", "/abs/*"])]);
    let got = run("import a from '@/x';", &["outside/x.ts", "abs/x.ts"], c);
    assert_eq!(got, FileImports::default());
}

#[test]
fn relative_imports_ignore_aliases_and_still_report_unresolved() {
    let c = config("", true, &[("*", &["./src/*"])]);
    let got = run(
        "import a from './sibling';\nimport b from './missing';",
        &["src/app/sibling.ts", "src/app/missing.ts"],
        c.clone(),
    );
    assert_eq!(targets(&got), ["src/app/sibling.ts", "src/app/missing.ts"]);
    let none = run("import a from './gone';", &["src/gone.ts"], c);
    assert_eq!(none.unresolved.len(), 1);
}

#[test]
fn require_and_dynamic_import_resolve_aliases_too() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    let got = run(
        "const a = require('@/a');\nconst b = import('@/b');\nexport * from '@/c';",
        &["src/a.ts", "src/b.ts", "src/c.ts"],
        c,
    );
    assert_eq!(targets(&got), ["src/a.ts", "src/b.ts", "src/c.ts"]);
}

#[test]
fn files_without_a_governing_config_see_no_aliases_and_the_specifier_is_a_package() {
    let known: BTreeSet<PathBuf> = [PathBuf::from("src/a.ts")].into();
    let crates = BTreeMap::new();
    let aliases = BTreeMap::from([(
        PathBuf::from("other"),
        config("other", false, &[("@/*", &["./src/*"])]),
    )]);
    let env = Environment {
        known: &known,
        assets: &BTreeSet::new(),
        unowned: &BTreeSet::new(),
        crates: &crates,
        aliases: &aliases,
        packages: &BTreeMap::new(),
        workspaces: &Workspaces::default(),
        projects: &BTreeMap::new(),
        python: &PythonLayout::default(),
        swift: &SwiftModules::default(),
        vite_roots: &BTreeMap::new(),
        vite_aliases: &BTreeMap::new(),
        test_files: &BTreeSet::new(),
        rust_paths: &PathModules::default(),
        rust_dependencies: &BTreeMap::new(),
    };
    let got = extract_in(
        "import a from '@/a';",
        Language::TypeScript,
        Path::new("src/main.ts"),
        &env,
    )
    .expect("extract succeeds");
    assert_eq!(got.resolved, Vec::new());
    assert_eq!(got.unresolved, Vec::new());
    assert_eq!(got.skipped.len(), 1);
}
