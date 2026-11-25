use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::model::Location;

/// A library crate at `src/` with module files in both layouts, plus a nested binary crate.
const REPO: &[&str] = &[
    "src/lib.rs",
    "src/model/mod.rs",
    "src/diagram/mod.rs",
    "src/lint/mod.rs",
    "src/lint/rules.rs",
    "src/lint/tests.rs",
    "src/util.rs",
    "src/util/inner.rs",
    "src/cli/main.rs",
    "src/cli/args.rs",
];

fn known(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

fn crates(pairs: &[(&str, &str)]) -> BTreeMap<String, PathBuf> {
    pairs
        .iter()
        .map(|(name, dir)| ((*name).to_owned(), PathBuf::from(dir)))
        .collect()
}

fn rs_in(
    source: &str,
    file: &str,
    repo: &[&str],
    library: &BTreeMap<String, PathBuf>,
) -> FileImports {
    let known = known(repo);
    let env = Environment {
        files: Files {
            known: &known,
            assets: &BTreeSet::new(),
            unowned: &BTreeSet::new(),
        },
        test_files: &BTreeSet::new(),
        ecmascript: fixtures::no_ecmascript(),
        python: &PythonLayout::default(),
        rust: crate::imports::rust::Env {
            crates: library,
            paths: &PathModules::default(),
            dependencies: &BTreeMap::new(),
        },
        swift: &SwiftModules::default(),
    };
    extract_in(source, Language::Rust, Path::new(file), &env).expect("extract succeeds")
}

/// Extracts `source` as if it were `file` in [`REPO`], with the library crate `driftwall`.
fn rs_at(source: &str, file: &str) -> FileImports {
    rs_in(source, file, REPO, &crates(&[("driftwall", "src")]))
}

fn rs(source: &str) -> FileImports {
    rs_at(source, "src/lint/rules.rs")
}

fn targets(imports: &FileImports) -> Vec<String> {
    imports
        .resolved
        .iter()
        .map(|r| r.target.display().to_string())
        .collect()
}

fn lines(imports: &FileImports) -> Vec<u32> {
    imports.resolved.iter().map(|r| r.at.line).collect()
}

fn assert_targets(source: &str, expected: &[&str]) {
    let got = rs(source);
    assert_eq!(targets(&got), expected, "source: {source}");
    assert_eq!(got.unresolved, Vec::new());
}

#[test]
fn simple_use() {
    assert_targets("use crate::model::Contract;", &["src/model/mod.rs"]);
}

#[test]
fn use_group_across_modules() {
    assert_targets(
        "use crate::{model::Edge, diagram::Parser};",
        &["src/diagram/mod.rs", "src/model/mod.rs"],
    );
}

#[test]
fn use_group_of_items_in_one_module() {
    let got = rs("use crate::model::{Contract, Edge};");
    assert_eq!(targets(&got), ["src/model/mod.rs"]);
}

#[test]
fn use_nested_groups() {
    assert_targets(
        "use crate::{model::{Contract, Edge}, util::{inner::Thing, Other}};",
        &["src/model/mod.rs", "src/util/inner.rs", "src/util.rs"],
    );
}

#[test]
fn use_self_in_group() {
    assert_targets("use crate::model::{self, Edge};", &["src/model/mod.rs"]);
    assert_targets("use crate::diagram::{self};", &["src/diagram/mod.rs"]);
}

#[test]
fn use_self_in_group_names_the_prefix_not_a_sibling() {
    // why: `crate::util::{self}` is util.rs, while `crate::util::{inner}` is the file util/inner.rs.
    assert_targets("use crate::util::{self};", &["src/util.rs"]);
    assert_targets("use crate::util::{inner};", &["src/util/inner.rs"]);
}

#[test]
fn use_alias() {
    assert_targets("use crate::model::Contract as C;", &["src/model/mod.rs"]);
    assert_targets("use crate::model as m;", &["src/model/mod.rs"]);
    assert_targets(
        "use crate::model::{self as m, Edge as E};",
        &["src/model/mod.rs"],
    );
}

#[test]
fn use_glob() {
    assert_targets("use crate::model::*;", &["src/model/mod.rs"]);
    assert_targets(
        "use crate::{diagram::*, model::*};",
        &["src/diagram/mod.rs", "src/model/mod.rs"],
    );
}

#[test]
fn use_pub_and_restricted_visibility() {
    assert_targets("pub use crate::model::Edge;", &["src/model/mod.rs"]);
    assert_targets("pub(crate) use crate::model::Edge;", &["src/model/mod.rs"]);
    assert_targets("pub(super) use crate::model::Edge;", &["src/model/mod.rs"]);
}

#[test]
fn use_with_several_nesting_levels() {
    assert_targets(
        "use crate::{util::{self, inner::{A, B}}, model};",
        &["src/model/mod.rs", "src/util/inner.rs", "src/util.rs"],
    );
}

#[test]
fn use_group_without_a_leading_path() {
    assert_targets(
        "use {crate::model::Edge, super::Thing};",
        &["src/lint/mod.rs", "src/model/mod.rs"],
    );
}

#[test]
fn use_of_std_and_third_party_is_ignored() {
    assert_targets(
        "use std::collections::BTreeMap;\nuse tree_sitter::{Node, Parser};\nuse serde::*;\nuse anyhow;",
        &[],
    );
}

#[test]
fn use_of_a_bare_local_name_is_ignored() {
    assert_targets("use rules::Thing;\nuse Thing;", &[]);
}

#[test]
fn qualified_path_in_expression() {
    assert_targets(
        "fn f() { crate::diagram::parse(src); }",
        &["src/diagram/mod.rs"],
    );
}

#[test]
fn qualified_path_in_types() {
    let source = "fn f(a: crate::model::Edge, b: Vec<crate::diagram::Node<u8>>) -> Option<crate::util::T> {}";
    assert_targets(
        source,
        &["src/diagram/mod.rs", "src/model/mod.rs", "src/util.rs"],
    );
}

#[test]
fn qualified_path_in_struct_literal() {
    assert_targets(
        "fn f() { let e = crate::model::Edge { from: a, to: b }; }",
        &["src/model/mod.rs"],
    );
}

#[test]
fn qualified_path_in_macro_position() {
    assert_targets(
        "fn f() { crate::diagram::render!(x); }",
        &["src/diagram/mod.rs"],
    );
}

#[test]
fn qualified_path_inside_macro_arguments() {
    let source =
        "fn f() { assert_eq!(crate::model::make(), self::missing::x()); vec![super::item()]; }";
    assert_targets(source, &["src/lint/mod.rs", "src/model/mod.rs"]);
}

#[test]
fn qualified_path_in_derive_and_attribute() {
    assert_targets(
        "#[derive(crate::model::Derive)]\n#[crate::diagram::attr]\nstruct S;",
        &["src/model/mod.rs", "src/diagram/mod.rs"],
    );
}

#[test]
fn qualified_path_in_patterns() {
    let source =
        "fn f(x: E) { match x { crate::model::E::A(v) => {}, crate::diagram::S { .. } => {} } }";
    assert_targets(source, &["src/diagram/mod.rs", "src/model/mod.rs"]);
}

#[test]
fn qualified_path_in_trait_positions() {
    let source = "impl crate::model::Tr for S {}\nfn f<T: crate::diagram::Bound>() { <T as crate::util::Other>::go(); }";
    assert_targets(
        source,
        &["src/model/mod.rs", "src/diagram/mod.rs", "src/util.rs"],
    );
}

#[test]
fn qualified_path_with_generic_arguments() {
    assert_targets(
        "fn f() { crate::model::Edge::<u8>::new(); }",
        &["src/model/mod.rs"],
    );
}

#[test]
fn only_the_outermost_path_counts() {
    // why: `crate::util` alone would add util.rs, but the path continues into util/inner.rs.
    assert_targets(
        "fn f() { crate::util::inner::go(); }",
        &["src/util/inner.rs"],
    );
    assert_targets(
        "fn f() { crate::util::inner::Thing::new(); }",
        &["src/util/inner.rs"],
    );
}

#[test]
fn std_and_third_party_paths_are_ignored() {
    assert_targets(
        "fn f() { std::fs::read(p); tree_sitter::Parser::new(); let x: serde_json::Value; }",
        &[],
    );
}

#[test]
fn plain_relative_names_are_ignored() {
    assert_targets("fn f() { rules::check(); Self::new(); Vec::new(); }", &[]);
}

#[test]
fn visibility_paths_are_not_dependencies() {
    assert_targets("pub(in crate::model) struct S;", &[]);
}

#[test]
fn mod_declarations_are_never_emitted() {
    let got = rs_at(
        "mod rules;\npub mod tests;\nmod missing;",
        "src/lint/mod.rs",
    );
    assert_eq!(got, FileImports::default());
}

#[test]
fn inline_test_module_with_use_super_is_a_self_import() {
    let source = "fn f() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n";
    assert_eq!(rs(source), FileImports::default());
}

#[test]
fn use_super_in_an_inline_module_of_a_mod_rs_file() {
    let source = "mod tests {\n    use super::*;\n    use super::Thing;\n}\n";
    assert_eq!(rs_at(source, "src/lint/mod.rs"), FileImports::default());
}

#[test]
fn super_inside_nested_inline_modules_climbs_one_level_each() {
    let source = "mod a {\n    mod b {\n        use super::super::*;\n    }\n}\n";
    assert_eq!(rs(source), FileImports::default());
    let got = rs_at(
        "mod a {\n    mod b {\n        use super::super::super::*;\n    }\n}\n",
        "src/lint/rules.rs",
    );
    assert_eq!(targets(&got), ["src/lint/mod.rs"]);
}

#[test]
fn self_inside_an_inline_module_names_the_inline_module() {
    // why: `self::sub` inside inline `mod outer` is `lint::rules::outer::sub`, which is rules.rs.
    let source = "mod outer {\n    use self::sub::X;\n    use super::sibling::Y;\n}\n";
    assert_eq!(rs(source), FileImports::default());
}

#[test]
fn super_from_a_nested_file() {
    let got = rs_at(
        "use super::Thing;\nuse super::rules::Other;\n",
        "src/lint/tests.rs",
    );
    assert_eq!(targets(&got), ["src/lint/mod.rs", "src/lint/rules.rs"]);
}

#[test]
fn super_from_a_file_directly_under_the_root_is_the_root() {
    let got = rs_at("use super::Thing;", "src/util.rs");
    assert_eq!(targets(&got), ["src/lib.rs"]);
}

#[test]
fn super_past_the_crate_root_is_ignored() {
    assert_eq!(
        rs_at("use super::Thing;", "src/lib.rs"),
        FileImports::default()
    );
    assert_eq!(
        rs_at("use super::super::Thing;", "src/util.rs"),
        FileImports::default()
    );
}

#[test]
fn self_relative_paths() {
    let got = rs_at(
        "use self::rules::Thing;\nfn f() { self::rules::go(); }",
        "src/lint/mod.rs",
    );
    assert_eq!(targets(&got), ["src/lint/rules.rs", "src/lint/rules.rs"]);
    let got = rs_at("use self::inner::Thing;", "src/util.rs");
    assert_eq!(targets(&got), ["src/util/inner.rs"]);
}

#[test]
fn self_reference_to_the_own_file_is_dropped() {
    assert_eq!(
        rs("use self::Thing;\nuse crate::lint::rules::Thing;\nfn f() { self::go(); }"),
        FileImports::default()
    );
}

#[test]
fn longest_prefix_drops_trailing_items() {
    assert_targets("use crate::model::Contract;", &["src/model/mod.rs"]);
    assert_targets(
        "use crate::util::inner::Thing::Variant;",
        &["src/util/inner.rs"],
    );
    assert_targets("use crate::util::Thing::Variant;", &["src/util.rs"]);
}

#[test]
fn path_to_an_unknown_module_falls_back_to_the_deepest_known_one() {
    assert_targets("use crate::util::inline::deep::Thing;", &["src/util.rs"]);
}

#[test]
fn crate_root_item_resolves_to_the_root_file() {
    assert_targets("use crate::Foo;", &["src/lib.rs"]);
    assert_targets("fn f() { crate::bar(); }", &["src/lib.rs"]);
    let got = rs_at("use crate::Foo;", "src/cli/args.rs");
    assert_eq!(targets(&got), ["src/cli/main.rs"]);
}

#[test]
fn both_module_layouts_resolve() {
    assert_targets("use crate::util::Thing;", &["src/util.rs"]);
    assert_targets("use crate::model::Thing;", &["src/model/mod.rs"]);
    assert_targets("use crate::lint::Thing;", &["src/lint/mod.rs"]);
}

#[test]
fn file_layout_wins_over_mod_rs_when_both_exist() {
    let repo = ["src/lib.rs", "src/a.rs", "src/a/mod.rs", "src/b.rs"];
    let got = rs_in("use crate::a::X;", "src/b.rs", &repo, &BTreeMap::new());
    assert_eq!(targets(&got), ["src/a.rs"]);
}

#[test]
fn nested_binary_is_its_own_crate() {
    let got = rs_at("use crate::args::Args;\nmod args;", "src/cli/main.rs");
    assert_eq!(targets(&got), ["src/cli/args.rs"]);
    // why: in the library crate the same path is `src/args.rs`, which does not exist.
    let got = rs_at("use crate::args::Args;", "src/lint/mod.rs");
    assert_eq!(targets(&got), ["src/lib.rs"]);
}

#[test]
fn files_of_a_nested_binary_use_its_root() {
    let got = rs_at("use super::Cli;", "src/cli/args.rs");
    assert_eq!(targets(&got), ["src/cli/main.rs"]);
    let got = rs_at("use crate::Other;", "src/cli/args.rs");
    assert_eq!(targets(&got), ["src/cli/main.rs"]);
}

#[test]
fn external_crate_resolves_like_crate_against_its_root() {
    let got = rs_at(
        "use driftwall::lint;\nuse driftwall::model::{Edge, Contract};\nfn f() { driftwall::lint::run(p); }",
        "src/cli/main.rs",
    );
    assert_eq!(
        targets(&got),
        ["src/lint/mod.rs", "src/model/mod.rs", "src/lint/mod.rs"]
    );
    assert_eq!(lines(&got), [1, 2, 3]);
}

#[test]
fn external_crate_root_item_resolves_to_its_lib_rs() {
    let got = rs_at("use driftwall::Thing;", "src/cli/main.rs");
    assert_eq!(targets(&got), ["src/lib.rs"]);
}

#[test]
fn external_crate_names_come_from_the_environment_only() {
    let got = rs_in(
        "use driftwall::lint;",
        "src/cli/main.rs",
        REPO,
        &BTreeMap::new(),
    );
    assert_eq!(got, FileImports::default());
    let got = rs_in(
        "use other_name::model;",
        "src/cli/main.rs",
        REPO,
        &crates(&[("other_name", "src")]),
    );
    assert_eq!(targets(&got), ["src/model/mod.rs"]);
}

#[test]
fn external_crate_in_a_subdirectory_workspace() {
    let repo = [
        "crates/core/src/lib.rs",
        "crates/core/src/graph.rs",
        "crates/app/src/main.rs",
        "crates/app/src/ui.rs",
    ];
    let library = crates(&[("my_core", "crates/core/src")]);
    let got = rs_in(
        "use my_core::graph::Node;\nuse crate::ui::Panel;\nmod ui;",
        "crates/app/src/main.rs",
        &repo,
        &library,
    );
    assert_eq!(
        targets(&got),
        ["crates/core/src/graph.rs", "crates/app/src/ui.rs"]
    );
}

#[test]
fn crate_at_the_repository_root() {
    let repo = ["lib.rs", "a.rs"];
    let got = rs_in("use crate::a::X;", "lib.rs", &repo, &BTreeMap::new());
    assert_eq!(targets(&got), ["a.rs"]);
    let got = rs_in("use crate::Y;", "a.rs", &repo, &BTreeMap::new());
    assert_eq!(targets(&got), ["lib.rs"]);
}

#[test]
fn a_directory_with_lib_and_main_gives_other_files_the_library_root() {
    let repo = ["src/lib.rs", "src/main.rs", "src/a.rs"];
    let got = rs_in("use crate::X;", "src/a.rs", &repo, &BTreeMap::new());
    assert_eq!(targets(&got), ["src/lib.rs"]);
    let got = rs_in("use crate::a::X;", "src/main.rs", &repo, &BTreeMap::new());
    assert_eq!(targets(&got), ["src/a.rs"]);
}

#[test]
fn a_file_outside_any_crate_follows_only_external_crates() {
    let repo = ["tests/style.rs", "src/lib.rs", "src/model/mod.rs"];
    let library = crates(&[("driftwall", "src")]);
    let got = rs_in(
        "use crate::x::Y;\nuse super::Z;\nuse driftwall::model::Edge;",
        "tests/style.rs",
        &repo,
        &library,
    );
    assert_eq!(targets(&got), ["src/model/mod.rs"]);
}

#[test]
fn raw_identifiers_name_module_files_without_the_prefix() {
    let repo = ["src/lib.rs", "src/type.rs"];
    let got = rs_in(
        "use crate::r#type::X;",
        "src/lib.rs",
        &repo,
        &BTreeMap::new(),
    );
    assert_eq!(targets(&got), ["src/type.rs"]);
}

#[test]
fn duplicates_collapse() {
    let source = "use crate::model::{Contract, Edge, Node};\nuse crate::model::*;";
    let got = rs(source);
    assert_eq!(targets(&got), ["src/model/mod.rs", "src/model/mod.rs"]);
    assert_eq!(lines(&got), [1, 2]);
    let one_line = rs("use crate::model::{self, Edge, Node as N, a::*};");
    assert_eq!(one_line.resolved.len(), 1);
    let twice = rs("fn f() { crate::model::A::new(); crate::model::A::new(); }");
    assert_eq!(twice.resolved.len(), 1);
}

#[test]
fn lines_are_one_indexed() {
    let source = "// first\nuse crate::model::Edge;\n\nfn f() {\n    crate::diagram::go();\n}\n";
    let got = rs(source);
    assert_eq!(targets(&got), ["src/model/mod.rs", "src/diagram/mod.rs"]);
    assert_eq!(lines(&got), [2, 5]);
    assert_eq!(
        got.resolved.first().map(|r| r.at.clone()),
        Some(Location {
            file: PathBuf::from("src/lint/rules.rs"),
            line: 2
        })
    );
}

#[test]
fn a_multi_line_use_reports_the_line_it_starts_on() {
    let got = rs("fn x() {}\nuse crate::{\n    model::Edge,\n    diagram::Node,\n};\n");
    assert_eq!(targets(&got), ["src/diagram/mod.rs", "src/model/mod.rs"]);
    assert_eq!(lines(&got), [2, 2]);
}

#[test]
fn a_use_reports_its_own_line_not_the_line_of_its_path() {
    let got = rs(
        "#[allow(unused)]\npub use\n    crate::model::Edge;\npub(crate)\nuse\n    crate::diagram::Node;\n",
    );
    assert_eq!(targets(&got), ["src/model/mod.rs", "src/diagram/mod.rs"]);
    assert_eq!(lines(&got), [2, 4]);
}

#[test]
fn a_path_in_a_multi_line_macro_reports_its_own_line() {
    let got =
        rs("fn f() {\n    println!(\n        \"{}\",\n        crate::model::show(),\n    );\n}\n");
    assert_eq!(lines(&got), [4]);
}

#[test]
fn output_is_sorted_and_free_of_duplicates() {
    let got = rs("use crate::util::A;\nuse crate::model::B;\n");
    let mut sorted = got.resolved.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(got.resolved, sorted);
}

#[test]
fn syntax_errors_still_yield_valid_imports() {
    let source =
        "use crate::model::Edge;\nfn broken( {{ let = ;\nuse crate::diagram::Node;\nstruct";
    let got = rs(source);
    assert_eq!(targets(&got), ["src/model/mod.rs", "src/diagram/mod.rs"]);
}

#[test]
fn pathological_input_does_not_panic() {
    for source in [
        "",
        "use",
        "use ;",
        "use ::",
        "use {};",
        "use crate::{};",
        "use crate::{self};",
        "use self;",
        "use *;",
        "use ::std::fs;",
        "use ::driftwall::model;",
        "fn f() { <crate::a::B>::c(); ::crate::x(); }",
        "mod a { mod b { use super::super::super::super::*; } }",
        "macro_rules! m { () => { $crate::a::b() }; }",
        "crate::",
        "fn f() { crate::::x(); }",
        "\u{0}\u{1}",
    ] {
        let got = rs(source);
        assert_eq!(got.unresolved, Vec::new(), "source: {source}");
    }
}

#[test]
fn leading_double_colon_paths_are_not_followed() {
    assert_targets(
        "use ::driftwall::model::Edge;\nfn f() { ::driftwall::lint::run(); }",
        &[],
    );
}

#[test]
fn macro_definitions_with_dollar_crate_are_not_dependencies() {
    assert_targets("macro_rules! m { () => { $crate::model::make() }; }", &[]);
}

#[test]
fn library_crates_are_discovered_from_manifests() {
    let dir = tempfile::tempdir().expect("tempdir");
    let write = |path: &str, text: &str| {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(full, text).expect("write");
    };
    write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    write(
        "crates/core/Cargo.toml",
        "[package]\nname = \"my-core\"\nversion = \"0.1.0\"\n",
    );
    write("crates/core/src/lib.rs", "");
    write(
        "crates/named/Cargo.toml",
        "[package]\nname = \"pkg-name\"\n[lib]\nname = \"lib-name\"\n",
    );
    write("crates/named/src/lib.rs", "");
    write("crates/broken/Cargo.toml", "[package\nname = ");
    write("crates/broken/src/lib.rs", "");
    write("crates/none/src/lib.rs", "");
    write(
        "crates/pathy/Cargo.toml",
        "[package]\nname = \"pathy\"\n[lib]\npath = \"other.rs\"\n",
    );
    write("crates/pathy/src/lib.rs", "");
    write("crates/app/Cargo.toml", "[package]\nname = \"app\"\n");
    write("crates/app/src/main.rs", "");
    let files = [
        "crates/core/src/lib.rs",
        "crates/named/src/lib.rs",
        "crates/broken/src/lib.rs",
        "crates/none/src/lib.rs",
        "crates/pathy/src/lib.rs",
        "crates/app/src/main.rs",
    ];
    let found = library_crates(dir.path(), &known(&files));
    assert_eq!(
        found,
        crates(&[
            ("lib_name", "crates/named/src"),
            ("my_core", "crates/core/src")
        ])
    );
}

#[test]
fn collect_follows_a_hyphenated_library_crate_from_a_binary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let write = |path: &str, text: &str| {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(full, text).expect("write");
    };
    write(
        "Cargo.toml",
        "[package]\nname = \"my-tool\"\nversion = \"0.1.0\"\n[[bin]]\nname = \"my-tool\"\npath = \"src/cli/main.rs\"\n",
    );
    write("src/lib.rs", "pub mod lint;\n");
    write("src/lint/mod.rs", "pub fn run() {}\n");
    write(
        "src/cli/main.rs",
        "use my_tool::lint;\nfn main() { my_tool::lint::run(); }\n",
    );
    let files: BTreeMap<PathBuf, Language> = ["src/lib.rs", "src/lint/mod.rs", "src/cli/main.rs"]
        .iter()
        .map(|path| (PathBuf::from(path), Language::Rust))
        .collect();
    let all = collect(dir.path(), &files).expect("collect succeeds");
    let main = all
        .get(Path::new("src/cli/main.rs"))
        .expect("main.rs extracted");
    assert_eq!(targets(main), ["src/lint/mod.rs", "src/lint/mod.rs"]);
    assert_eq!(lines(main), [1, 2]);
    let lib = all.get(Path::new("src/lib.rs")).expect("lib.rs extracted");
    assert_eq!(*lib, FileImports::default());
}
