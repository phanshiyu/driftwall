//! Workspace packages imported by name, and package `imports`, through [`collect`].

use super::fixtures::{pairs, repo};
use crate::code::SkipReason;

const ROOT: (&str, &str) = ("package.json", r#"{"workspaces": ["packages/*"]}"#);
const IMPORTER: &str = "packages/a/src/a.ts";
const BOTH: &str = "import '@x/b';\nimport '@x/b/util';\n";

fn member(manifest: &str) -> (&'static str, &str) {
    ("packages/b/package.json", manifest)
}

#[test]
fn a_member_resolves_through_main_and_a_subpath_file() {
    let r = repo(&[
        ROOT,
        member(r#"{"name": "@x/b", "main": "src/index.ts"}"#),
        ("packages/b/src/index.ts", ""),
        ("packages/b/util.ts", ""),
        (IMPORTER, BOTH),
    ]);
    assert_eq!(
        r.edges(IMPORTER),
        pairs(&[(1, "packages/b/src/index.ts"), (2, "packages/b/util.ts")])
    );
    assert_eq!(r.skipped(IMPORTER), pairs(&[]));
}

#[test]
fn exports_are_honored_by_subpath() {
    let r = repo(&[
        ROOT,
        member(
            r#"{"name": "@x/b", "exports": {".": "./src/index.ts", "./util": "./src/util.ts"}}"#,
        ),
        ("packages/b/src/index.ts", ""),
        ("packages/b/src/util.ts", ""),
        (IMPORTER, BOTH),
    ]);
    assert_eq!(
        r.edges(IMPORTER),
        pairs(&[
            (1, "packages/b/src/index.ts"),
            (2, "packages/b/src/util.ts")
        ])
    );
}

#[test]
fn exports_as_a_string_is_the_root_entry() {
    let r = repo(&[
        ROOT,
        member(r#"{"name": "@x/b", "exports": "./src/index.ts"}"#),
        ("packages/b/src/index.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[(1, "packages/b/src/index.ts")]));
}

#[test]
fn exports_conditions_are_tried_in_the_documented_order() {
    let manifest = r#"{"name": "@x/b", "exports": {
        ".": {"require": "./src/r.ts", "default": "./src/d.ts", "import": "./src/i.ts", "types": "./src/t.d.ts"}
    }}"#;
    let all = [
        ROOT,
        member(manifest),
        ("packages/b/src/t.d.ts", ""),
        ("packages/b/src/i.ts", ""),
        ("packages/b/src/d.ts", ""),
        ("packages/b/src/r.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ];
    let order = [
        "packages/b/src/t.d.ts",
        "packages/b/src/i.ts",
        "packages/b/src/d.ts",
        "packages/b/src/r.ts",
    ];
    for kept in 0..order.len() {
        let files: Vec<(&str, &str)> = all
            .iter()
            .copied()
            .filter(|(path, _)| !order.get(..kept).unwrap_or_default().contains(path))
            .collect();
        let r = repo(&files);
        assert_eq!(
            r.edges(IMPORTER),
            pairs(&[(1, order.get(kept).copied().unwrap_or_default())]),
            "with the first {kept} conditions missing"
        );
    }
}

#[test]
fn a_types_condition_that_is_not_in_the_repository_falls_through() {
    let r = repo(&[
        ROOT,
        member(
            r#"{"name": "@x/b", "exports": {".": {"types": "./dist/index.d.ts", "import": "./src/index.ts"}}}"#,
        ),
        ("packages/b/src/index.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[(1, "packages/b/src/index.ts")]));
}

#[test]
fn nested_conditions_and_arrays_are_followed() {
    let r = repo(&[
        ROOT,
        member(
            r#"{"name": "@x/b", "exports": {".": {"import": {"types": "./nope.d.ts", "default": "./src/esm.ts"}, "default": ["./nope.js", "./src/cjs.ts"]}}}"#,
        ),
        ("packages/b/src/esm.ts", ""),
        ("packages/b/src/cjs.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[(1, "packages/b/src/esm.ts")]));
}

#[test]
fn star_subpath_patterns_substitute_the_matched_text() {
    let r = repo(&[
        ROOT,
        member(
            r#"{"name": "@x/b", "exports": {"./features/*": "./src/features/*/index.ts", "./*": "./src/*.ts"}}"#,
        ),
        ("packages/b/src/features/auth/index.ts", ""),
        ("packages/b/src/util.ts", ""),
        (
            IMPORTER,
            "import '@x/b/features/auth';\nimport '@x/b/util';\nimport '@x/b/missing';",
        ),
    ]);
    assert_eq!(
        r.edges(IMPORTER),
        pairs(&[
            (1, "packages/b/src/features/auth/index.ts"),
            (2, "packages/b/src/util.ts")
        ])
    );
    assert_eq!(r.skipped(IMPORTER), pairs(&[(3, "@x/b/missing")]));
    assert_eq!(r.unresolved(IMPORTER), pairs(&[]));
}

#[test]
fn module_then_main_then_types_then_index_are_probed_for_the_root() {
    let fields = [
        ("module", "src/m.ts"),
        ("main", "src/main.ts"),
        ("types", "src/types.ts"),
    ];
    for skip in 0..=fields.len() {
        let present = fields.get(skip..).unwrap_or_default();
        // why: written in reverse, to show the probe order is not the order of the keys.
        let mut written = vec!["\"name\": \"@x/b\"".to_owned()];
        written.extend(
            present
                .iter()
                .rev()
                .map(|(key, value)| format!("\"{key}\": \"{value}\"")),
        );
        let manifest = format!("{{{}}}", written.join(", "));
        let r = repo(&[
            ROOT,
            member(&manifest),
            ("packages/b/src/m.ts", ""),
            ("packages/b/src/main.ts", ""),
            ("packages/b/src/types.ts", ""),
            ("packages/b/index.ts", ""),
            (IMPORTER, "import '@x/b';"),
        ]);
        let expected = present.first().map_or_else(
            || "packages/b/index.ts".to_owned(),
            |(_, f)| format!("packages/b/{f}"),
        );
        assert_eq!(r.edges(IMPORTER), pairs(&[(1, &expected)]), "{manifest}");
    }
}

#[test]
fn a_main_that_names_a_missing_build_output_falls_back_to_the_index() {
    let r = repo(&[
        ROOT,
        member(r#"{"name": "@x/b", "main": "dist/index.js"}"#),
        ("packages/b/index.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[(1, "packages/b/index.ts")]));
}

#[test]
fn a_member_whose_entry_is_not_in_the_repository_is_skipped_with_a_reason() {
    let r = repo(&[
        ROOT,
        member(r#"{"name": "@x/b", "main": "dist/index.js"}"#),
        ("packages/b/src/other.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[]));
    assert_eq!(r.skipped(IMPORTER), pairs(&[(1, "@x/b")]));
    assert_eq!(r.unresolved(IMPORTER), pairs(&[]));
    let skip = &r.imports(IMPORTER).skipped[0];
    assert_eq!(skip.reason, SkipReason::WorkspaceEntryMissing);
    let message = skip.message.as_deref().unwrap_or_default();
    assert!(message.contains("`@x/b` (`packages/b`)"), "{message}");
    assert!(message.contains("outDir"), "{message}");
}

const OUT_DIR: &str = r#"{"compilerOptions": {"outDir": "dist", "rootDir": "src"}}"#;
const BUILT_IMPORTS: &str = "import '@x/b';\nimport '@x/b/util/deep';";

fn built<'a>(
    manifest: &'a str,
    tsconfig: Option<(&'static str, &'a str)>,
) -> Vec<(&'static str, &'a str)> {
    let mut files = vec![
        ROOT,
        member(manifest),
        ("packages/b/src/index.ts", ""),
        ("packages/b/src/util/deep.ts", ""),
        (IMPORTER, BUILT_IMPORTS),
    ];
    files.extend(tsconfig);
    files
}

const ROOT_EDGE: (u32, &str) = (1, "packages/b/src/index.ts");

#[test]
fn a_build_output_entry_maps_back_to_the_source_through_out_dir_and_root_dir() {
    for manifest in [
        r#"{"name": "@x/b", "main": "dist/index.js", "types": "dist/index.d.ts"}"#,
        r#"{"name": "@x/b", "types": "dist/index.d.ts"}"#,
        r#"{"name": "@x/b", "exports": {".": {"types": "./dist/index.d.ts", "default": "./dist/index.js"}}}"#,
        r#"{"name": "@x/b", "module": "./dist/index.mjs"}"#,
        r#"{"name": "@x/b", "main": "./dist"}"#,
    ] {
        let r = repo(&built(
            manifest,
            Some(("packages/b/tsconfig.json", OUT_DIR)),
        ));
        assert_eq!(
            r.edges(IMPORTER).first(),
            Some(&(ROOT_EDGE.0, ROOT_EDGE.1.to_owned())),
            "{manifest}"
        );
    }
}

#[test]
fn a_sub_path_into_the_build_output_maps_back_too() {
    let manifest =
        r#"{"name": "@x/b", "exports": {".": "./dist/index.js", "./util/*": "./dist/util/*.js"}}"#;
    let r = repo(&built(
        manifest,
        Some(("packages/b/tsconfig.json", OUT_DIR)),
    ));
    assert_eq!(
        r.edges(IMPORTER),
        pairs(&[ROOT_EDGE, (2, "packages/b/src/util/deep.ts")])
    );
}

#[test]
fn out_dir_and_root_dir_are_read_through_extends() {
    let manifest = r#"{"name": "@x/b", "main": "dist/index.js"}"#;
    let r = repo(&[
        ROOT,
        member(manifest),
        ("packages/b/src/index.ts", ""),
        (
            "packages/b/tsconfig.json",
            r#"{"extends": "./tsconfig.shared", "compilerOptions": {"strict": true}}"#,
        ),
        ("packages/b/tsconfig.shared.json", OUT_DIR),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[ROOT_EDGE]));
}

#[test]
fn without_a_root_dir_the_config_directory_and_its_src_are_the_roots() {
    let manifest = r#"{"name": "@x/b", "main": "dist/index.js"}"#;
    let config = r#"{"compilerOptions": {"outDir": "./dist"}}"#;
    let r = repo(&built(manifest, Some(("packages/b/tsconfig.json", config))));
    assert_eq!(
        r.edges(IMPORTER).first(),
        Some(&(ROOT_EDGE.0, ROOT_EDGE.1.to_owned()))
    );
}

#[test]
fn a_build_config_is_read_when_the_tsconfig_has_no_out_dir() {
    let manifest = r#"{"name": "@x/b", "main": "dist/index.js"}"#;
    let mut files = built(manifest, Some(("packages/b/tsconfig.build.json", OUT_DIR)));
    files.push(("packages/b/tsconfig.json", "{}"));
    let r = repo(&files);
    assert_eq!(
        r.edges(IMPORTER).first(),
        Some(&(ROOT_EDGE.0, ROOT_EDGE.1.to_owned()))
    );
}

#[test]
fn a_build_output_with_no_source_behind_it_is_skipped() {
    let manifest = r#"{"name": "@x/b", "main": "dist/index.js"}"#;
    let r = repo(&[
        ROOT,
        member(manifest),
        ("packages/b/tsconfig.json", OUT_DIR),
        ("packages/b/src/other.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[]));
    assert_eq!(r.skipped(IMPORTER), pairs(&[(1, "@x/b")]));
}

const LINKER: &str = "packages/c/src/c.ts";
const LINKER_IMPORTS: &str = "import 'b-alias';\nimport 'b-alias/src/index';";
const B_MANIFEST: &str = r#"{"name": "@x/b", "main": "src/index.ts"}"#;

fn linked_repo(dependencies: &str, extra: &[(&str, &str)]) -> super::fixtures::Repo {
    let manifest = format!(r#"{{"name": "@x/c", {dependencies}}}"#);
    let mut files = vec![
        ROOT,
        ("packages/b/package.json", B_MANIFEST),
        ("packages/b/src/index.ts", ""),
        ("packages/c/package.json", manifest.as_str()),
        (LINKER, LINKER_IMPORTS),
    ];
    files.extend_from_slice(extra);
    repo(&files)
}

#[test]
fn a_dependency_linked_to_a_directory_or_a_member_resolves_to_the_member() {
    for version in [
        "file:../b",
        "link:../b",
        "file:./../b",
        "workspace:@x/b@*",
        "workspace:../b",
        "npm:@x/b@*",
        "npm:@x/b",
    ] {
        let r = linked_repo(
            &format!(r#""dependencies": {{"b-alias": "{version}"}}"#),
            &[],
        );
        assert_eq!(
            r.edges(LINKER),
            pairs(&[
                (1, "packages/b/src/index.ts"),
                (2, "packages/b/src/index.ts")
            ]),
            "{version}"
        );
        assert_eq!(r.skipped(LINKER), pairs(&[]), "{version}");
    }
}

#[test]
fn a_link_in_another_dependency_table_counts() {
    let r = linked_repo(r#""devDependencies": {"b-alias": "link:../b"}"#, &[]);
    assert_eq!(r.edges(LINKER).len(), 2);
}

#[test]
fn a_link_that_points_at_no_package_is_a_skipped_reference() {
    for version in [
        "file:../missing",
        "link:../../outside",
        "workspace:@x/none@*",
    ] {
        let r = linked_repo(
            &format!(r#""dependencies": {{"b-alias": "{version}"}}"#),
            &[],
        );
        assert_eq!(r.edges(LINKER), pairs(&[]), "{version}");
        assert_eq!(
            r.skipped(LINKER),
            pairs(&[(1, "b-alias"), (2, "b-alias/src/index")]),
            "{version}"
        );
        let skip = &r.imports(LINKER).skipped[0];
        assert_eq!(skip.reason, SkipReason::WorkspaceEntryMissing, "{version}");
    }
}

#[test]
fn an_npm_alias_of_a_package_that_is_no_member_is_an_ordinary_package() {
    let r = linked_repo(r#""dependencies": {"b-alias": "npm:react@18"}"#, &[]);
    assert_eq!(r.edges(LINKER), pairs(&[]));
    assert_eq!(r.skipped(LINKER), pairs(&[]));
    assert_eq!(r.imports(LINKER).packages.len(), 2);
}

#[test]
fn a_link_applies_only_below_the_manifest_that_declares_it() {
    let other = [("packages/d/src/d.ts", "import 'b-alias';")];
    let r = linked_repo(r#""dependencies": {"b-alias": "file:../b"}"#, &other);
    assert_eq!(r.edges("packages/d/src/d.ts"), pairs(&[]));
    assert_eq!(r.skipped("packages/d/src/d.ts"), pairs(&[(1, "b-alias")]));
}

#[test]
fn a_member_name_beats_the_undeclared_package_check_and_a_declaration() {
    let r = repo(&[
        ROOT,
        member(r#"{"name": "@x/b", "main": "index.ts"}"#),
        ("packages/b/index.ts", ""),
        (
            "packages/a/package.json",
            r#"{"name": "@x/a", "dependencies": {"@x/b": "*"}}"#,
        ),
        (IMPORTER, "import '@x/b';\nimport '@x/c';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[(1, "packages/b/index.ts")]));
    assert_eq!(r.skipped(IMPORTER), pairs(&[(2, "@x/c")]));
}

#[test]
fn a_package_that_is_not_a_member_is_still_an_undeclared_package() {
    let r = repo(&[
        ROOT,
        ("packages/b/package.json", r#"{"main": "index.ts"}"#),
        ("packages/b/index.ts", ""),
        (
            "other/package.json",
            r#"{"name": "@x/other", "main": "index.ts"}"#,
        ),
        ("other/index.ts", ""),
        (IMPORTER, "import 'b';\nimport '@x/other';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[]));
    assert_eq!(r.skipped(IMPORTER), pairs(&[(1, "b"), (2, "@x/other")]));
}

#[test]
fn workspaces_may_be_an_object_with_packages_and_negations_remove_members() {
    let r = repo(&[
        (
            "package.json",
            r#"{"workspaces": {"packages": ["packages/*", "!packages/c"]}}"#,
        ),
        member(r#"{"name": "@x/b", "main": "index.ts"}"#),
        ("packages/b/index.ts", ""),
        (
            "packages/c/package.json",
            r#"{"name": "@x/c", "main": "index.ts"}"#,
        ),
        ("packages/c/index.ts", ""),
        (IMPORTER, "import '@x/b';\nimport '@x/c';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[(1, "packages/b/index.ts")]));
    assert_eq!(r.skipped(IMPORTER), pairs(&[(2, "@x/c")]));
}

#[test]
fn nested_workspace_roots_and_double_star_patterns_are_discovered() {
    let r = repo(&[
        ("package.json", r#"{"workspaces": ["apps/**"]}"#),
        (
            "apps/web/package.json",
            r#"{"name": "web", "workspaces": ["libs/*"]}"#,
        ),
        (
            "apps/web/libs/ui/package.json",
            r#"{"name": "ui", "main": "index.ts"}"#,
        ),
        ("apps/web/libs/ui/index.ts", ""),
        (
            "apps/deep/er/package.json",
            r#"{"name": "deeper", "main": "index.ts"}"#,
        ),
        ("apps/deep/er/index.ts", ""),
        ("apps/web/src/app.ts", "import 'ui';\nimport 'deeper';"),
    ]);
    assert_eq!(
        r.edges("apps/web/src/app.ts"),
        pairs(&[
            (1, "apps/web/libs/ui/index.ts"),
            (2, "apps/deep/er/index.ts")
        ])
    );
}

#[test]
fn a_malformed_manifest_is_skipped_silently() {
    let r = repo(&[
        ROOT,
        member("{ not json"),
        ("packages/b/index.ts", ""),
        (IMPORTER, "import '@x/b';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[]));
    assert_eq!(r.skipped(IMPORTER), pairs(&[(1, "@x/b")]));
}

#[test]
fn a_root_with_a_main_is_not_a_member_of_itself() {
    let r = repo(&[
        (
            "package.json",
            r#"{"name": "root", "workspaces": ["packages/*"], "main": "index.ts"}"#,
        ),
        ("index.ts", ""),
        (IMPORTER, "import 'root';"),
    ]);
    assert_eq!(r.edges(IMPORTER), pairs(&[]));
}

#[test]
fn package_imports_resolve_through_the_nearest_package_json_that_has_them() {
    let r = repo(&[
        ("package.json", r##"{"imports": {"#far": "./src/far.ts"}}"##),
        (
            "app/package.json",
            r##"{"imports": {"#internal/*": "./src/internal/*.ts", "#cond": {"import": "./src/c.ts"}, "#gone": "./src/gone.ts", "#ext": "lodash"}}"##,
        ),
        ("app/src/internal/db.ts", ""),
        ("app/src/c.ts", ""),
        ("src/far.ts", ""),
        (
            "app/src/main.ts",
            "import '#internal/db';\nimport '#cond';\nimport '#gone';\nimport '#ext';\nimport '#far';\nimport '#nope';",
        ),
    ]);
    assert_eq!(
        r.edges("app/src/main.ts"),
        pairs(&[(1, "app/src/internal/db.ts"), (2, "app/src/c.ts")])
    );
    assert_eq!(
        r.skipped("app/src/main.ts"),
        pairs(&[(5, "#far"), (6, "#nope")])
    );
    assert_eq!(r.unresolved("app/src/main.ts"), pairs(&[]));
}

#[test]
fn a_hash_specifier_without_any_imports_field_is_an_undeclared_package() {
    let r = repo(&[("src/main.ts", "import '#internal/db';")]);
    assert_eq!(r.skipped("src/main.ts"), pairs(&[(1, "#internal/db")]));
}
