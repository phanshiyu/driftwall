//! Solution-style `tsconfig.json` files and the projects they reference, through [`collect`].

use super::fixtures::{pairs, repo};

const SOLUTION: (&str, &str) = (
    "tsconfig.json",
    r#"{"files": [], "references": [{"path": "./tsconfig.app.json"}, {"path": "./tsconfig.node.json"}]}"#,
);
const APP: (&str, &str) = (
    "tsconfig.app.json",
    r#"{"compilerOptions": {"baseUrl": ".", "paths": {"@/*": ["./src/*"]}}, "include": ["src"]}"#,
);
const NODE: (&str, &str) = (
    "tsconfig.node.json",
    r#"{"compilerOptions": {}, "include": ["vite.config.ts"]}"#,
);

#[test]
fn the_vite_template_resolves_paths_defined_in_the_referenced_app_config() {
    let r = repo(&[
        SOLUTION,
        APP,
        NODE,
        ("src/main.ts", "import '@/b/b';"),
        ("src/b/b.ts", ""),
    ]);
    assert_eq!(r.edges("src/main.ts"), pairs(&[(1, "src/b/b.ts")]));
    assert_eq!(r.skipped("src/main.ts"), pairs(&[]));
}

#[test]
fn a_file_only_the_other_project_includes_gets_that_projects_aliases() {
    let r = repo(&[
        SOLUTION,
        APP,
        NODE,
        ("vite.config.ts", "import '@/b/b';"),
        ("src/b/b.ts", ""),
    ]);
    assert_eq!(r.edges("vite.config.ts"), pairs(&[]));
    assert_eq!(r.skipped("vite.config.ts"), pairs(&[(1, "@/b/b")]));
}

#[test]
fn without_a_matching_reference_the_nearest_config_above_applies() {
    let r = repo(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@/*": ["./web/src/*"]}}}"#,
        ),
        (
            "web/tsconfig.json",
            r#"{"files": [], "references": [{"path": "./tsconfig.app.json"}]}"#,
        ),
        (
            "web/tsconfig.app.json",
            r#"{"compilerOptions": {"paths": {"@/*": ["./nowhere/*"]}}, "include": ["src"]}"#,
        ),
        ("web/scripts/tool.ts", "import '@/b';"),
        ("web/src/b.ts", ""),
    ]);
    assert_eq!(
        r.edges("web/scripts/tool.ts"),
        pairs(&[(1, "web/src/b.ts")])
    );
}

#[test]
fn the_first_matching_reference_in_order_wins() {
    let files = |first: &'static str, second: &'static str| {
        vec![
            (
                "tsconfig.json",
                r#"{"files": [], "references": [{"path": "./a.json"}, {"path": "./b.json"}]}"#,
            ),
            ("a.json", first),
            ("b.json", second),
            ("src/main.ts", "import '@/x';"),
            ("one/x.ts", ""),
            ("two/x.ts", ""),
        ]
    };
    let one = r#"{"compilerOptions": {"paths": {"@/*": ["./one/*"]}}, "include": ["src"]}"#;
    let two = r#"{"compilerOptions": {"paths": {"@/*": ["./two/*"]}}, "include": ["src"]}"#;
    assert_eq!(
        repo(&files(one, two)).edges("src/main.ts"),
        pairs(&[(1, "one/x.ts")])
    );
    assert_eq!(
        repo(&files(two, one)).edges("src/main.ts"),
        pairs(&[(1, "two/x.ts")])
    );
}

fn with_include(include: &str, file: &str) -> Vec<(u32, String)> {
    let config = format!(
        r#"{{"compilerOptions": {{"paths": {{"@/*": ["./lib/*"]}}}}, "include": {include}}}"#
    );
    let r = repo(&[
        (
            "tsconfig.json",
            r#"{"files": [], "references": [{"path": "./app.json"}]}"#,
        ),
        ("app.json", &config),
        (file, "import '@/x';"),
        ("lib/x.ts", ""),
    ]);
    r.edges(file)
}

#[test]
fn include_patterns_use_double_star_star_and_plain_directories() {
    let hit = pairs(&[(1, "lib/x.ts")]);
    let miss = pairs(&[]);
    assert_eq!(with_include(r#"["src"]"#, "src/a/b/c.ts"), hit);
    assert_eq!(with_include(r#"["./src/"]"#, "src/a.ts"), hit);
    assert_eq!(with_include(r#"["src/**/*"]"#, "src/a/b/c.ts"), hit);
    assert_eq!(with_include(r#"["src/**/*.ts"]"#, "src/c.ts"), hit);
    assert_eq!(with_include(r#"["src/*"]"#, "src/c.ts"), hit);
    assert_eq!(with_include(r#"["src/*"]"#, "src/a/c.ts"), miss);
    assert_eq!(with_include(r#"["**/*.test.ts"]"#, "src/a/c.test.ts"), hit);
    assert_eq!(with_include(r#"["**/*.test.ts"]"#, "src/a/c.ts"), miss);
    assert_eq!(with_include(r#"["lib", "src"]"#, "src/a.ts"), hit);
    assert_eq!(with_include(r#"["srcs"]"#, "src/a.ts"), miss);
    assert_eq!(with_include(r"[]", "src/a.ts"), miss);
}

#[test]
fn exclude_removes_files_from_include_and_files_adds_them_back() {
    let config = r#"{"compilerOptions": {"paths": {"@/*": ["./lib/*"]}}, "include": ["src"], "exclude": ["src/legacy", "**/*.test.ts"], "files": ["src/legacy/keep.ts"]}"#;
    let r = repo(&[
        (
            "tsconfig.json",
            r#"{"files": [], "references": [{"path": "./app.json"}]}"#,
        ),
        ("app.json", config),
        ("src/a.ts", "import '@/x';"),
        ("src/a.test.ts", "import '@/x';"),
        ("src/legacy/old.ts", "import '@/x';"),
        ("src/legacy/keep.ts", "import '@/x';"),
        ("lib/x.ts", ""),
    ]);
    let hit = pairs(&[(1, "lib/x.ts")]);
    assert_eq!(r.edges("src/a.ts"), hit);
    assert_eq!(r.edges("src/legacy/keep.ts"), hit);
    assert_eq!(r.edges("src/a.test.ts"), pairs(&[]));
    assert_eq!(r.edges("src/legacy/old.ts"), pairs(&[]));
}

#[test]
fn a_config_with_neither_files_nor_include_includes_everything_below_it() {
    let r = repo(&[
        (
            "tsconfig.json",
            r#"{"files": [], "references": [{"path": "./app"}]}"#,
        ),
        (
            "app/tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
        ),
        ("app/src/a.ts", "import '@/b';"),
        ("app/src/b.ts", ""),
        ("other/c.ts", "import '@/b';"),
    ]);
    assert_eq!(r.edges("app/src/a.ts"), pairs(&[(1, "app/src/b.ts")]));
    assert_eq!(r.edges("other/c.ts"), pairs(&[]));
}

#[test]
fn include_is_inherited_through_extends_relative_to_the_file_that_sets_it() {
    let r = repo(&[
        (
            "tsconfig.json",
            r#"{"files": [], "references": [{"path": "./configs/web.json"}]}"#,
        ),
        (
            "configs/base/base.json",
            r#"{"include": ["../../web/src"]}"#,
        ),
        (
            "configs/web.json",
            r#"{"extends": "./base/base.json", "compilerOptions": {"paths": {"@/*": ["../web/src/*"]}}}"#,
        ),
        ("web/src/a.ts", "import '@/b';"),
        ("web/src/b.ts", ""),
        ("web/other/c.ts", "import '@/b';"),
    ]);
    assert_eq!(r.edges("web/src/a.ts"), pairs(&[(1, "web/src/b.ts")]));
    assert_eq!(r.edges("web/other/c.ts"), pairs(&[]));
}

#[test]
fn missing_malformed_and_nested_solution_references_are_skipped() {
    let r = repo(&[
        (
            "tsconfig.json",
            r#"{"files": [], "references": [{"path": "./missing.json"}, {"path": "./broken.json"}, {"path": "./nested.json"}, {"path": "./app.json"}]}"#,
        ),
        ("broken.json", "{ nope"),
        (
            "nested.json",
            r#"{"files": [], "references": [{"path": "./app.json"}]}"#,
        ),
        (
            "app.json",
            r#"{"compilerOptions": {"paths": {"@/*": ["./lib/*"]}}, "include": ["src"]}"#,
        ),
        ("src/a.ts", "import '@/x';"),
        ("lib/x.ts", ""),
    ]);
    assert_eq!(r.edges("src/a.ts"), pairs(&[(1, "lib/x.ts")]));
}

#[test]
fn a_nearer_non_solution_config_is_not_replaced_by_the_solution_above_it() {
    let r = repo(&[
        SOLUTION,
        APP,
        NODE,
        (
            "src/sub/tsconfig.json",
            r#"{"compilerOptions": {"baseUrl": ".", "paths": {"~/*": ["./*"]}}}"#,
        ),
        ("src/sub/a.ts", "import '~/b';\nimport '@/b/b';"),
        ("src/sub/b.ts", ""),
        ("src/b/b.ts", ""),
    ]);
    assert_eq!(r.edges("src/sub/a.ts"), pairs(&[(1, "src/sub/b.ts")]));
}
