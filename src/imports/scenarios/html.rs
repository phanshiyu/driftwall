//! HTML pages: inline scripts, `src`, module preloads, Vite root-absolute paths and what is ignored.

use std::collections::BTreeSet;

use super::fixtures::{pairs, repo};
use super::*;
use crate::model::{ImportKind, Symbol};

fn kinds(imports: &FileImports) -> Vec<(u32, String, ImportKind)> {
    imports
        .resolved
        .iter()
        .map(|i| (i.at.line, i.target.display().to_string(), i.kind))
        .collect()
}

fn edge(line: u32, target: &str, kind: ImportKind) -> (u32, String, ImportKind) {
    (line, target.to_owned(), kind)
}

const PAGE: &str = "<!doctype html>
<html>
<body>
<script type=\"module\">
  import { paint } from '/src/forest-paint.ts';
  import './local.js';
  import 'https://cdn.example.com/lib.js';
  import '//cdn.example.com/lib.js';
  const lazy = () => import('/src/lazy.ts?v=2');
</script>
</body>
</html>
";

#[test]
fn a_root_absolute_import_resolves_against_the_nearest_package_json() {
    let r = repo(&[
        ("app/package.json", "{}"),
        ("app/src/forest-paint.ts", ""),
        ("app/src/lazy.ts", ""),
        ("app/dev/local.js", ""),
        ("app/dev/index.html", PAGE),
    ]);
    let got = r.imports("app/dev/index.html");
    assert_eq!(
        kinds(&got),
        [
            edge(5, "app/src/forest-paint.ts", ImportKind::Runtime),
            edge(6, "app/dev/local.js", ImportKind::Runtime),
            edge(9, "app/src/lazy.ts", ImportKind::Runtime),
        ]
    );
    assert_eq!(
        got.resolved[0].symbols,
        BTreeSet::from([Symbol::Name("paint".to_owned())])
    );
    assert_eq!(got.resolved[2].symbols, BTreeSet::from([Symbol::All]));
    assert_eq!(got.unresolved, []);
    assert_eq!(got.skipped, []);
}

#[test]
fn the_vite_root_is_the_closest_package_json_else_the_repository_root() {
    let nested = repo(&[
        ("app/package.json", "{}"),
        ("app/dev/package.json", "{}"),
        ("app/dev/src/forest-paint.ts", ""),
        ("app/src/forest-paint.ts", ""),
        ("app/dev/index.html", PAGE),
    ]);
    assert_eq!(
        nested.edges("app/dev/index.html"),
        [(5, "app/dev/src/forest-paint.ts".to_owned())]
    );
    let none = repo(&[
        ("src/forest-paint.ts", ""),
        ("app/src/forest-paint.ts", ""),
        ("app/dev/index.html", PAGE),
    ]);
    assert_eq!(
        none.edges("app/dev/index.html"),
        [(5, "src/forest-paint.ts".to_owned())]
    );
}

#[test]
fn a_root_absolute_specifier_that_finds_no_file_is_skipped_never_unresolved() {
    let r = repo(&[
        ("package.json", "{}"),
        (
            "index.html",
            "<script type=module>\nimport '/public/logo.js';\n</script>",
        ),
    ]);
    let got = r.imports("index.html");
    assert_eq!(got.resolved, []);
    assert_eq!(got.unresolved, []);
    assert_eq!(r.skipped("index.html"), pairs(&[(2, "/public/logo.js")]));
}

#[test]
fn a_script_src_and_a_module_preload_import_the_whole_file() {
    let page = "<!doctype html>
<head>
  <link rel=\"stylesheet\" href=\"./style.css\">
  <link href=\"/src/pre.ts\" rel=\"icon  MODULEPRELOAD\" />
  <script src=\"main.js\"></script>
  <script
    defer
    type=\"module\"
    src='./sub/deep.js?x=1#top'
  ></script>
  <script src=\"../shared.js\" />
  <script src=\"/src/root.ts\"></script>
  <script src=\"https://cdn.example.com/a.js\"></script>
  <script src=\"//cdn.example.com/b.js\"></script>
  <script src=\"data:text/javascript,1\"></script>
  <script src=\"\"></script>
</head>";
    let r = repo(&[
        ("package.json", "{}"),
        ("pages/index.html", page),
        ("pages/main.js", ""),
        ("pages/sub/deep.js", ""),
        ("shared.js", ""),
        ("src/pre.ts", ""),
        ("src/root.ts", ""),
        ("pages/style.css", ""),
    ]);
    let got = r.imports("pages/index.html");
    assert_eq!(
        kinds(&got),
        [
            edge(3, "pages/style.css", ImportKind::Asset),
            edge(4, "src/pre.ts", ImportKind::Runtime),
            edge(5, "pages/main.js", ImportKind::Runtime),
            edge(6, "pages/sub/deep.js", ImportKind::Runtime),
            edge(11, "shared.js", ImportKind::Runtime),
            edge(12, "src/root.ts", ImportKind::Runtime),
        ]
    );
    assert!(got.resolved.iter().all(|i| i.symbols.is_empty()));
    assert_eq!(got.unresolved, []);
    assert_eq!(got.skipped, []);
}

#[test]
fn a_missing_relative_src_is_unresolved_at_the_tag() {
    let page =
        "<html>\n<script src=\"gone.js\"></script>\n<script\n src=\"./also-gone.js\">\n</script>";
    let r = repo(&[("p/index.html", page), ("p/here.js", "")]);
    assert_eq!(
        r.unresolved("p/index.html"),
        pairs(&[(2, "gone.js"), (3, "./also-gone.js")])
    );
}

#[test]
fn a_commented_out_script_and_other_script_types_are_ignored() {
    let page = "<!--
<script src=\"./a.js\"></script>
<script>import './a.js';</script>
-->
<script type=\"importmap\">{\"imports\":{\"a\":\"./a.js\"}}</script>
<script type=\"text/template\">import './a.js'</script>
<script type=\"application/json\" src=\"./a.js\"></script>
<script type=\"text/javascript\">import './a.js';</script>
";
    let r = repo(&[("p/index.html", page), ("p/a.js", "")]);
    assert_eq!(
        kinds(&r.imports("p/index.html")),
        [edge(8, "p/a.js", ImportKind::Runtime)]
    );
}

#[test]
fn inline_script_lines_are_lines_of_the_html_file_with_crlf_too() {
    let page = "<html>\r\n<body>\r\n<p>x</p>\r\n<script>\r\n\r\nimport './a.js';\r\n</script>\r\n<script>\r\nimport './b.js';\r\n</script>";
    let r = repo(&[("p/index.html", page), ("p/a.js", ""), ("p/b.js", "")]);
    assert_eq!(
        kinds(&r.imports("p/index.html")),
        [
            edge(6, "p/a.js", ImportKind::Runtime),
            edge(9, "p/b.js", ImportKind::Runtime),
        ]
    );
}

#[test]
fn an_html_page_without_a_script_is_an_asset_that_typescript_can_import() {
    let r = repo(&[
        (
            "a/main.ts",
            "import doc from './doc.html';\nimport page from './page.html';",
        ),
        ("a/doc.html", "<h1>docs</h1><!-- <script src=x.js> -->"),
        ("a/page.html", "<script src=\"x.js\"></script>"),
        ("a/x.js", ""),
    ]);
    assert_eq!(
        kinds(&r.imports("a/main.ts")),
        [
            edge(1, "a/doc.html", ImportKind::Asset),
            edge(2, "a/page.html", ImportKind::Runtime),
        ]
    );
    assert!(!r.collect().contains_key(std::path::Path::new("a/doc.html")));
}

#[test]
fn an_html_script_uses_the_aliases_and_packages_of_its_directory() {
    let r = repo(&[
        ("web/package.json", r#"{"dependencies":{"lit":"3"}}"#),
        (
            "web/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
        ),
        (
            "web/index.html",
            "<script type=\"module\">\nimport '@/boot';\nimport 'lit';\nimport 'undeclared';\n</script>",
        ),
        ("web/src/boot.ts", ""),
    ]);
    assert_eq!(
        r.edges("web/index.html"),
        [(2, "web/src/boot.ts".to_owned())]
    );
    assert_eq!(r.skipped("web/index.html"), pairs(&[(4, "undeclared")]));
}

#[test]
fn a_page_in_a_test_directory_is_test_code() {
    let r = repo(&[
        ("tests/page.html", "<script src=\"../a.js\"></script>"),
        ("a.js", ""),
    ]);
    assert!(r.imports("tests/page.html").resolved[0].test_only);
}

#[test]
fn a_relative_resource_link_into_public_is_ignored_and_a_missing_one_is_not() {
    let page = "<html>
<link rel=\"icon\" href=\"./icons/a.svg\">
<link rel=\"icon\" href=\"icons/gone.svg\">
<script>1</script>
</html>";
    let r = repo(&[
        ("web/package.json", "{}"),
        ("web/index.html", page),
        ("web/public/icons/a.svg", ""),
    ]);
    let got = r.imports("web/index.html");
    assert_eq!(got.resolved, []);
    assert_eq!(
        r.unresolved("web/index.html"),
        pairs(&[(3, "icons/gone.svg")])
    );
}

#[test]
fn a_link_to_a_resource_is_an_asset_reference() {
    let page = "<html>
<link rel=\"stylesheet\" href=\"/src/app.css?v=1\">
<link rel=\"preload\" href=\"/src/ui/font.woff2\" as=\"font\">
<link rel=\"prefetch\" href=\"next.js\">
<link rel=\"icon\" href=\"/icon.svg\">
<link rel=\"apple-touch-icon\" href=\"/public/touch.png\">
<link rel=\"manifest\" href=\"/manifest.webmanifest\">
<link rel=\"stylesheet\" href=\"https://fonts.example.com/a.css\">
<link rel=\"stylesheet\" href=\"//fonts.example.com/b.css\">
<link rel=\"canonical\" href=\"/nothing\">
<link rel=\"stylesheet\" href=\"./gone.css\">
<link rel=\"stylesheet\" href=\"/gone2.css\">
<link rel=\"stylesheet\">
<script>1</script>
</html>";
    let r = repo(&[
        ("package.json", "{}"),
        ("index.html", page),
        ("src/app.css", ""),
        ("src/ui/font.woff2", ""),
        ("next.js", ""),
        ("icon.svg", ""),
        ("public/touch.png", ""),
        ("manifest.webmanifest", ""),
    ]);
    let got = r.imports("index.html");
    assert_eq!(
        kinds(&got),
        [
            edge(2, "src/app.css", ImportKind::Asset),
            edge(3, "src/ui/font.woff2", ImportKind::Asset),
            edge(4, "next.js", ImportKind::Asset),
            edge(5, "icon.svg", ImportKind::Asset),
            edge(6, "public/touch.png", ImportKind::Asset),
            edge(7, "manifest.webmanifest", ImportKind::Asset),
        ]
    );
    assert_eq!(r.unresolved("index.html"), pairs(&[(11, "./gone.css")]));
    assert_eq!(r.skipped("index.html"), pairs(&[(12, "/gone2.css")]));
}
