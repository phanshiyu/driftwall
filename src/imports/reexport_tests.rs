//! Rust re-exports followed to the defining file, see "Re-exports" in `docs/DESIGN.md`.

use super::fixtures::repo;
use crate::model::ImportKind;

/// The (line, target, symbols) of every resolved import of `file`.
pub(super) fn sites(files: &[(&str, &str)], file: &str) -> Vec<(u32, String, String)> {
    repo(files)
        .imports(file)
        .resolved
        .iter()
        .map(|import| {
            let symbols: Vec<String> = import.symbols.iter().map(ToString::to_string).collect();
            (
                import.at.line,
                import.target.display().to_string(),
                symbols.join(","),
            )
        })
        .collect()
}

pub(super) fn site(line: u32, target: &str, symbols: &str) -> (u32, String, String) {
    (line, target.to_owned(), symbols.to_owned())
}

const UI: &[(&str, &str)] = &[
    ("src/lib.rs", "mod ui;\nmod app;\n"),
    (
        "src/ui/mod.rs",
        "mod tone;\nmod other;\npub use tone::Tone;\npub(crate) use other::{Theme, Mode as Style};\n",
    ),
    ("src/ui/tone.rs", "pub struct Tone;\n"),
    ("src/ui/other.rs", "pub struct Theme;\npub struct Mode;\n"),
];

fn with_app(app: &str) -> Vec<(&str, &str)> {
    let mut files = UI.to_vec();
    files.push(("src/app.rs", app));
    files
}

#[test]
fn rust_pub_use_is_followed_to_the_defining_file() {
    let got = sites(&with_app("use crate::ui::Tone;\n"), "src/app.rs");
    assert_eq!(got, [site(1, "src/ui/tone.rs", "Tone")]);
}

#[test]
fn rust_group_and_pub_crate_use_follow_each_name() {
    let got = sites(&with_app("use crate::ui::{Tone, Theme};\n"), "src/app.rs");
    assert_eq!(
        got,
        [
            site(1, "src/ui/other.rs", "Theme"),
            site(1, "src/ui/tone.rs", "Tone")
        ]
    );
}

#[test]
fn rust_renamed_reexport_is_recorded_under_the_defining_name() {
    let got = sites(&with_app("use crate::ui::Style;\n"), "src/app.rs");
    assert_eq!(got, [site(1, "src/ui/other.rs", "Mode")]);
}

#[test]
fn rust_path_in_code_is_followed_too() {
    let got = sites(
        &with_app("fn f() { crate::ui::Tone::new(); }\n"),
        "src/app.rs",
    );
    assert_eq!(got, [site(1, "src/ui/tone.rs", "Tone")]);
}

#[test]
fn rust_reexporting_module_keeps_its_own_edge() {
    let got = sites(&with_app(""), "src/ui/mod.rs");
    assert_eq!(
        got,
        [
            site(3, "src/ui/tone.rs", "Tone"),
            site(4, "src/ui/other.rs", "Mode,Theme")
        ]
    );
}

#[test]
fn rust_module_binding_and_unknown_names_stay_on_the_module() {
    let got = sites(
        &with_app("use crate::ui;\nuse crate::ui::Missing;\nuse crate::ui::*;\n"),
        "src/app.rs",
    );
    assert_eq!(
        got,
        [
            site(1, "src/ui/mod.rs", ""),
            site(2, "src/ui/mod.rs", "Missing"),
            site(3, "src/ui/mod.rs", "*"),
        ]
    );
}

#[test]
fn rust_private_use_is_not_a_reexport() {
    let files = [
        ("src/lib.rs", "mod ui;\nmod app;\n"),
        ("src/ui/mod.rs", "mod tone;\nuse tone::Tone;\n"),
        ("src/ui/tone.rs", "pub struct Tone;\n"),
        ("src/app.rs", "use crate::ui::Tone;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/ui/mod.rs", "Tone")]
    );
}

#[test]
fn rust_name_defined_locally_stays_local() {
    let files = [
        ("src/lib.rs", "mod ui;\nmod app;\n"),
        (
            "src/ui/mod.rs",
            "mod tone;\npub use tone::Tone;\npub struct Tone;\n",
        ),
        ("src/ui/tone.rs", "pub struct Tone;\n"),
        ("src/app.rs", "use crate::ui::Tone;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/ui/mod.rs", "Tone")]
    );
}

#[test]
fn rust_chain_is_followed_to_the_end_and_keeps_each_hop_edge() {
    let files = [
        ("src/lib.rs", "mod a;\nmod app;\n"),
        ("src/a/mod.rs", "mod b;\npub use b::Deep;\n"),
        ("src/a/b/mod.rs", "mod c;\npub use c::Deep;\n"),
        ("src/a/b/c.rs", "pub struct Deep;\n"),
        ("src/app.rs", "use crate::a::Deep;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/a/b/c.rs", "Deep")]
    );
    assert_eq!(
        sites(&files, "src/a/mod.rs"),
        [site(2, "src/a/b/mod.rs", "Deep")]
    );
}

#[test]
fn rust_cycle_stays_on_the_module() {
    let files = [
        ("src/lib.rs", "mod a;\nmod b;\nmod app;\n"),
        ("src/a.rs", "pub use crate::b::Loop;\n"),
        ("src/b.rs", "pub use crate::a::Loop;\n"),
        ("src/app.rs", "use crate::a::Loop;\n"),
    ];
    assert_eq!(sites(&files, "src/app.rs"), [site(1, "src/a.rs", "Loop")]);
}

#[test]
fn rust_glob_is_followed_when_one_source_can_define_the_name() {
    let files = [
        ("src/lib.rs", "mod ui;\nmod app;\n"),
        (
            "src/ui/mod.rs",
            "mod tone;\nmod size;\npub use tone::*;\npub use size::*;\n",
        ),
        ("src/ui/tone.rs", "pub struct Tone;\n"),
        ("src/ui/size.rs", "pub struct Size;\n"),
        ("src/app.rs", "use crate::ui::{Tone, Size, Other};\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [
            site(1, "src/ui/mod.rs", "Other"),
            site(1, "src/ui/size.rs", "Size"),
            site(1, "src/ui/tone.rs", "Tone"),
        ]
    );
}

#[test]
fn rust_glob_with_two_candidate_sources_stays() {
    let files = [
        ("src/lib.rs", "mod ui;\nmod app;\n"),
        (
            "src/ui/mod.rs",
            "mod a;\nmod b;\npub use a::*;\npub use b::*;\n",
        ),
        ("src/ui/a.rs", "pub struct Same;\n"),
        ("src/ui/b.rs", "pub struct Same;\n"),
        ("src/app.rs", "use crate::ui::Same;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/ui/mod.rs", "Same")]
    );
}

#[test]
fn rust_glob_through_a_glob_is_followed() {
    let files = [
        ("src/lib.rs", "mod ui;\nmod app;\n"),
        ("src/ui/mod.rs", "mod inner;\npub use inner::*;\n"),
        ("src/ui/inner/mod.rs", "mod leaf;\npub use leaf::*;\n"),
        ("src/ui/inner/leaf.rs", "pub fn draw() {}\n"),
        ("src/app.rs", "use crate::ui::draw;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/ui/inner/leaf.rs", "draw")]
    );
}

#[test]
fn rust_crate_root_facade_is_followed() {
    let files = [
        ("src/lib.rs", "mod config;\npub use config::Config;\n"),
        ("src/config.rs", "pub struct Config;\n"),
        ("src/app.rs", "use crate::Config;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/config.rs", "Config")]
    );
}

#[test]
fn rust_kind_and_test_flag_survive_the_move() {
    let files = with_app("#[cfg(test)]\nmod tests {\n    use crate::ui::Tone;\n}\n");
    let imports = repo(&files).imports("src/app.rs");
    let [import] = imports.resolved.as_slice() else {
        panic!("one import expected, got {:?}", imports.resolved);
    };
    assert_eq!(import.target.display().to_string(), "src/ui/tone.rs");
    assert!(import.test_only);
    assert_eq!(import.kind, ImportKind::Runtime);
}

#[test]
fn rust_name_that_leads_back_to_the_importer_is_not_an_import() {
    let files = [
        ("src/lib.rs", "mod ui;\n"),
        ("src/ui/mod.rs", "mod tone;\npub use tone::Tone;\n"),
        ("src/ui/tone.rs", "use super::Tone;\npub struct Tone;\n"),
    ];
    assert_eq!(sites(&files, "src/ui/tone.rs"), []);
}
