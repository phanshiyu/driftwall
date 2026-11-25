//! Tests of config discovery, `extends` merging and JSONC parsing, run against an in-memory
//! repository so no test touches the filesystem. The tempdir test is in `tests.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;
use crate::imports::ecmascript::{MAX_EXTENDS_DEPTH, discover};
use crate::imports::syntax::paths::directories;
use crate::model::Language;

const NONE: [&str; 0] = [];

/// An in-memory repository: path to contents. Files ending in `.json` are configs, the rest are
/// source files.
fn repo(entries: &[(&str, &str)]) -> BTreeMap<PathBuf, String> {
    entries
        .iter()
        .map(|(path, text)| (PathBuf::from(path), (*text).to_owned()))
        .collect()
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "json")
}

/// The configs found for `files`, looking above every source file and above `importers`.
fn configs_for(
    files: &BTreeMap<PathBuf, String>,
    importers: &[&str],
) -> BTreeMap<PathBuf, AliasConfig> {
    let sources = files
        .keys()
        .filter(|path| !is_json(path))
        .map(PathBuf::as_path)
        .chain(importers.iter().map(Path::new));
    discover(&directories(sources), &|path| files.get(path).cloned())
}

/// The repository files `source`, written in `importer`, resolves to.
fn imports(files: &BTreeMap<PathBuf, String>, importer: &str, source: &str) -> Vec<String> {
    let known: BTreeSet<PathBuf> = files
        .keys()
        .filter(|path| !is_json(path))
        .cloned()
        .collect();
    let aliases = configs_for(files, &[importer]);
    let env = Environment {
        files: Files {
            known: &known,
            assets: &BTreeSet::new(),
            unowned: &BTreeSet::new(),
        },
        test_files: &BTreeSet::new(),
        ecmascript: crate::imports::ecmascript::Env {
            aliases: &aliases,
            packages: &BTreeMap::new(),
            workspaces: &Workspaces::default(),
            projects: &BTreeMap::new(),
            vite_roots: &BTreeMap::new(),
            vite_aliases: &BTreeMap::new(),
        },
        python: &PythonLayout::default(),
        rust: fixtures::no_rust(),
        swift: &SwiftModules::default(),
    };
    let found = extract_in(source, Language::TypeScript, Path::new(importer), &env)
        .expect("extract succeeds");
    assert_eq!(
        found.unresolved,
        [],
        "aliases are never reported unresolved"
    );
    found
        .resolved
        .iter()
        .map(|r| r.target.display().to_string())
        .collect()
}

fn one(files: &[(&str, &str)], importer: &str, specifier: &str) -> Vec<String> {
    imports(
        &repo(files),
        importer,
        &format!("import x from '{specifier}';"),
    )
}

#[test]
fn simple_paths_alias() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("src/foo/bar.ts", ""),
        ("src/main.ts", ""),
    ];
    assert_eq!(one(&files, "src/main.ts", "@/foo/bar"), ["src/foo/bar.ts"]);
}

#[test]
fn second_target_is_the_fallback() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./gen/*","./src/*"]}}}"#,
        ),
        ("src/a.ts", ""),
        ("src/b.ts", ""),
        ("gen/b.ts", ""),
        ("src/main.ts", ""),
    ];
    assert_eq!(one(&files, "src/main.ts", "@/a"), ["src/a.ts"]);
    assert_eq!(one(&files, "src/main.ts", "@/b"), ["gen/b.ts"]);
}

#[test]
fn exact_pattern() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"config":["./src/config/index"]}}}"#,
        ),
        ("src/config/index.ts", ""),
        ("src/config/other.ts", ""),
    ];
    assert_eq!(one(&files, "a.ts", "config"), ["src/config/index.ts"]);
    assert_eq!(one(&files, "a.ts", "config/other"), NONE);
}

#[test]
fn longest_prefix_between_overlapping_patterns() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{
                "@/*": ["./src/*"],
                "@/components/*": ["./ui/*"]
            }}}"#,
        ),
        ("src/util.ts", ""),
        ("src/components/Button.ts", ""),
        ("ui/Button.ts", ""),
    ];
    assert_eq!(one(&files, "a.ts", "@/components/Button"), ["ui/Button.ts"]);
    assert_eq!(one(&files, "a.ts", "@/util"), ["src/util.ts"]);
}

#[test]
fn star_in_the_middle_of_a_target() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"~/*":["./src/*/index"]}}}"#,
        ),
        ("src/foo/index.ts", ""),
    ];
    assert_eq!(one(&files, "a.ts", "~/foo"), ["src/foo/index.ts"]);
}

#[test]
fn base_url_bare_imports() {
    let files = [
        ("tsconfig.json", r#"{"compilerOptions":{"baseUrl":"src"}}"#),
        ("src/utils/x.ts", ""),
        ("src/app/main.ts", ""),
    ];
    assert_eq!(
        one(&files, "src/app/main.ts", "utils/x"),
        ["src/utils/x.ts"]
    );
    assert_eq!(one(&files, "src/app/main.ts", "react"), NONE);
}

#[test]
fn base_url_is_tried_after_paths() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("src/a.ts", ""),
        ("lib/b.ts", ""),
    ];
    assert_eq!(one(&files, "x.ts", "@/a"), ["src/a.ts"]);
    assert_eq!(one(&files, "x.ts", "lib/b"), ["lib/b.ts"]);
}

#[test]
fn paths_without_base_url_are_relative_to_the_config_file() {
    let files = [
        (
            "packages/web/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("packages/web/src/a.ts", ""),
        ("src/a.ts", ""),
    ];
    assert_eq!(
        one(&files, "packages/web/src/main.ts", "@/a"),
        ["packages/web/src/a.ts"]
    );
    assert_eq!(one(&files, "packages/web/src/main.ts", "a"), NONE);
}

#[test]
fn extends_chain_child_paths_replace_the_parents() {
    let files = [
        (
            "tsconfig.base.json",
            r#"{"compilerOptions":{"paths":{"@base/*":["./base/*"],"@/*":["./old/*"]}}}"#,
        ),
        (
            "tsconfig.json",
            r#"{"extends":"./tsconfig.base.json","compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("src/a.ts", ""),
        ("old/a.ts", ""),
        ("base/b.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
    assert_eq!(one(&files, "main.ts", "@base/b"), NONE);
}

#[test]
fn extends_inherits_paths_the_child_does_not_set() {
    let files = [
        (
            "tsconfig.base.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "tsconfig.json",
            r#"{"extends":"./tsconfig.base","compilerOptions":{"strict":true}}"#,
        ),
        ("src/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn extends_chains_through_several_files_and_directories() {
    let files = [
        (
            "configs/root.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./*"]},"baseUrl":"../src"}}"#,
        ),
        ("configs/mid.json", r#"{"extends":"./root"}"#),
        ("app/tsconfig.json", r#"{"extends":"../configs/mid.json"}"#),
        ("src/a.ts", ""),
        ("app/a.ts", ""),
    ];
    assert_eq!(one(&files, "app/main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn base_url_is_relative_to_the_config_that_defines_it() {
    let files = [
        (
            "configs/base.json",
            r#"{"compilerOptions":{"baseUrl":"../src"}}"#,
        ),
        (
            "app/tsconfig.json",
            r#"{"extends":"../configs/base","compilerOptions":{"paths":{"@/*":["./ui/*"]}}}"#,
        ),
        ("src/ui/b.ts", ""),
        ("src/util.ts", ""),
        ("app/ui/b.ts", ""),
    ];
    assert_eq!(one(&files, "app/main.ts", "@/b"), ["src/ui/b.ts"]);
    assert_eq!(one(&files, "app/main.ts", "util"), ["src/util.ts"]);
}

#[test]
fn paths_inherited_without_base_url_stay_relative_to_the_defining_config() {
    let files = [
        (
            "configs/base.json",
            r#"{"compilerOptions":{"paths":{"@/*":["../src/*"]}}}"#,
        ),
        ("app/tsconfig.json", r#"{"extends":"../configs/base.json"}"#),
        ("src/a.ts", ""),
        ("app/src/a.ts", ""),
    ];
    assert_eq!(one(&files, "app/main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn a_cycle_in_extends_keeps_what_was_read() {
    let files = [
        (
            "tsconfig.json",
            r#"{"extends":"./b.json","compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "b.json",
            r#"{"extends":"./tsconfig.json","compilerOptions":{"paths":{"@b/*":["./bb/*"]}}}"#,
        ),
        ("src/a.ts", ""),
        ("bb/c.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
    assert_eq!(one(&files, "main.ts", "@b/c"), NONE);
}

#[test]
fn a_config_extending_itself_terminates() {
    let files = [
        (
            "tsconfig.json",
            r#"{"extends":"./tsconfig.json","compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("src/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn an_extends_chain_is_cut_at_the_depth_limit() {
    let mut files = vec![("src/a.ts".to_owned(), String::new())];
    let names: Vec<String> = (0..=MAX_EXTENDS_DEPTH + 2)
        .map(|index| {
            if index == 0 {
                "tsconfig.json".to_owned()
            } else {
                format!("c{index}.json")
            }
        })
        .collect();
    for (index, name) in names.iter().enumerate() {
        let next = names
            .get(index + 1)
            .map_or(String::new(), |next| format!(r#""extends":"./{next}","#));
        let paths = if index == names.len() - 1 {
            r#""compilerOptions":{"paths":{"@/*":["./src/*"]}}"#
        } else {
            r#""compilerOptions":{}"#
        };
        files.push((name.clone(), format!("{{{next}{paths}}}")));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    assert_eq!(one(&borrowed, "main.ts", "@/a"), NONE);
}

#[test]
fn extends_to_a_package_or_an_array_is_ignored() {
    let own = r#""compilerOptions":{"paths":{"@/*":["./src/*"]}}"#;
    for extends in [
        r#""extends":"@tsconfig/node18/tsconfig.json","#,
        r#""extends":"tsconfig-base","#,
        r#""extends":["./a.json","./b.json"],"#,
        r#""extends":5,"#,
        r#""extends":"./missing","#,
        r#""extends":"../../escapes-the-repo","#,
        r#""extends":"/absolute/tsconfig.json","#,
    ] {
        let config = format!("{{{extends}{own}}}");
        let files = [
            ("tsconfig.json", config.as_str()),
            ("src/a.ts", ""),
            (
                "a.json",
                r#"{"compilerOptions":{"paths":{"@/*":["./wrong/*"]}}}"#,
            ),
        ];
        assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"], "{extends}");
    }
}

#[test]
fn jsonc_comments_trailing_commas_and_slashes_in_strings() {
    let config = r#"
{ // leading comment
  /* block
     comment */
  "description": "see http://example.com // not a comment /* nor this */",
  "compilerOptions": {
    "paths": {
      "@/*": [
        "./src/*", // trailing comma after this
      ],
    },
  },
}
"#;
    let files = [("tsconfig.json", config), ("src/a.ts", "")];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn a_slash_slash_inside_a_path_value_is_kept() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src//dir/*"]}}}"#,
        ),
        ("src/dir/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/dir/a.ts"]);
}

#[test]
fn a_byte_order_mark_is_accepted() {
    let config = "\u{feff}{\"compilerOptions\":{\"paths\":{\"@/*\":[\"./src/*\"]}}}";
    let files = [("tsconfig.json", config), ("src/a.ts", "")];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn solution_style_config_falls_through_to_the_ancestor() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "packages/app/tsconfig.json",
            r#"{"files":[],"references":[{"path":"./tsconfig.app.json"}]}"#,
        ),
        ("src/a.ts", ""),
    ];
    let found = configs_for(&repo(&files), &["packages/app/main.ts"]);
    assert_eq!(found.keys().collect::<Vec<_>>(), [Path::new("")]);
    assert_eq!(one(&files, "packages/app/main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn solution_style_that_extends_a_config_with_paths_is_not_skipped() {
    let files = [
        (
            "tsconfig.base.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "app/tsconfig.json",
            r#"{"extends":"../tsconfig.base.json","files":[],"references":[{"path":"./x"}]}"#,
        ),
        ("src/a.ts", ""),
    ];
    assert_eq!(one(&files, "app/main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn a_config_without_paths_that_is_not_solution_style_hides_the_ancestor() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "lib/tsconfig.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        ),
        ("src/a.ts", ""),
    ];
    assert_eq!(one(&files, "lib/main.ts", "@/a"), NONE);
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn files_without_references_are_not_solution_style() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("lib/tsconfig.json", r#"{"files":[]}"#),
        ("src/a.ts", ""),
    ];
    assert_eq!(one(&files, "lib/main.ts", "@/a"), NONE);
}

#[test]
fn a_nested_config_takes_precedence_for_its_subdirectory() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "packages/web/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./app/*"]}}}"#,
        ),
        ("src/a.ts", ""),
        ("packages/web/app/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
    assert_eq!(
        one(&files, "packages/web/deep/er/main.ts", "@/a"),
        ["packages/web/app/a.ts"]
    );
}

#[test]
fn each_file_uses_the_config_nearest_to_it() {
    let files = [
        (
            "packages/a/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "packages/b/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./lib/*"]}}}"#,
        ),
        ("packages/a/src/x.ts", ""),
        ("packages/b/lib/x.ts", ""),
        ("loose/main.ts", ""),
    ];
    assert_eq!(
        one(&files, "packages/a/main.ts", "@/x"),
        ["packages/a/src/x.ts"]
    );
    assert_eq!(
        one(&files, "packages/b/main.ts", "@/x"),
        ["packages/b/lib/x.ts"]
    );
    assert_eq!(one(&files, "loose/main.ts", "@/x"), NONE);
}

#[test]
fn a_matched_but_missing_target_is_ignored() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("src/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/missing"), NONE);
}

#[test]
fn jsconfig_is_read_when_there_is_no_tsconfig() {
    let files = [
        (
            "jsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("src/a.js", ""),
    ];
    assert_eq!(one(&files, "main.js", "@/a"), ["src/a.js"]);
}

#[test]
fn tsconfig_wins_over_jsconfig_in_one_directory() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./ts/*"]}}}"#,
        ),
        (
            "jsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./js/*"]}}}"#,
        ),
        ("ts/a.ts", ""),
        ("js/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["ts/a.ts"]);
}

#[test]
fn a_target_outside_the_repository_is_ignored() {
    let files = [
        (
            "app/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["../../outside/*","./src/*"]}}}"#,
        ),
        ("app/src/a.ts", ""),
        ("outside/a.ts", ""),
    ];
    assert_eq!(one(&files, "app/main.ts", "@/a"), ["app/src/a.ts"]);
}

#[test]
fn a_base_url_outside_the_repository_gives_no_aliases() {
    let files = [
        (
            "app/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":"../../elsewhere","paths":{"@/*":["./*"]}}}"#,
        ),
        ("app/a.ts", ""),
    ];
    assert_eq!(one(&files, "app/main.ts", "@/a"), NONE);
}

#[test]
fn malformed_configs_are_skipped_and_the_ancestor_applies() {
    let root = r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#;
    for broken in [
        "",
        "{",
        "not json",
        "[]",
        r#"{"compilerOptions":{"paths":"nope"}}"#,
        r#"{"compilerOptions":{"paths":{"@/*":"./x"}}}"#,
        r#"{"compilerOptions":{"baseUrl":5}}"#,
        r#"{"compilerOptions":"x"}"#,
    ] {
        let files = [
            ("tsconfig.json", root),
            ("lib/tsconfig.json", broken),
            ("src/a.ts", ""),
        ];
        assert_eq!(
            one(&files, "lib/main.ts", "@/a"),
            ["src/a.ts"],
            "{broken:?}"
        );
    }
}

#[test]
fn a_broken_tsconfig_does_not_fall_back_to_the_jsconfig_beside_it() {
    let files = [
        ("tsconfig.json", "{"),
        (
            "jsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        ("src/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), NONE);
}

#[test]
fn unknown_fields_and_unusual_values_are_tolerated() {
    let config = r#"{
        "compileOnSave": true,
        "compilerOptions": {"target": "es2020", "lib": ["dom"], "paths": {"@/*": ["./src/*"]}},
        "include": ["src"]
    }"#;
    let files = [("tsconfig.json", config), ("src/a.ts", "")];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn a_repeated_paths_key_keeps_the_last_value() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./old/*"],"@/*":["./src/*"]}}}"#,
        ),
        ("src/a.ts", ""),
        ("old/a.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "@/a"), ["src/a.ts"]);
}

#[test]
fn equal_prefix_patterns_tie_break_in_declaration_order() {
    let files = [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"*.css":["./css/*.css"],"*":["./any/*"]}}}"#,
        ),
        ("css/a.css.ts", ""),
        ("any/a.css.ts", ""),
    ];
    assert_eq!(one(&files, "main.ts", "a.css"), ["css/a.css.ts"]);
}

#[test]
fn discovery_is_deterministic() {
    let files = repo(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"],"@/c/*":["./c/*"]}}}"#,
        ),
        (
            "packages/a/tsconfig.json",
            r#"{"extends":"../../tsconfig"}"#,
        ),
        (
            "packages/b/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":"."}}"#,
        ),
        ("packages/a/src/x.ts", ""),
        ("packages/b/src/x.ts", ""),
    ]);
    let first = configs_for(&files, &[]);
    assert_eq!(first, configs_for(&files, &[]));
    assert_eq!(
        first
            .keys()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>(),
        ["", "packages/a", "packages/b"]
    );
}

#[test]
fn only_directories_with_source_files_below_them_are_considered() {
    let files = repo(&[
        ("tsconfig.json", r#"{"compilerOptions":{"baseUrl":"."}}"#),
        (
            "unrelated/tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":"."}}"#,
        ),
        ("src/a.ts", ""),
    ]);
    let found = configs_for(&files, &[]);
    assert_eq!(found.keys().collect::<Vec<_>>(), [Path::new("")]);
}
