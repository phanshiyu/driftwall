use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::model::Location;

fn known(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

fn run(source: &str, language: Language, file: &str, repo: &[&str]) -> FileImports {
    extract(source, language, Path::new(file), &known(repo)).expect("extract succeeds")
}

fn ts(source: &str, repo: &[&str]) -> FileImports {
    run(source, Language::TypeScript, "src/main.ts", repo)
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

fn unresolved_specifiers(imports: &FileImports) -> Vec<&str> {
    imports
        .unresolved
        .iter()
        .map(|u| u.specifier.as_str())
        .collect()
}

#[test]
fn default_import() {
    let got = ts("import x from './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn named_import() {
    let got = ts("import { x, y } from './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn namespace_import() {
    let got = ts("import * as x from './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn side_effect_import() {
    let got = ts("import './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn type_only_import_counts() {
    let got = ts("import type { X } from './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn export_star() {
    let got = ts("export * from './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn export_named_from() {
    let got = ts("export { x } from './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn export_type_from() {
    let got = ts("export type { X } from './a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn export_without_source_is_not_an_import() {
    let got = ts("const x = 1;\nexport { x };\nexport default x;", &[]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn require_call() {
    let got = run(
        "const x = require('./a');",
        Language::JavaScript,
        "src/main.js",
        &["src/a.js"],
    );
    assert_eq!(targets(&got), ["src/a.js"]);
}

#[test]
fn require_in_typescript() {
    let got = ts("const x = require('./a');", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn other_call_with_string_argument_is_not_require() {
    let got = ts("load('./a'); obj.require('./a');", &["src/a.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn require_with_non_literal_argument_ignored() {
    let got = ts("require(name); require('./a' + b);", &["src/a.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn dynamic_import() {
    let got = ts("const m = await import('./a');", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn dynamic_import_with_substituted_template_ignored() {
    let got = ts("import(`./${name}`);", &["src/a.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn dynamic_import_with_plain_template_ignored() {
    let got = ts("import(`./a`);", &["src/a.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn dynamic_import_with_variable_ignored() {
    let got = ts("import(path);", &["src/a.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn import_equals_require() {
    let got = ts("import x = require('./a');", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn tsx_file_with_jsx() {
    let source =
        "import { Button } from './button';\nexport const App = () => <Button<string> />;\n";
    let got = run(
        source,
        Language::TypeScript,
        "src/App.tsx",
        &["src/button.tsx"],
    );
    assert_eq!(targets(&got), ["src/button.tsx"]);
}

#[test]
fn jsx_file_with_jsx() {
    let source =
        "import { Button } from './button';\nexport const App = () => <Button label=\"x\" />;\n";
    let got = run(
        source,
        Language::JavaScript,
        "src/App.jsx",
        &["src/button.jsx"],
    );
    assert_eq!(targets(&got), ["src/button.jsx"]);
}

#[test]
fn comments_are_ignored() {
    let source = "// import x from './a';\n/* import y from './a';\nrequire('./a'); */\n";
    let got = ts(source, &["src/a.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn string_contents_are_ignored() {
    let source = "const s = \"import x from './a'\";\nconst t = `require('./a')`;\n";
    let got = ts(source, &["src/a.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn non_relative_specifiers_are_ignored() {
    let source = "import a from 'react';\nimport b from '@scope/pkg';\nimport c from 'node:fs';\n\
                  import d from '@/x';\nimport e from 'src/a';\nimport f from '.hidden';\n";
    let got = ts(source, &["src/a.ts", "@/x.ts"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn exact_file_wins_over_probing() {
    let got = ts("import './a';", &["src/a", "src/a.ts"]);
    assert_eq!(targets(&got), ["src/a"]);
}

#[test]
fn extension_probing_follows_declared_order() {
    let order = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
    for (position, expected) in order.iter().enumerate() {
        let repo: Vec<String> = order
            .iter()
            .skip(position)
            .map(|ext| format!("src/a.{ext}"))
            .collect();
        let repo_refs: Vec<&str> = repo.iter().map(String::as_str).collect();
        let got = ts("import './a';", &repo_refs);
        assert_eq!(targets(&got), [format!("src/a.{expected}")]);
    }
}

#[test]
fn extension_is_appended_not_replaced() {
    let got = ts("import './user.service';", &["src/user.service.ts"]);
    assert_eq!(targets(&got), ["src/user.service.ts"]);
}

#[test]
fn directory_index() {
    let got = ts("import './lib';", &["src/lib/index.ts"]);
    assert_eq!(targets(&got), ["src/lib/index.ts"]);
}

#[test]
fn file_beats_directory_index() {
    let got = ts("import './lib';", &["src/lib.ts", "src/lib/index.ts"]);
    assert_eq!(targets(&got), ["src/lib.ts"]);
}

#[test]
fn directory_index_follows_extension_order() {
    let got = ts(
        "import './lib';",
        &["src/lib/index.js", "src/lib/index.tsx"],
    );
    assert_eq!(targets(&got), ["src/lib/index.tsx"]);
}

#[test]
fn trailing_slash_means_directory() {
    let got = ts("import './lib/';", &["src/lib.ts", "src/lib/index.ts"]);
    assert_eq!(targets(&got), ["src/lib/index.ts"]);
}

#[test]
fn dot_and_dot_dot_resolve_to_directory_index() {
    let source = "import a from '.';\nimport b from '..';\n";
    let got = ts(source, &["src/index.ts", "index.ts"]);
    assert_eq!(targets(&got), ["src/index.ts", "index.ts"]);
}

#[test]
fn js_specifier_resolves_to_ts_file() {
    let got = ts("import './a.js';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn js_to_ts_mapping_covers_every_extension() {
    let cases = [
        ("./a.js", "src/a.tsx"),
        ("./a.jsx", "src/a.tsx"),
        ("./a.mjs", "src/a.mts"),
        ("./a.cjs", "src/a.cts"),
    ];
    for (specifier, file) in cases {
        let got = ts(&format!("import '{specifier}';"), &[file]);
        assert_eq!(targets(&got), [file], "{specifier}");
    }
}

#[test]
fn real_js_file_beats_ts_mapping() {
    let got = ts("import './a.js';", &["src/a.js", "src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.js"]);
}

#[test]
fn parent_traversal() {
    let got = run(
        "import '../shared/util';",
        Language::TypeScript,
        "src/deep/main.ts",
        &["src/shared/util.ts"],
    );
    assert_eq!(targets(&got), ["src/shared/util.ts"]);
}

#[test]
fn dot_segments_are_folded() {
    let got = ts("import './x/../a';", &["src/a.ts"]);
    assert_eq!(targets(&got), ["src/a.ts"]);
}

#[test]
fn importer_at_root_resolves_siblings() {
    let got = run("import './a';", Language::TypeScript, "main.ts", &["a.ts"]);
    assert_eq!(targets(&got), ["a.ts"]);
}

#[test]
fn escaping_the_repository_root_is_unresolved() {
    let got = run(
        "import '../../outside';",
        Language::TypeScript,
        "src/main.ts",
        &["outside.ts"],
    );
    assert_eq!(targets(&got), Vec::<String>::new());
    assert_eq!(unresolved_specifiers(&got), ["../../outside"]);
}

#[test]
fn assets_are_skipped() {
    let source = "import a from './data.json';\nimport './style.css';\nimport b from './logo.svg';\n\
                  import c from './pic.png';\nimport d from './addon.node';\n";
    let got = ts(source, &[]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn asset_that_is_a_known_file_resolves() {
    let got = ts("import a from './data.json';", &["src/data.json"]);
    assert_eq!(targets(&got), ["src/data.json"]);
}

#[test]
fn missing_file_is_unresolved_with_raw_specifier() {
    let got = ts("import x from './nope';", &["src/a.ts"]);
    assert_eq!(targets(&got), Vec::<String>::new());
    assert_eq!(unresolved_specifiers(&got), ["./nope"]);
}

#[test]
fn missing_js_specifier_is_unresolved() {
    let got = ts("import x from './nope.js';", &[]);
    assert_eq!(unresolved_specifiers(&got), ["./nope.js"]);
}

#[test]
fn line_numbers_are_one_indexed() {
    let source = "import a from './a';\n\nconst x = require('./b');\nexport * from './c';\n";
    let got = ts(source, &["src/a.ts", "src/b.ts", "src/c.ts"]);
    assert_eq!(lines(&got), [1, 3, 4]);
}

#[test]
fn multi_line_import_reports_the_line_the_statement_starts_on() {
    let source = "\nimport {\n  a,\n  b,\n} from\n  './a';\n";
    let got = ts(source, &["src/a.ts"]);
    assert_eq!(lines(&got), [2]);
}

#[test]
fn multi_line_export_from_reports_the_line_the_statement_starts_on() {
    let source = "\n\nexport {\n  a,\n} from './a';\n";
    let got = ts(source, &["src/a.ts"]);
    assert_eq!(lines(&got), [3]);
}

#[test]
fn multi_line_require_and_dynamic_import_report_the_call_start() {
    let source = "const x =\n  require(\n    './a',\n  );\nconst y = await\n  import(\n './b');\n";
    let got = ts(source, &["src/a.ts", "src/b.ts"]);
    assert_eq!(lines(&got), [2, 6]);
}

#[test]
fn location_carries_the_importing_file() {
    let got = ts("import './a';", &["src/a.ts"]);
    assert_eq!(
        got.resolved.first().map(|r| r.at.clone()),
        Some(Location {
            file: PathBuf::from("src/main.ts"),
            line: 1
        })
    );
}

#[test]
fn duplicates_collapse() {
    let source = "import a from './a';\nimport { b } from './a'; import c from './a';\nimport './a';\n\
                  import x from './a';\n";
    let got = ts(source, &["src/a.ts"]);
    assert_eq!(lines(&got), [1, 2, 3, 4]);
    let same_line = ts("import a from './a'; import b from './a';", &["src/a.ts"]);
    assert_eq!(lines(&same_line), [1]);
    let missing = ts("import a from './x'; import b from './x';", &[]);
    assert_eq!(missing.unresolved.len(), 1);
}

#[test]
fn output_is_sorted() {
    let source = "import './b';\nimport './a';\nimport './zz';\nimport './yy';\n";
    let got = ts(source, &["src/a.ts", "src/b.ts"]);
    let mut sorted = got.resolved.clone();
    sorted.sort();
    assert_eq!(got.resolved, sorted);
    assert_eq!(targets(&got), ["src/b.ts", "src/a.ts"]);
    let mut sorted = got.unresolved.clone();
    sorted.sort();
    assert_eq!(got.unresolved, sorted);
    assert_eq!(unresolved_specifiers(&got), ["./zz", "./yy"]);
}

#[test]
fn table_entries_match_their_language() {
    for language in [
        Language::TypeScript,
        Language::JavaScript,
        Language::Python,
        Language::Rust,
    ] {
        assert_eq!(spec(language).map(|s| s.language), Some(language));
    }
}

#[test]
fn syntax_error_still_yields_valid_imports() {
    let source = "import a from './a';\nconst = ;\nimport b from './b';\n";
    let got = ts(source, &["src/a.ts", "src/b.ts"]);
    assert_eq!(targets(&got), ["src/a.ts", "src/b.ts"]);
}

#[test]
fn empty_source_has_no_imports() {
    assert_eq!(ts("", &[]), FileImports::default());
}

#[test]
fn collect_reads_a_tree_deterministically() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = [
        ("src/a.ts", "import './b';\nimport './missing';\n"),
        ("src/b.tsx", "export const B = () => <div />;\n"),
        ("src/c.js", "const a = require('./a.js');\n"),
        ("src/lib/index.ts", "import '../b';\n"),
    ];
    let mut map = BTreeMap::new();
    for (path, content) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(&full, content).expect("write");
        let language = if path == "src/c.js" {
            Language::JavaScript
        } else {
            Language::TypeScript
        };
        map.insert(PathBuf::from(path), language);
    }

    let first = collect(dir.path(), &map).expect("collect");
    let second = collect(dir.path(), &map).expect("collect again");
    assert_eq!(first, second);
    assert_eq!(first.keys().cloned().collect::<Vec<_>>(), {
        let mut keys: Vec<PathBuf> = map.keys().cloned().collect();
        keys.sort();
        keys
    });

    let a = first.get(Path::new("src/a.ts")).expect("a.ts");
    assert_eq!(targets(a), ["src/b.tsx"]);
    assert_eq!(unresolved_specifiers(a), ["./missing"]);
    let c = first.get(Path::new("src/c.js")).expect("c.js");
    assert_eq!(targets(c), ["src/a.ts"]);
    let lib = first.get(Path::new("src/lib/index.ts")).expect("lib");
    assert_eq!(targets(lib), ["src/b.tsx"]);
}

#[test]
fn collect_reports_unreadable_file_with_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut map = BTreeMap::new();
    map.insert(PathBuf::from("gone.ts"), Language::TypeScript);
    match collect(dir.path(), &map) {
        Err(ImportsError::Io { path, .. }) => assert_eq!(path, PathBuf::from("gone.ts")),
        other => panic!("expected Io error, got {other:?}"),
    }
}

#[test]
fn collect_reports_non_utf8_file_with_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("bad.ts"), [0xff, 0xfe, 0x00]).expect("write");
    let mut map = BTreeMap::new();
    map.insert(PathBuf::from("bad.ts"), Language::TypeScript);
    match collect(dir.path(), &map) {
        Err(ImportsError::Io { path, .. }) => assert_eq!(path, PathBuf::from("bad.ts")),
        other => panic!("expected Io error, got {other:?}"),
    }
}

#[test]
fn collect_resolves_aliased_imports_end_to_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = [
        (
            "tsconfig.json",
            "{\n  // aliases\n  \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\"], }, },\n}\n",
        ),
        (
            "src/app/main.ts",
            "import a from '@/foo/bar';\nimport r from 'react';\nimport m from '@/missing';\nimport l from './local';\n",
        ),
        ("src/foo/bar.ts", "export const bar = 1;\n"),
        ("src/app/local.ts", "export const local = 1;\n"),
        (
            "packages/web/tsconfig.json",
            "{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"./lib/*\"] } } }\n",
        ),
        (
            "packages/web/src/page.ts",
            "import u from '@/util';\nimport b from '@/foo/bar';\n",
        ),
        ("packages/web/lib/util.ts", "export const util = 1;\n"),
    ];
    let mut map = BTreeMap::new();
    for (path, content) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(&full, content).expect("write");
        if Path::new(path).extension().is_some_and(|e| e == "ts") {
            map.insert(PathBuf::from(path), Language::TypeScript);
        }
    }

    let first = collect(dir.path(), &map).expect("collect");
    assert_eq!(first, collect(dir.path(), &map).expect("collect again"));

    let main = first.get(Path::new("src/app/main.ts")).expect("main.ts");
    assert_eq!(targets(main), ["src/foo/bar.ts", "src/app/local.ts"]);
    assert_eq!(lines(main), [1, 4]);
    assert_eq!(unresolved_specifiers(main), Vec::<&str>::new());
    let page = first
        .get(Path::new("packages/web/src/page.ts"))
        .expect("page.ts");
    assert_eq!(targets(page), ["packages/web/lib/util.ts"]);
}
