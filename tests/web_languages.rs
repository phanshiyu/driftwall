//! End to end: a small Svelte web app with a Vite dev page, linted through the library. The edges
//! between directories come from `.svelte` scripts and from an HTML page, which no TypeScript-only
//! reading sees.

use driftwall::lint::{self, LintOptions};
use driftwall::results::{Severity, ViolationKind};

const APP: &str = "<script lang=\"ts\">
  import { store } from '../lib/store';
  import Card from './Card.svelte';
  import Chart from '../widgets/Chart.svelte';
  import Missing from './Missing.svelte';
</script>

<Card /><Chart />
";

const DEV_PAGE: &str = "<!doctype html>
<script type=\"module\">
  import { store } from '/src/lib/store.ts';
</script>
<script type=\"module\" src=\"/src/ui/main.ts\"></script>
";

const CONTRACT: &str = "flowchart LR
  ui[\"ui\"]
  lib[\"lib\"]
  widgets[\"widgets\"]
  dev[\"dev\"]
  ui --> lib
  ui --> widgets
  widgets --> lib
  dev --> lib
  dev --> ui
";

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::Path;

    use driftwall::lint::{self, LintOptions};
    use tempfile::TempDir;

    use super::{APP, DEV_PAGE};
    pub fn app(contract: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let files = [
            ("architecture.mmd", contract),
            ("web/package.json", "{}"),
            ("web/src/ui/.arch", "node: ui\n"),
            ("web/src/ui/App.svelte", APP),
            ("web/src/ui/Card.svelte", "<p>card</p>"),
            ("web/src/ui/main.ts", "import App from './App.svelte';\n"),
            ("web/src/lib/.arch", "node: lib\n"),
            ("web/src/lib/store.ts", "export const store = {};\n"),
            ("web/src/widgets/.arch", "node: widgets\n"),
            (
                "web/src/widgets/Chart.svelte",
                "<script>import { store } from '../lib/store';</script>\n",
            ),
            ("web/dev/.arch", "node: dev\n"),
            ("web/dev/index.html", DEV_PAGE),
            ("web/notes/page.html", "<h1>notes, no script</h1>\n"),
        ];
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    pub fn edges(root: &Path) -> Vec<String> {
        let observation = lint::observe(root, &LintOptions::default()).unwrap();
        observation
            .observed
            .iter()
            .flat_map(|(edge, sites)| {
                sites.iter().map(move |site| {
                    format!(
                        "{} -> {} at {}:{}",
                        edge.from,
                        edge.to,
                        site.at.file.display(),
                        site.at.line
                    )
                })
            })
            .collect()
    }
}

use support::{app, edges};

#[test]
fn svelte_scripts_and_an_html_page_make_the_cross_directory_edges() {
    let dir = app(CONTRACT);
    assert_eq!(
        edges(dir.path()),
        [
            "dev -> lib at web/dev/index.html:3",
            "dev -> ui at web/dev/index.html:5",
            "ui -> lib at web/src/ui/App.svelte:2",
            "ui -> widgets at web/src/ui/App.svelte:4",
            "widgets -> lib at web/src/widgets/Chart.svelte:1",
        ]
    );
}

#[test]
fn a_matching_contract_leaves_only_the_missing_component_to_report() {
    let dir = app(CONTRACT);
    let report = lint::run(dir.path(), &LintOptions::default()).unwrap();
    let found: Vec<_> = report
        .violations
        .iter()
        .map(|v| {
            let at = v.at.as_ref().unwrap();
            (v.kind, v.severity, at.file.display().to_string(), at.line)
        })
        .collect();
    assert_eq!(
        found,
        [(
            ViolationKind::UnresolvedImport,
            Severity::Warning,
            "web/src/ui/App.svelte".to_owned(),
            5
        )]
    );
}

#[test]
fn an_edge_only_a_component_uses_is_undeclared_without_a_contract_line() {
    let dir = app(&CONTRACT.replace("  ui --> widgets\n", ""));
    let report = lint::run(dir.path(), &LintOptions::default()).unwrap();
    let errors: Vec<_> = report
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .map(|v| {
            let at = v.at.as_ref().unwrap();
            (v.kind, at.file.display().to_string(), at.line)
        })
        .collect();
    assert_eq!(
        errors,
        [(
            ViolationKind::UndeclaredEdge,
            "web/src/ui/App.svelte".to_owned(),
            4
        )]
    );
}
