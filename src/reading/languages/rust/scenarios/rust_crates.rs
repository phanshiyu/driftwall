//! Crate names a path starts with (`::a_lib`, a renamed dependency), module re-exports, globs of a
//! re-exporter, `self::` through a binding and test attributes, run through the whole pipeline.

use super::fixtures::{pairs, repo};

const LIB_MANIFEST: &str = "[package]\nname = \"a-lib\"\nversion = \"0.1.0\"\n";
const LIB: &str = "pub fn a() {}\npub struct A;\n";

fn dependent(dependencies: &str) -> String {
    format!("[package]\nname = \"b\"\nversion = \"0.1.0\"\n[dependencies]\n{dependencies}\n")
}

#[test]
fn a_leading_double_colon_names_an_extern_crate_in_use_code_and_macros() {
    let repo = repo(&[
        ("crates/a-lib/Cargo.toml", LIB_MANIFEST),
        ("crates/a-lib/src/lib.rs", LIB),
        (
            "crates/b/Cargo.toml",
            &dependent("a-lib = { path = \"../a-lib\" }"),
        ),
        (
            "crates/b/src/lib.rs",
            "use ::a_lib::A;\nfn f() { ::a_lib::a(); }\nfn g() { println!(\"{}\", ::a_lib::a()); }\n",
        ),
    ]);
    let lib = "crates/a-lib/src/lib.rs";
    assert_eq!(
        repo.edges("crates/b/src/lib.rs"),
        pairs(&[(1, lib), (2, lib), (3, lib)])
    );
}

#[test]
fn a_leading_double_colon_is_never_the_current_crate() {
    // why: `::model::A` is a crate named `model`, not `crate::model`.
    let repo = repo(&[
        (
            "src/lib.rs",
            "pub mod model;\nuse ::model::A;\nfn f() { ::model::g(); }\n",
        ),
        ("src/model.rs", "pub struct A;\npub fn g() {}\n"),
    ]);
    assert_eq!(repo.edges("src/lib.rs"), Vec::new());
}

#[test]
fn a_renamed_dependency_is_the_crate_it_renames_for_its_dependent_only() {
    let repo = repo(&[
        ("crates/a-lib/Cargo.toml", LIB_MANIFEST),
        ("crates/a-lib/src/lib.rs", LIB),
        (
            "crates/b/Cargo.toml",
            &dependent("alpha = { path = \"../a-lib\", package = \"a-lib\" }"),
        ),
        (
            "crates/b/src/lib.rs",
            "use alpha::A;\nfn f() { alpha::a(); }\n",
        ),
        ("crates/b/tests/it.rs", "#[test]\nfn t() { alpha::a(); }\n"),
        ("crates/c/Cargo.toml", &dependent("")),
        ("crates/c/src/lib.rs", "use alpha::A;\n"),
    ]);
    let lib = "crates/a-lib/src/lib.rs";
    assert_eq!(
        repo.edges("crates/b/src/lib.rs"),
        pairs(&[(1, lib), (2, lib)])
    );
    assert_eq!(repo.edges("crates/b/tests/it.rs"), pairs(&[(2, lib)]));
    assert_eq!(repo.edges("crates/c/src/lib.rs"), Vec::new());
}

#[test]
fn a_renamed_dev_and_build_dependency_count_too() {
    let manifest = "[package]\nname = \"b\"\nversion = \"0.1.0\"\n\
                    [dev-dependencies]\nd = { path = \"../a-lib\", package = \"a-lib\" }\n\
                    [target.'cfg(unix)'.build-dependencies]\nbd = { path = \"../a-lib\", package = \"a-lib\" }\n";
    let repo = repo(&[
        ("crates/a-lib/Cargo.toml", LIB_MANIFEST),
        ("crates/a-lib/src/lib.rs", LIB),
        ("crates/b/Cargo.toml", manifest),
        ("crates/b/src/lib.rs", "fn f() { d::a(); bd::a(); }\n"),
    ]);
    assert_eq!(
        repo.edges("crates/b/src/lib.rs"),
        pairs(&[(1, "crates/a-lib/src/lib.rs")])
    );
}

const MODULES: &[(&str, &str)] = &[
    ("src/lib.rs", "pub mod a;\npub mod b;\npub mod c;\n"),
    ("src/a/mod.rs", "pub fn a() {}\npub struct A;\n"),
];

fn with(extra: &[(&'static str, &'static str)]) -> super::fixtures::Repo {
    let all: Vec<(&str, &str)> = MODULES.iter().chain(extra).copied().collect();
    repo(&all)
}

#[test]
fn self_through_a_module_alias_is_followed_like_the_alias() {
    let repo = with(&[
        (
            "src/c/mod.rs",
            "use crate::a as z;\npub use self::z::A;\npub type X = self::z::A;\n",
        ),
        ("src/b/mod.rs", "use crate::c::A;\nuse crate::c::X;\n"),
    ]);
    let a = "src/a/mod.rs";
    assert_eq!(repo.edges("src/b/mod.rs"), pairs(&[(1, a), (2, a)]));
}

#[test]
fn a_module_reexport_is_followed_into_the_module() {
    let repo = with(&[
        (
            "src/c/mod.rs",
            "pub use crate::a;\npub use crate::a as inner;\n",
        ),
        (
            "src/b/mod.rs",
            "fn f() { crate::c::a::a(); }\nuse crate::c::inner::A;\n",
        ),
    ]);
    let a = "src/a/mod.rs";
    assert_eq!(repo.edges("src/b/mod.rs"), pairs(&[(1, a), (2, a)]));
}

#[test]
fn a_module_reexport_keeps_the_edge_of_its_own_statement() {
    let repo = with(&[("src/c/mod.rs", "pub use crate::a;\n")]);
    assert_eq!(repo.edges("src/c/mod.rs"), pairs(&[(1, "src/a/mod.rs")]));
}

#[test]
fn a_private_module_use_is_not_a_reexport() {
    let repo = with(&[
        ("src/c/mod.rs", "use crate::a;\npub fn f() {}\n"),
        ("src/b/mod.rs", "use crate::c::a::A;\n"),
    ]);
    assert_eq!(repo.edges("src/b/mod.rs"), pairs(&[(1, "src/c/mod.rs")]));
}

#[test]
fn a_glob_of_a_reexporter_reaches_what_it_reexports() {
    let repo = with(&[
        ("src/c/mod.rs", "pub use crate::a::A;\n"),
        ("src/b/mod.rs", "use crate::c::*;\nfn f(_: A) {}\n"),
    ]);
    assert_eq!(repo.edges("src/b/mod.rs"), pairs(&[(1, "src/a/mod.rs")]));
}

#[test]
fn a_glob_of_a_module_with_items_of_its_own_keeps_that_module_too() {
    let repo = with(&[
        ("src/c/mod.rs", "pub use crate::a::A;\npub fn own() {}\n"),
        ("src/b/mod.rs", "use crate::c::*;\n"),
    ]);
    let imports = repo.imports("src/b/mod.rs");
    let targets: Vec<String> = imports
        .resolved
        .iter()
        .map(|import| import.target.display().to_string())
        .collect();
    assert_eq!(targets, ["src/a/mod.rs", "src/c/mod.rs"]);
}

#[test]
fn a_glob_of_a_module_that_reexports_nothing_stays_on_it() {
    let repo = with(&[
        ("src/c/mod.rs", "pub fn own() {}\n"),
        ("src/b/mod.rs", "use crate::c::*;\n"),
    ]);
    assert_eq!(repo.edges("src/b/mod.rs"), pairs(&[(1, "src/c/mod.rs")]));
}

#[test]
fn an_inline_module_of_reexports_is_followed() {
    let repo = with(&[
        ("src/c/mod.rs", "pub mod r {\n    pub use crate::a::A;\n}\n"),
        ("src/b/mod.rs", "use crate::c::r::A;\n"),
    ]);
    assert_eq!(repo.edges("src/b/mod.rs"), pairs(&[(1, "src/a/mod.rs")]));
}

#[test]
fn an_inline_module_with_items_of_its_own_is_not_followed() {
    let repo = with(&[
        (
            "src/c/mod.rs",
            "pub mod r {\n    pub use crate::a::A;\n    pub fn own() {}\n}\n",
        ),
        ("src/b/mod.rs", "use crate::c::r::A;\n"),
    ]);
    assert_eq!(repo.edges("src/b/mod.rs"), pairs(&[(1, "src/c/mod.rs")]));
}

#[test]
fn a_test_attribute_marks_the_item_as_test_code() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "pub mod a;\n#[test]\nfn t() { crate::a::x(); }\n\
             #[tokio::test(flavor = \"multi_thread\")]\nasync fn u() { crate::a::y(); }\n\
             fn prod() { crate::a::z(); }\n",
        ),
        ("src/a.rs", ""),
    ]);
    let flags: Vec<(u32, bool)> = repo
        .imports("src/lib.rs")
        .resolved
        .iter()
        .map(|import| (import.at.line, import.test_only))
        .collect();
    assert_eq!(flags, [(3, true), (5, true), (6, false)]);
}
