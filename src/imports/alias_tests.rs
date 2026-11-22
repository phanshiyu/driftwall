//! Tests of the pure alias model: which paths a specifier stands for, before any probing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;

const NONE: [&str; 0] = [];

fn config(directory: &str, bare_imports: bool, entries: &[(&str, &[&str])]) -> AliasConfig {
    AliasConfig {
        base: PathBuf::from(directory),
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

fn paths(config: &AliasConfig, specifier: &str) -> Vec<String> {
    config
        .candidates(specifier)
        .iter()
        .map(|c| c.path.display().to_string())
        .collect()
}

#[test]
fn star_pattern_substitutes_the_matched_text() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    assert_eq!(paths(&c, "@/foo/bar"), ["src/foo/bar"]);
}

#[test]
fn star_may_match_the_empty_string() {
    let c = config("", false, &[("@/*", &["./src/*"])]);
    let found = c.candidates("@/");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found.first().map(|f| f.path.as_path()),
        Some(Path::new("src"))
    );
}

#[test]
fn a_trailing_slash_in_the_target_names_a_directory() {
    let c = config("", false, &[("@lib", &["./lib/"])]);
    let found = c.candidates("@lib");
    assert_eq!(
        found.first().map(|f| f.path.as_path()),
        Some(Path::new("lib"))
    );
    assert_eq!(found.first().map(|f| f.directory_only), Some(true));
}

#[test]
fn targets_are_listed_in_declaration_order() {
    let c = config("", false, &[("@/*", &["./first/*", "./second/*"])]);
    assert_eq!(paths(&c, "@/x"), ["first/x", "second/x"]);
}

#[test]
fn targets_are_relative_to_the_base() {
    let c = config(
        "packages/web",
        false,
        &[("@/*", &["./src/*", "../shared/*"])],
    );
    assert_eq!(
        paths(&c, "@/x"),
        ["packages/web/src/x", "packages/shared/x"]
    );
}

#[test]
fn exact_pattern_matches_only_the_whole_specifier() {
    let c = config("", false, &[("config", &["./src/config/index"])]);
    assert_eq!(paths(&c, "config"), ["src/config/index"]);
    assert_eq!(paths(&c, "config/more"), NONE);
    assert_eq!(paths(&c, "myconfig"), NONE);
}

#[test]
fn exact_pattern_beats_a_longer_star_pattern_declared_before_it() {
    let c = config(
        "",
        false,
        &[("@/ui/*", &["./star/*"]), ("@/ui/button", &["./exact"])],
    );
    assert_eq!(paths(&c, "@/ui/button"), ["exact"]);
}

#[test]
fn an_exact_key_has_no_text_for_a_target_with_a_star() {
    let c = config("", false, &[("lib", &["./a/*", "./b"])]);
    assert_eq!(paths(&c, "lib"), ["b"]);
}

#[test]
fn longest_prefix_wins_whatever_the_declaration_order() {
    let wide_first = config(
        "",
        false,
        &[("@/*", &["./src/*"]), ("@/components/*", &["./ui/*"])],
    );
    let narrow_first = config(
        "",
        false,
        &[("@/components/*", &["./ui/*"]), ("@/*", &["./src/*"])],
    );
    for c in [&wide_first, &narrow_first] {
        assert_eq!(paths(c, "@/components/Button"), ["ui/Button"]);
        assert_eq!(paths(c, "@/util"), ["src/util"]);
    }
}

#[test]
fn equal_prefixes_pick_the_first_declared() {
    let c = config(
        "",
        false,
        &[("*.css", &["./css/*.css"]), ("*", &["./any/*"])],
    );
    assert_eq!(paths(&c, "a.css"), ["css/a.css"]);
    let reversed = config(
        "",
        false,
        &[("*", &["./any/*"]), ("*.css", &["./css/*.css"])],
    );
    assert_eq!(paths(&reversed, "a.css"), ["any/a.css"]);
}

#[test]
fn a_suffix_must_match_too() {
    let c = config("", false, &[("@/*.svg", &["./icons/*.svg"])]);
    assert_eq!(paths(&c, "@/a.svg"), ["icons/a.svg"]);
    assert_eq!(paths(&c, "@/a.png"), NONE);
}

#[test]
fn prefix_and_suffix_may_not_overlap() {
    let c = config("", false, &[("ab*ba", &["./x/*"])]);
    assert_eq!(paths(&c, "aba"), NONE);
    assert_eq!(paths(&c, "abba"), ["x"]);
}

#[test]
fn star_in_the_middle_of_a_target() {
    let c = config("", false, &[("~/*", &["./src/*/index"])]);
    assert_eq!(paths(&c, "~/foo"), ["src/foo/index"]);
}

#[test]
fn a_key_with_two_stars_is_invalid_and_dropped() {
    assert_eq!(Pattern::parse("@/*/*", &["./a".to_owned()]), None);
    let c = config("", false, &[("@/*/*", &["./a"])]);
    assert_eq!(paths(&c, "@/x/y"), NONE);
}

#[test]
fn a_target_with_two_stars_is_dropped_but_its_siblings_stay() {
    let c = config("", false, &[("@/*", &["./a/*/*", "./b/*"])]);
    assert_eq!(paths(&c, "@/x"), ["b/x"]);
}

#[test]
fn bare_specifiers_use_the_base_when_base_url_is_set() {
    let c = config("src", true, &[]);
    assert_eq!(paths(&c, "utils/x"), ["src/utils/x"]);
}

#[test]
fn bare_specifiers_are_not_tried_without_base_url() {
    let c = config("src", false, &[("@/*", &["./*"])]);
    assert_eq!(paths(&c, "utils/x"), NONE);
}

#[test]
fn base_url_comes_after_the_pattern_targets() {
    let c = config("src", true, &[("@/*", &["./alt/*"])]);
    assert_eq!(paths(&c, "@/x"), ["src/alt/x", "src/@/x"]);
}

#[test]
fn a_candidate_leaving_the_repository_is_dropped() {
    let c = config("", false, &[("@/*", &["../outside/*", "/abs/*", "./in/*"])]);
    assert_eq!(paths(&c, "@/x"), ["in/x"]);
    let deep = config("a", false, &[("@/*", &["../../out/*"])]);
    assert_eq!(paths(&deep, "@/x"), NONE);
}

#[test]
fn candidates_are_deterministic() {
    let c = config(
        "",
        true,
        &[("@/*", &["./a/*", "./b/*"]), ("@/x/*", &["./c/*"])],
    );
    assert_eq!(c.candidates("@/x/y"), c.candidates("@/x/y"));
}

#[test]
fn nearest_prefers_the_closest_directory() {
    let mut configs = BTreeMap::new();
    configs.insert(PathBuf::new(), config("", false, &[("r", &["./r"])]));
    configs.insert(
        PathBuf::from("pkg/web"),
        config("pkg/web", false, &[("w", &["./w"])]),
    );
    let at = |file: &str| nearest(&configs, Path::new(file)).map(|c| c.base.clone());
    assert_eq!(at("pkg/web/src/a.ts"), Some(PathBuf::from("pkg/web")));
    assert_eq!(at("pkg/web/a.ts"), Some(PathBuf::from("pkg/web")));
    assert_eq!(at("pkg/api/a.ts"), Some(PathBuf::new()));
    assert_eq!(at("a.ts"), Some(PathBuf::new()));
    assert_eq!(
        nearest::<AliasConfig>(&BTreeMap::new(), Path::new("a.ts")),
        None
    );
}
