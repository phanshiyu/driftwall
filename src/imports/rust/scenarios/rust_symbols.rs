//! The symbols Rust imports use: `use` trees and qualified paths.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;

/// A library crate at `src/` with module files in both layouts.
const REPO: &[&str] = &[
    "src/lib.rs",
    "src/model/mod.rs",
    "src/diagram/mod.rs",
    "src/diagram/parse.rs",
    "src/lint/mod.rs",
    "src/lint/rules.rs",
    "src/gitlab.rs",
    "src/gitlab/api.rs",
    "src/util.rs",
    "src/util/inner.rs",
];

/// One expected import: line, target file, symbols in serialized order.
type Used = (u32, String, Vec<String>);

/// The imports of `source` written in `file` of [`REPO`], as (line, target, symbols).
fn used_at(source: &str, file: &str) -> Vec<Used> {
    let known: BTreeSet<PathBuf> = REPO.iter().map(PathBuf::from).collect();
    let crates = BTreeMap::from([("driftwall".to_owned(), PathBuf::from("src"))]);
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
            crates: &crates,
            paths: &PathModules::default(),
            dependencies: &BTreeMap::new(),
        },
        swift: &SwiftModules::default(),
    };
    let got = extract_in(source, Language::Rust, Path::new(file), &env).expect("extract succeeds");
    got.resolved
        .iter()
        .map(|import| {
            (
                import.at.line,
                import.target.display().to_string(),
                import.symbols.iter().map(ToString::to_string).collect(),
            )
        })
        .collect()
}

fn used(source: &str) -> Vec<Used> {
    used_at(source, "src/lint/rules.rs")
}

fn entry(line: u32, target: &str, symbols: &[&str]) -> Used {
    (
        line,
        target.to_owned(),
        symbols.iter().map(|s| (*s).to_owned()).collect(),
    )
}

#[test]
fn use_names_are_recorded_per_leaf() {
    assert_eq!(
        used("use crate::model::Contract;"),
        [entry(1, "src/model/mod.rs", &["Contract"])]
    );
    assert_eq!(
        used("use crate::model::{Edge, Contract};"),
        [entry(1, "src/model/mod.rs", &["Contract", "Edge"])]
    );
}

#[test]
fn use_group_across_modules_records_each_against_its_file() {
    assert_eq!(
        used("use crate::{model::Edge, diagram::Parser, util::{inner::Thing, Other}};"),
        [
            entry(1, "src/diagram/mod.rs", &["Parser"]),
            entry(1, "src/model/mod.rs", &["Edge"]),
            entry(1, "src/util/inner.rs", &["Thing"]),
            entry(1, "src/util.rs", &["Other"]),
        ]
    );
}

#[test]
fn use_alias_records_the_original_name() {
    assert_eq!(
        used("use crate::model::Contract as C;"),
        [entry(1, "src/model/mod.rs", &["Contract"])]
    );
    assert_eq!(
        used("use crate::model::{Edge as E, Contract};"),
        [entry(1, "src/model/mod.rs", &["Contract", "Edge"])]
    );
}

#[test]
fn use_glob_records_all() {
    assert_eq!(
        used("use crate::model::*;"),
        [entry(1, "src/model/mod.rs", &["*"])]
    );
    assert_eq!(
        used("use crate::{diagram::*, model::*};"),
        [
            entry(1, "src/diagram/mod.rs", &["*"]),
            entry(1, "src/model/mod.rs", &["*"])
        ]
    );
}

#[test]
fn self_in_a_group_binds_the_module_and_records_no_symbol_for_it() {
    assert_eq!(
        used("use crate::model::{self, Edge};"),
        [entry(1, "src/model/mod.rs", &["Edge"])]
    );
    assert_eq!(
        used("use crate::util::{self};"),
        [entry(1, "src/util.rs", &[])]
    );
    assert_eq!(
        used("use crate::model::{self as m};"),
        [entry(1, "src/model/mod.rs", &[])]
    );
}

#[test]
fn a_use_of_a_module_binds_it_and_records_no_symbol() {
    assert_eq!(
        used("use crate::model;"),
        [entry(1, "src/model/mod.rs", &[])]
    );
    assert_eq!(
        used("use crate::model as m;"),
        [entry(1, "src/model/mod.rs", &[])]
    );
}

#[test]
fn a_root_item_records_its_name_against_the_root_file() {
    assert_eq!(used("use crate::Foo;"), [entry(1, "src/lib.rs", &["Foo"])]);
}

#[test]
fn qualified_paths_record_the_first_item_segment() {
    assert_eq!(
        used("fn f() { crate::model::build(); }"),
        [entry(1, "src/model/mod.rs", &["build"])]
    );
    assert_eq!(
        used("fn f(x: crate::model::Edge) {}"),
        [entry(1, "src/model/mod.rs", &["Edge"])]
    );
    assert_eq!(
        used("fn f() { crate::model::Edge::new(); }"),
        [entry(1, "src/model/mod.rs", &["Edge"])]
    );
    assert_eq!(
        used("fn f() -> crate::model::Edge<u8> { todo() }"),
        [entry(1, "src/model/mod.rs", &["Edge"])]
    );
}

#[test]
fn a_qualified_path_ending_at_a_module_file_names_an_item_of_the_parent() {
    // why: `diagram/parse.rs` is a module and a module is no value, so the call names `diagram::parse`.
    assert_eq!(
        used("fn f() { crate::diagram::parse(); }"),
        [entry(1, "src/diagram/mod.rs", &["parse"])]
    );
    assert_eq!(
        used("fn f(x: crate::diagram::parse) {}"),
        [entry(1, "src/diagram/mod.rs", &["parse"])]
    );
    assert_eq!(
        used("fn f() { crate::model; }"),
        [entry(1, "src/lib.rs", &["model"])]
    );
}

#[test]
fn a_use_leaf_that_ends_at_a_module_file_binds_the_module() {
    assert_eq!(
        used("use crate::diagram::parse;"),
        [entry(1, "src/diagram/parse.rs", &[])]
    );
}

#[test]
fn paths_in_macro_arguments_record_the_item() {
    assert_eq!(
        used("fn f() { println!(\"{}\", crate::model::build()); }"),
        [entry(1, "src/model/mod.rs", &["build"])]
    );
}

#[test]
fn super_and_self_paths_record_the_item() {
    assert_eq!(
        used("use super::Thing;"),
        [entry(1, "src/lint/mod.rs", &["Thing"])]
    );
    assert_eq!(used("use super::*;"), [entry(1, "src/lint/mod.rs", &["*"])]);
    assert_eq!(
        used_at("use self::rules::Rule;", "src/lint/mod.rs"),
        [entry(1, "src/lint/rules.rs", &["Rule"])]
    );
    assert_eq!(
        used("fn f() { super::helper(); }"),
        [entry(1, "src/lint/mod.rs", &["helper"])]
    );
}

#[test]
fn library_crate_paths_record_the_item() {
    assert_eq!(
        used_at("use driftwall::lint::run;", "src/other/main.rs"),
        [entry(1, "src/lint/mod.rs", &["run"])]
    );
    assert_eq!(
        used("fn f() { driftwall::lint::run(); }"),
        [entry(1, "src/lint/mod.rs", &["run"])]
    );
}

#[test]
fn an_inline_test_module_importing_super_makes_no_edge() {
    assert_eq!(
        used_at("mod tests { use super::*; }", "src/lint/mod.rs"),
        Vec::<Used>::new()
    );
}

#[test]
fn statements_on_one_line_for_one_file_union_their_symbols() {
    assert_eq!(
        used("use crate::model::A; use crate::model::B; use crate::util::C;"),
        [
            entry(1, "src/model/mod.rs", &["A", "B"]),
            entry(1, "src/util.rs", &["C"]),
        ]
    );
}

#[test]
fn different_lines_stay_separate_entries() {
    assert_eq!(
        used("use crate::model::A;\nuse crate::model::B;\nuse crate::model::A;\n"),
        [
            entry(1, "src/model/mod.rs", &["A"]),
            entry(2, "src/model/mod.rs", &["B"]),
            entry(3, "src/model/mod.rs", &["A"]),
        ]
    );
}

#[test]
fn symbols_are_sorted_with_all_last() {
    assert_eq!(
        used("use crate::model::{Zed, Alpha}; use crate::model::*;"),
        [entry(1, "src/model/mod.rs", &["Alpha", "Zed", "*"])]
    );
}

#[test]
fn std_and_third_party_paths_record_nothing() {
    assert_eq!(
        used("use std::fmt::Display; use serde::Serialize;"),
        Vec::<Used>::new()
    );
}

const GITLAB: &str = "src/gitlab.rs";

#[test]
fn a_used_module_name_resolves_the_path_that_starts_with_it() {
    assert_eq!(
        used("use crate::gitlab;\nfn f() { gitlab::Client::new(); }"),
        [entry(1, GITLAB, &[]), entry(2, GITLAB, &["Client"])]
    );
}

#[test]
fn an_aliased_module_name_resolves_to_the_original_module() {
    assert_eq!(
        used("use crate::gitlab as gl;\nfn f(c: gl::Client) {}"),
        [entry(1, GITLAB, &[]), entry(2, GITLAB, &["Client"])]
    );
    assert_eq!(
        used("use crate::{gitlab::{self as g}};\nfn f() { g::fetch(); }"),
        [entry(1, GITLAB, &[]), entry(2, GITLAB, &["fetch"])]
    );
}

#[test]
fn an_aliased_nested_module_resolves_to_its_own_file() {
    assert_eq!(
        used("use crate::gitlab::api as a;\nfn f() { a::get(); }"),
        [
            entry(1, "src/gitlab/api.rs", &[]),
            entry(2, "src/gitlab/api.rs", &["get"])
        ]
    );
}

#[test]
fn a_use_leaf_that_starts_with_a_module_name_resolves_through_it() {
    assert_eq!(
        used("use crate::gitlab;\nuse gitlab::Client;"),
        [entry(1, GITLAB, &[]), entry(2, GITLAB, &["Client"])]
    );
}

#[test]
fn a_declared_module_resolves_the_path_that_starts_with_it() {
    assert_eq!(
        used_at("mod gitlab;\nfn f() { gitlab::fetch(); }", "src/lib.rs"),
        [entry(2, GITLAB, &["fetch"])]
    );
    assert_eq!(
        used_at(
            "pub mod rules;\nfn f() { rules::check(); }",
            "src/lint/mod.rs"
        ),
        [entry(2, "src/lint/rules.rs", &["check"])]
    );
}

#[test]
fn a_name_bound_to_an_item_is_not_a_second_reference_to_its_module() {
    assert_eq!(
        used("use crate::gitlab::Client;\nfn f() { Client::new(); }"),
        [entry(1, GITLAB, &["Client"])]
    );
}

#[test]
fn a_glob_binds_no_name() {
    assert_eq!(
        used("use crate::gitlab::*;\nfn f() { Client::new(); }"),
        [entry(1, GITLAB, &["*"])]
    );
}

#[test]
fn std_and_unknown_names_are_not_followed() {
    assert_eq!(
        used("use std::fmt;\nfn f(_: fmt::Result) {}"),
        Vec::<Used>::new()
    );
    assert_eq!(used("fn f() { gitlab::fetch(); }"), Vec::<Used>::new());
}

#[test]
fn a_name_bound_to_two_different_paths_is_not_followed() {
    assert_eq!(
        used("use crate::gitlab as x;\nuse crate::model as x;\nfn f() { x::A; }"),
        [entry(1, GITLAB, &[]), entry(2, "src/model/mod.rs", &[])]
    );
}

#[test]
fn bindings_are_scoped_to_their_module_block() {
    assert_eq!(
        used("use crate::gitlab;\nmod inner { fn f() { gitlab::Client::new(); } }"),
        [entry(1, GITLAB, &[])]
    );
    assert_eq!(
        used("mod inner {\n    use crate::gitlab;\n    fn f() { gitlab::Client::new(); }\n}"),
        [entry(2, GITLAB, &[]), entry(3, GITLAB, &["Client"])]
    );
    assert_eq!(
        used("mod inner {\n    use crate::gitlab;\n}\nfn f() { gitlab::Client::new(); }"),
        [entry(2, GITLAB, &[])]
    );
}

#[test]
fn a_use_inside_a_function_binds_for_its_module() {
    assert_eq!(
        used("fn f() {\n    use crate::gitlab;\n    gitlab::fetch();\n}"),
        [entry(2, GITLAB, &[]), entry(3, GITLAB, &["fetch"])]
    );
}

#[test]
fn a_module_name_in_a_macro_argument_resolves_through_its_binding() {
    assert_eq!(
        used("use crate::gitlab as gl;\nfn f() { println!(\"{}\", gl::fetch()); }"),
        [entry(1, GITLAB, &[]), entry(2, GITLAB, &["fetch"])]
    );
}

#[test]
fn an_inline_module_with_a_super_glob_inherits_the_parent_bindings() {
    assert_eq!(
        used_at(
            "use crate::gitlab;\nmod tests { use super::*; fn t() { gitlab::fetch(); } }",
            "src/lint/mod.rs"
        ),
        [entry(1, GITLAB, &[]), entry(2, GITLAB, &["fetch"])]
    );
}

#[test]
fn a_super_glob_sees_self_aliases_renames_and_mod_declarations() {
    let source = "use crate::gitlab::{self as gl};\nuse crate::model as m;\nmod tests {\n    use super::*;\n    fn t() { gl::fetch(); m::A; }\n}";
    assert_eq!(
        used_at(source, "src/lint/mod.rs"),
        [
            entry(1, GITLAB, &[]),
            entry(2, "src/model/mod.rs", &[]),
            entry(5, GITLAB, &["fetch"]),
            entry(5, "src/model/mod.rs", &["A"]),
        ]
    );
    assert_eq!(
        used_at(
            "mod rules;\nmod tests { use super::*; fn t() { rules::f(); } }",
            "src/lint/mod.rs"
        ),
        [entry(2, "src/lint/rules.rs", &["f"])]
    );
}

#[test]
fn use_super_name_re_imports_the_parent_binding() {
    let source = "use crate::gitlab;\nmod tests {\n    use super::gitlab;\n    fn t() { gitlab::fetch(); }\n}";
    assert_eq!(
        used_at(source, "src/lint/mod.rs"),
        [entry(1, GITLAB, &[]), entry(4, GITLAB, &["fetch"])]
    );
}

#[test]
fn a_super_glob_chains_and_its_own_binding_wins() {
    let chained = "use crate::gitlab;\nmod a { use super::*; mod b { use super::*; fn t() { gitlab::f(); } } }";
    assert_eq!(
        used_at(chained, "src/lint/mod.rs"),
        [entry(1, GITLAB, &[]), entry(2, GITLAB, &["f"])]
    );
    let own = "use crate::gitlab;\nmod a { use super::*; use crate::model as gitlab; fn t() { gitlab::A; } }";
    assert_eq!(
        used_at(own, "src/lint/mod.rs"),
        [entry(1, GITLAB, &[]), entry(2, "src/model/mod.rs", &["A"])]
    );
}

#[test]
fn a_module_without_a_super_glob_inherits_nothing() {
    assert_eq!(
        used_at(
            "use crate::gitlab;\nmod tests { fn t() { gitlab::fetch(); } }",
            "src/lint/mod.rs"
        ),
        [entry(1, GITLAB, &[])]
    );
}
