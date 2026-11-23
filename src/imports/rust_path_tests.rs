//! `#[path = ".."]` places a module at the file it names.

use super::fixtures::{pairs, repo};

#[test]
fn a_path_module_resolves_crate_paths_to_its_file() {
    let repo = repo(&[
        ("Cargo.toml", ""),
        (
            "src/lib.rs",
            "#[path = \"b/impl.rs\"]\npub mod a;\npub mod c;\n",
        ),
        (
            "src/b/impl.rs",
            "use crate::c::Thing;\npub fn g() { crate::a::g(); }\n",
        ),
        ("src/c.rs", "use crate::a::f;\npub struct Thing;\n"),
    ]);
    assert_eq!(repo.edges("src/c.rs"), pairs(&[(1, "src/b/impl.rs")]));
    // why: a path into its own module is the file itself, so it is no edge and no false edge to lib.rs.
    assert_eq!(repo.edges("src/b/impl.rs"), pairs(&[(1, "src/c.rs")]));
}

#[test]
fn self_and_super_inside_a_path_module_are_relative_to_its_module_name() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "#[path = \"b/impl.rs\"]\npub mod a;\npub mod c;\n",
        ),
        ("src/b/impl.rs", "use super::c::Thing;\n"),
        ("src/c.rs", ""),
    ]);
    assert_eq!(repo.edges("src/b/impl.rs"), pairs(&[(1, "src/c.rs")]));
}

#[test]
fn modules_below_a_path_module_are_found_next_to_its_file() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "#[path = \"b/impl.rs\"]\npub mod a;\npub mod c;\n",
        ),
        ("src/b/impl.rs", "pub mod x;\n"),
        ("src/b/x.rs", ""),
        ("src/c.rs", "use crate::a::x::f;\n"),
    ]);
    assert_eq!(repo.edges("src/c.rs"), pairs(&[(1, "src/b/x.rs")]));
}

#[test]
fn a_path_inside_an_inline_module_is_relative_to_the_module_directory() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "mod outer {\n    #[path = \"p.rs\"]\n    pub mod a;\n}\npub mod c;\n",
        ),
        ("src/outer/p.rs", ""),
        ("src/c.rs", "use crate::outer::a::f;\n"),
    ]);
    assert_eq!(repo.edges("src/c.rs"), pairs(&[(1, "src/outer/p.rs")]));
}

#[test]
fn a_path_inside_an_inline_module_of_a_non_mod_rs_file_starts_at_the_file_stem() {
    let repo = repo(&[
        ("src/lib.rs", "pub mod m;\npub mod c;\n"),
        (
            "src/m.rs",
            "mod inner {\n    #[path = \"q.rs\"]\n    pub mod a;\n}\n",
        ),
        ("src/m/inner/q.rs", ""),
        ("src/c.rs", "use crate::m::inner::a::f;\n"),
    ]);
    assert_eq!(repo.edges("src/c.rs"), pairs(&[(1, "src/m/inner/q.rs")]));
}

#[test]
fn a_path_to_an_unknown_file_is_not_followed() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "#[path = \"nope.rs\"]\npub mod a;\npub mod c;\n",
        ),
        ("src/c.rs", "use crate::a::f;\n"),
    ]);
    assert_eq!(repo.edges("src/c.rs"), pairs(&[(1, "src/lib.rs")]));
}

#[test]
fn a_cfg_test_path_module_makes_its_file_test_code() {
    let repo = repo(&[
        (
            "src/lib.rs",
            "pub mod c;\n#[cfg(test)]\n#[path = \"t/x.rs\"]\nmod tests;\n",
        ),
        ("src/t/x.rs", "use crate::c::f;\n"),
        ("src/c.rs", ""),
    ]);
    let imports = repo.imports("src/t/x.rs");
    assert_eq!(imports.resolved.len(), 1);
    assert!(imports.resolved.iter().all(|import| import.test_only));
}
