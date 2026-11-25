use std::path::PathBuf;

use crate::model::{LintReport, Severity, Violation, ViolationKind};
use crate::report::fixtures::violation;
use crate::report::render_text;

const HINT: &str = "fix it";

fn site(kind: ViolationKind, file: &str, line: u32, message: &str, key: &str) -> Violation {
    let mut found = violation(
        kind,
        Severity::Error,
        Some((file, line)),
        message,
        Some(HINT),
    );
    found.key = key.to_owned();
    found
}

fn edge(file: &str, line: u32, target: &str) -> Violation {
    let mut found = site(
        ViolationKind::UndeclaredEdge,
        file,
        line,
        &format!(
            "`billing` imports `search` ({target}) but architecture.mmd declares no edge billing --> search"
        ),
        "billing --> search",
    );
    found.site.target = Some(PathBuf::from(target));
    found
}

fn text(violations: Vec<Violation>) -> String {
    render_text(&LintReport { violations })
}

#[test]
fn a_group_prints_a_header_sorted_sites_and_one_hint() {
    let out = text(vec![
        edge("src/billing/b.rs", 8, "src/search/mod.rs"),
        edge("src/billing/a.rs", 3, "src/search/mod.rs"),
        edge("src/billing/c.rs", 1, "src/search/index.rs"),
    ]);
    let expected = "\
src/billing/a.rs:3: error[undeclared-edge]: `billing` imports `search` in 3 places but architecture.mmd declares no edge billing --> search
  at src/billing/a.rs:3 (src/search/mod.rs)
  at src/billing/b.rs:8 (src/search/mod.rs)
  at src/billing/c.rs:1 (src/search/index.rs)
  hint: fix it
driftwall: 3 errors, 0 warnings (1 group)
";
    assert_eq!(out, expected);
}

#[test]
fn a_single_finding_prints_as_before_and_adds_no_group_count() {
    let out = text(vec![edge("src/billing/a.rs", 3, "src/search/mod.rs")]);
    let expected = "\
src/billing/a.rs:3: error[undeclared-edge]: `billing` imports `search` (src/search/mod.rs) but architecture.mmd declares no edge billing --> search
  hint: fix it
driftwall: 1 error, 0 warnings
";
    assert_eq!(out, expected);
}

#[test]
fn a_test_site_is_its_own_group_and_keeps_its_mark() {
    let mut a = edge("src/billing/a.test.rs", 1, "src/search/mod.rs");
    let mut b = edge("src/billing/b.test.rs", 2, "src/search/mod.rs");
    for found in [&mut a, &mut b] {
        found.test_only = true;
        found.key.push_str(" (test)");
    }
    let out = text(vec![a, b, edge("src/billing/z.rs", 9, "src/search/mod.rs")]);
    assert!(
        out.contains("error[undeclared-edge] (test code): `billing` imports `search` in 2 places")
    );
    assert!(out.contains(
        "src/billing/z.rs:9: error[undeclared-edge]: `billing` imports `search` (src/search/mod.rs)"
    ));
    assert!(out.ends_with("driftwall: 3 errors, 0 warnings (2 groups)\n"));
}

#[test]
fn the_baseline_note_is_shown_once_in_the_header() {
    let note = "; the baseline allows 1, found 2";
    let mut a = edge("src/billing/a.rs", 3, "src/search/mod.rs");
    let mut b = edge("src/billing/b.rs", 4, "src/search/mod.rs");
    a.message.push_str(note);
    b.message.push_str(note);
    let out = text(vec![a, b]);
    assert_eq!(out.matches("the baseline allows").count(), 1);
    assert!(
        out.lines()
            .next()
            .unwrap()
            .ends_with("billing --> search; the baseline allows 1, found 2")
    );
}

#[test]
fn private_symbol_header_and_sites_name_the_symbol() {
    let mut found = Vec::new();
    for (file, line) in [("src/a.rs", 1), ("src/b.rs", 2)] {
        let mut v = site(
            ViolationKind::PrivateSymbol,
            file,
            line,
            "`a` imports `Secret` from `b` (src/b/mod.rs) but m.mmd lets `b` export only: Open",
            "a --> b Secret",
        );
        v.site.target = Some(PathBuf::from("src/b/mod.rs"));
        v.site.symbol = Some("Secret".to_owned());
        found.push(v);
    }
    let out = text(found);
    assert!(out.starts_with(
        "src/a.rs:1: error[private-symbol]: `a` imports `Secret` from `b` in 2 places but m.mmd"
    ));
    assert!(out.contains("  at src/b.rs:2 (src/b/mod.rs, `Secret`)\n"));
}

#[test]
fn upward_edge_header_wording() {
    let mut found = Vec::new();
    for (file, line) in [("src/a.rs", 1), ("src/b.rs", 2)] {
        let mut v = site(
            ViolationKind::UpwardEdge,
            file,
            line,
            "`low` (boundary `down`) imports `top` (boundary `up`, src/top/x.rs) but the `%% order` on line 4 puts `up` above `down`",
            "low --> top",
        );
        v.site.target = Some(PathBuf::from("src/top/x.rs"));
        found.push(v);
    }
    assert!(text(found).contains("imports `top` (boundary `up`) in 2 places but the `%% order`"));
}

#[test]
fn denied_edge_header_wording() {
    let mut found = Vec::new();
    for (file, line) in [("src/a.rs", 1), ("src/b.rs", 2)] {
        let mut v = site(
            ViolationKind::DeniedEdge,
            file,
            line,
            "`ui-shell` imports `core-db` (src/core/db.rs) but m.mmd:14 `%% deny ui: core` forbids it",
            "ui-shell --> core-db",
        );
        v.site.target = Some(PathBuf::from("src/core/db.rs"));
        found.push(v);
    }
    assert!(
        text(found)
            .contains("imports `core-db` in 2 places but m.mmd:14 `%% deny ui: core` forbids it")
    );
}

#[test]
fn isolation_breach_header_wording() {
    let mut found = Vec::new();
    for (file, line) in [("src/a.rs", 1), ("src/b.rs", 2)] {
        let mut v = site(
            ViolationKind::IsolationBreach,
            file,
            line,
            "boundary `f` is isolated (`%% isolated` on line 3): `x` (in `p`) imports `y` (in `q`, src/y.rs), and `p` and `q` may not depend on each other",
            "f: x --> y",
        );
        v.site.target = Some(PathBuf::from("src/y.rs"));
        found.push(v);
    }
    assert!(text(found).contains("imports `y` (in `q`) in 2 places, and `p` and `q`"));
}

#[test]
fn production_imports_test_header_wording() {
    let mut found = Vec::new();
    for line in [1, 5] {
        let mut v = site(
            ViolationKind::ProductionImportsTest,
            "src/a.rs",
            line,
            "production file `src/a.rs` imports test file `src/t.rs`, but `%% tests: isolated` on line 2 keeps test code out of production",
            "src/a.rs --> src/t.rs",
        );
        v.site.target = Some(PathBuf::from("src/t.rs"));
        found.push(v);
    }
    assert!(text(found).contains("imports test file `src/t.rs` in 2 places, but"));
}

#[test]
fn forbidden_package_header_wording() {
    let mut found = Vec::new();
    for (file, line) in [("src/a.rs", 1), ("src/b.rs", 2)] {
        found.push(site(
            ViolationKind::ForbiddenPackage,
            file,
            line,
            "`app` imports `left-pad` (third-party) but m.mmd:3 `%% forbid-packages app` forbids it",
            "app left-pad",
        ));
    }
    let out = text(found);
    assert!(out.contains("`app` imports `left-pad` in 2 places (third-party) but m.mmd:3"));
    assert!(out.contains("  at src/b.rs:2\n"));
}

#[test]
fn an_unknown_shape_falls_back_to_a_place_count() {
    let out = text(vec![
        site(
            ViolationKind::UnresolvedImport,
            "src/a.ts",
            1,
            "import `./x` resolves to no file",
            "src/a.ts ./x",
        ),
        site(
            ViolationKind::UnresolvedImport,
            "src/a.ts",
            2,
            "import `./x` resolves to no file",
            "src/a.ts ./x",
        ),
    ]);
    assert!(out.starts_with(
        "src/a.ts:1: error[unresolved-import]: import `./x` resolves to no file (2 places)\n"
    ));
}

#[test]
fn findings_without_a_key_or_place_never_group() {
    let out = text(vec![
        violation(
            ViolationKind::StaleEdge,
            Severity::Warning,
            None,
            "stale",
            None,
        ),
        violation(
            ViolationKind::StaleEdge,
            Severity::Warning,
            None,
            "stale",
            None,
        ),
    ]);
    assert_eq!(out.matches("warning[stale-edge]: stale").count(), 2);
    assert!(!out.contains("group"));
}
