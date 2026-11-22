//! Svelte and HTML files: the scripts they hold, resolved like TypeScript and JavaScript.

use std::collections::BTreeSet;

use super::fixtures::{Repo, pairs, repo};
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

const CARD: &str = "<script context=\"module\" lang=\"ts\">
  import { shared } from '../lib/x';
</script>

<script lang=\"ts\">
  import Y from './Y.svelte';
  import type { T } from '../lib/types';
  const n: number = 1;
</script>

<p>import fake from '../lib/nope';</p>
<style>
  @import './nope.css';
</style>
";

fn card_repo(card: &str) -> Repo {
    repo(&[
        ("app/ui/Card.svelte", card),
        ("app/ui/Y.svelte", ""),
        ("app/lib/x.ts", ""),
        ("app/lib/types.ts", ""),
    ])
}

#[test]
fn a_component_imports_through_its_module_and_instance_scripts() {
    let got = card_repo(CARD).imports("app/ui/Card.svelte");
    assert_eq!(
        kinds(&got),
        [
            edge(2, "app/lib/x.ts", ImportKind::Runtime),
            edge(6, "app/ui/Y.svelte", ImportKind::Runtime),
            edge(7, "app/lib/types.ts", ImportKind::TypeOnly),
        ]
    );
    assert_eq!(
        got.resolved
            .iter()
            .map(|i| i
                .symbols
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec!["shared"], vec!["default"], vec!["T"]]
    );
    assert_eq!(got.unresolved, []);
    assert_eq!(got.skipped, []);
}

#[test]
fn the_svelte_5_module_attribute_and_other_attributes_count() {
    let r = repo(&[
        (
            "c/A.svelte",
            "<script module>\nimport './m.js';\n</script>\n<script generics=\"T\" lang=TS>\nimport './t';\n</script>",
        ),
        ("c/m.js", ""),
        ("c/t.ts", ""),
    ]);
    assert_eq!(
        kinds(&r.imports("c/A.svelte")),
        [
            edge(2, "c/m.js", ImportKind::Runtime),
            edge(5, "c/t.ts", ImportKind::Runtime),
        ]
    );
}

#[test]
fn a_script_without_lang_is_javascript() {
    let r = repo(&[
        (
            "c/A.svelte",
            "<h1>x</h1>\n<script>\n  import b from './b';\n  const lazy = () => import('./c.js');\n</script>\n",
        ),
        ("c/b.js", ""),
        ("c/c.js", ""),
    ]);
    assert_eq!(
        kinds(&r.imports("c/A.svelte")),
        [
            edge(3, "c/b.js", ImportKind::Runtime),
            edge(4, "c/c.js", ImportKind::Runtime),
        ]
    );
}

#[test]
fn typescript_syntax_is_read_with_lang_ts() {
    let source = "<script lang=\"typescript\">\nimport a from './a';\nlet x: Map<string, number> = new Map();\nimport b from './b';\n</script>";
    let r = repo(&[("c/A.svelte", source), ("c/a.ts", ""), ("c/b.ts", "")]);
    assert_eq!(
        kinds(&r.imports("c/A.svelte")),
        [
            edge(2, "c/a.ts", ImportKind::Runtime),
            edge(4, "c/b.ts", ImportKind::Runtime),
        ]
    );
}

#[test]
fn crlf_files_report_the_same_lines() {
    let crlf = CARD.replace('\n', "\r\n");
    let got = kinds(&card_repo(&crlf).imports("app/ui/Card.svelte"));
    assert_eq!(got.len(), 3);
    assert_eq!(got, kinds(&card_repo(CARD).imports("app/ui/Card.svelte")));
}

#[test]
fn markup_and_style_are_never_read() {
    let source = "<div>\nimport a from './a';\n{@const x = require('./a')}\n</div>\n<style>\n@import './a.css';\n</style>\n<!-- <script>import './a'</script> -->\n";
    let r = repo(&[("c/A.svelte", source), ("c/a.ts", "")]);
    let got = r.imports("c/A.svelte");
    assert_eq!(kinds(&got), []);
    assert_eq!(got.unresolved, []);
}

#[test]
fn a_tsconfig_alias_resolves_from_a_component() {
    let r = repo(&[
        (
            "web/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"$lib/*":["./src/lib/*"]}}}"#,
        ),
        (
            "web/src/routes/Page.svelte",
            "<script lang=\"ts\">\nimport { f } from '$lib/util';\nimport Button from '$lib/Button.svelte';\n</script>",
        ),
        ("web/src/lib/util.ts", ""),
        ("web/src/lib/Button.svelte", ""),
    ]);
    assert_eq!(
        kinds(&r.imports("web/src/routes/Page.svelte")),
        [
            edge(2, "web/src/lib/util.ts", ImportKind::Runtime),
            edge(3, "web/src/lib/Button.svelte", ImportKind::Runtime),
        ]
    );
}

#[test]
fn packages_and_dynamic_specifiers_are_handled_as_in_typescript() {
    let r = repo(&[
        ("package.json", r#"{"dependencies":{"svelte":"5"}}"#),
        (
            "A.svelte",
            "<script>\nimport { onMount } from 'svelte';\nimport 'left-pad';\nimport fs from 'node:fs';\nconst m = require(name);\n</script>",
        ),
    ]);
    assert_eq!(r.imports("A.svelte").resolved, []);
    assert_eq!(
        r.skipped("A.svelte"),
        pairs(&[(3, "left-pad"), (5, "name")])
    );
}

#[test]
fn an_existing_component_wins_over_an_appended_extension() {
    let r = repo(&[
        (
            "a/Main.ts",
            "import A from '../b/both.svelte';\nimport B from '../b/both';",
        ),
        ("b/both.svelte", ""),
        ("b/both.svelte.ts", ""),
        ("b/both.ts", ""),
    ]);
    assert_eq!(
        r.edges("a/Main.ts"),
        [(1, "b/both.svelte".to_owned()), (2, "b/both.ts".to_owned())]
    );
}

#[test]
fn without_the_component_the_module_next_to_it_resolves() {
    let r = repo(&[
        ("a/Main.ts", "import A from '../b/both.svelte';"),
        ("b/both.svelte.ts", ""),
    ]);
    assert_eq!(r.edges("a/Main.ts"), [(1, "b/both.svelte.ts".to_owned())]);
}

#[test]
fn an_extensionless_specifier_finds_a_component_after_every_script_extension() {
    let r = repo(&[
        ("a/Main.ts", "import X from './X';\nimport Y from './Y';"),
        ("a/X.svelte", ""),
        ("a/X.cjs", ""),
        ("a/Y.svelte", ""),
    ]);
    assert_eq!(
        r.edges("a/Main.ts"),
        [(1, "a/X.cjs".to_owned()), (2, "a/Y.svelte".to_owned())]
    );
}

#[test]
fn a_missing_component_is_unresolved_at_its_line() {
    let r = repo(&[
        (
            "a/A.svelte",
            "<script lang=\"ts\">\n  import ok from './Ok.svelte';\n  import Missing from './Missing.svelte';\n</script>",
        ),
        ("a/Ok.svelte", ""),
    ]);
    assert_eq!(
        r.unresolved("a/A.svelte"),
        pairs(&[(3, "./Missing.svelte")])
    );
}

#[test]
fn a_typescript_file_imports_a_component() {
    let r = repo(&[
        (
            "a/main.ts",
            "import App from './App.svelte';\nexport { default as Side } from './Side.svelte';",
        ),
        ("a/App.svelte", "<p/>"),
        ("a/Side.svelte", ""),
    ]);
    let got = r.imports("a/main.ts");
    assert_eq!(
        kinds(&got),
        [
            edge(1, "a/App.svelte", ImportKind::Runtime),
            edge(2, "a/Side.svelte", ImportKind::Runtime),
        ]
    );
    assert_eq!(
        got.resolved[0].symbols,
        BTreeSet::from([Symbol::Name("default".to_owned())])
    );
}

#[test]
fn a_test_component_is_test_code() {
    let r = repo(&[
        ("a/A.test.svelte", "<script>\nimport './b';\n</script>"),
        ("a/b.js", ""),
    ]);
    assert!(r.imports("a/A.test.svelte").resolved[0].test_only);
}
