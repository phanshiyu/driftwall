//! The Vite root of an HTML page: the nearest `vite.config.*` and its `root` option, then the
//! nearest `package.json`, then the repository root; and a root-absolute path that finds nothing.

use super::fixtures::{pairs, repo};
use crate::code::SkipReason;

const PAGE: &str = "<!doctype html>\n<script type=\"module\" src=\"/src/main.tsx\"></script>\n";

#[test]
fn the_directory_of_the_vite_config_is_the_root_even_when_a_package_json_is_further_up() {
    let r = repo(&[
        ("package.json", "{}"),
        ("src/main.ts", ""),
        ("src/main.tsx", ""),
        ("ui/vite.config.ts", "export default {};"),
        ("ui/index.html", PAGE),
        ("ui/src/main.tsx", ""),
    ]);
    let got = r.imports("ui/index.html");
    assert_eq!(
        r.edges("ui/index.html"),
        [(2, "ui/src/main.tsx".to_owned())]
    );
    assert_eq!(got.skipped, []);
    assert_eq!(got.unresolved, []);
}

#[test]
fn a_vite_config_beats_a_nearer_package_json() {
    let r = repo(&[
        ("app/vite.config.js", "export default {};"),
        ("app/dev/package.json", "{}"),
        ("app/dev/index.html", PAGE),
        ("app/dev/src/main.tsx", ""),
        ("app/src/main.tsx", ""),
    ]);
    assert_eq!(
        r.edges("app/dev/index.html"),
        [(2, "app/src/main.tsx".to_owned())]
    );
}

#[test]
fn the_nearest_vite_config_governs() {
    let r = repo(&[
        ("vite.config.ts", "export default {};"),
        ("site/vite.config.ts", "export default {};"),
        ("site/pages/index.html", PAGE),
        ("site/src/main.tsx", ""),
        ("src/main.tsx", ""),
    ]);
    assert_eq!(
        r.edges("site/pages/index.html"),
        [(2, "site/src/main.tsx".to_owned())]
    );
}

#[test]
fn a_literal_root_option_is_relative_to_the_config_directory() {
    let r = repo(&[
        (
            "vite.config.ts",
            "import { defineConfig } from 'vite';\nexport default defineConfig({ root: 'ui' });",
        ),
        ("src/main.tsx", ""),
        ("ui/index.html", PAGE),
        ("ui/src/main.tsx", ""),
    ]);
    assert_eq!(
        r.edges("ui/index.html"),
        [(2, "ui/src/main.tsx".to_owned())]
    );
}

#[test]
fn a_resolved_root_option_is_read_without_running_the_config() {
    for config in [
        "export default { root: path.resolve(__dirname, 'ui') };",
        "export default { root: fileURLToPath(new URL('./ui', import.meta.url)) };",
        "export default () => ({ root: resolve(__dirname, './ui/') });",
    ] {
        let r = repo(&[
            ("vite.config.mts", config),
            ("src/main.tsx", ""),
            ("ui/index.html", PAGE),
            ("ui/src/main.tsx", ""),
        ]);
        assert_eq!(
            r.edges("ui/index.html"),
            [(2, "ui/src/main.tsx".to_owned())],
            "{config}"
        );
    }
}

#[test]
fn a_root_option_that_cannot_be_read_means_the_config_directory() {
    let r = repo(&[
        (
            "ui/vite.config.ts",
            "export default { root: process.cwd() };",
        ),
        ("ui/index.html", PAGE),
        ("ui/src/main.tsx", ""),
    ]);
    assert_eq!(
        r.edges("ui/index.html"),
        [(2, "ui/src/main.tsx".to_owned())]
    );
}

#[test]
fn without_a_vite_config_the_nearest_package_json_is_the_root() {
    let r = repo(&[
        ("app/package.json", "{}"),
        ("app/dev/index.html", PAGE),
        ("app/src/main.tsx", ""),
        ("src/main.tsx", ""),
    ]);
    assert_eq!(
        r.edges("app/dev/index.html"),
        [(2, "app/src/main.tsx".to_owned())]
    );
}

#[test]
fn without_either_the_repository_root_is_the_root() {
    let r = repo(&[
        ("app/dev/index.html", PAGE),
        ("app/src/main.tsx", ""),
        ("src/main.tsx", ""),
    ]);
    assert_eq!(
        r.edges("app/dev/index.html"),
        [(2, "src/main.tsx".to_owned())]
    );
}

#[test]
fn a_root_absolute_path_that_finds_nothing_is_skipped_with_the_root_it_searched() {
    let page = "<script src=\"/src/gone.ts\"></script>\n<script type=\"module\">\nimport '/lib/gone.ts';\nimport 'https://cdn.example.com/x.js';\n</script>";
    let r = repo(&[
        ("ui/vite.config.ts", "export default {};"),
        ("ui/index.html", page),
        ("src/gone.ts", ""),
    ]);
    let got = r.imports("ui/index.html");
    assert_eq!(got.resolved, []);
    assert_eq!(got.unresolved, []);
    let skipped: Vec<_> = got
        .skipped
        .iter()
        .map(|skip| {
            (
                skip.at.line,
                skip.specifier.as_str(),
                skip.reason,
                skip.message.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        skipped,
        [
            (
                1,
                "/src/gone.ts",
                SkipReason::UnresolvedRootPath,
                Some("`/src/gone.ts` finds no file below the Vite root `ui`")
            ),
            (
                3,
                "/lib/gone.ts",
                SkipReason::UnresolvedRootPath,
                Some("`/lib/gone.ts` finds no file below the Vite root `ui`")
            ),
        ]
    );
}

#[test]
fn the_message_names_the_repository_root_as_a_dot() {
    let r = repo(&[("index.html", "<script src=\"/gone.js\"></script>")]);
    let got = r.imports("index.html");
    assert_eq!(
        got.skipped[0].message.as_deref(),
        Some("`/gone.js` finds no file below the Vite root `.`")
    );
}

#[test]
fn a_file_vite_serves_from_public_is_not_skipped() {
    let r = repo(&[
        ("ui/vite.config.ts", "export default {};"),
        (
            "ui/index.html",
            "<script src=\"/logo.js\"></script>\n<link rel=\"modulepreload\" href=\"/gone.js\">",
        ),
        ("ui/public/logo.js", ""),
    ]);
    assert_eq!(r.edges("ui/index.html"), []);
    assert_eq!(r.skipped("ui/index.html"), pairs(&[(2, "/gone.js")]));
}

#[test]
fn a_root_absolute_import_outside_html_is_not_special() {
    let r = repo(&[
        ("a/main.ts", "import '/src/x.ts';\n"),
        ("src/x.ts", ""),
        ("a/vite.config.ts", "export default {};"),
    ]);
    let got = r.imports("a/main.ts");
    assert_eq!(got.resolved, []);
    assert_eq!(got.skipped.len(), 1);
    assert_eq!(got.skipped[0].reason, SkipReason::UndeclaredPackage);
}
