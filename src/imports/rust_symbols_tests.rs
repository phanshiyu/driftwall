//! The symbols Rust imports use: `use` trees and qualified paths.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;

/// A library crate at `src/` with module files in both layouts.
const REPO: &[&str] = &[
    "src/lib.rs",
    "src/model/mod.rs",
    "src/diagram/mod.rs",
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
        known: &known,
        crates: &crates,
        aliases: &BTreeMap::new(),
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
fn self_in_a_group_records_the_module_itself_as_all() {
    assert_eq!(
        used("use crate::model::{self, Edge};"),
        [entry(1, "src/model/mod.rs", &["Edge", "*"])]
    );
    assert_eq!(
        used("use crate::util::{self};"),
        [entry(1, "src/util.rs", &["*"])]
    );
    assert_eq!(
        used("use crate::model::{self as m};"),
        [entry(1, "src/model/mod.rs", &["*"])]
    );
}

#[test]
fn a_use_of_a_module_records_all() {
    assert_eq!(
        used("use crate::model;"),
        [entry(1, "src/model/mod.rs", &["*"])]
    );
    assert_eq!(
        used("use crate::model as m;"),
        [entry(1, "src/model/mod.rs", &["*"])]
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
fn a_qualified_path_ending_at_the_module_records_all() {
    assert_eq!(
        used("fn f() { let _ = crate::model; }"),
        [entry(1, "src/model/mod.rs", &["*"])]
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
        used("use crate::model::{self, Zed, Alpha};"),
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
