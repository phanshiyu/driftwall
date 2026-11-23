# Dogfooding driftwall on real projects

driftwall was checked against nine real repositories, itself included (TypeScript, React, Svelte,
Python, Rust, Swift, shell), in two phases. In both, an independent review agent judged
driftwall's output against the code with a fixed rubric, the defects it found were fixed in
driftwall (never hand-tuned in the project), and the loop repeated until every project passed.

## Phase 1: accuracy of the code graph

`scripts/dogfood.sh` runs a tagless `driftwall init`, `lint` and `brief` on each project and keeps
the evidence; a reviewer then checks it against [accuracy-rubric.md](accuracy-rubric.md): runtime,
granularity, edge precision and recall (every import site, both directions, by an independent
parser), symbols, blind spots and usefulness.

Five rounds took all nine projects from unsatisfactory to satisfactory. Final recall was complete
on every project (for example 592/592 cross-component import sites on one, 671 on another), with no
false edges. Defects found and fixed on the way include: whole source trees collapsing into one
node; Svelte, Swift and shell code passing unread and unreported; generated files reported as
missing imports; Vite roots and aliases; tsconfig references and workspaces; Python namespace
directories mistaken for first-party packages; Rust test modules, `use super::*` and `#[path]`; and
file references through `new URL(..., import.meta.url)`. Tagless `init` on driftwall itself
rediscovers its hand-written contract exactly (9 nodes, 19 edges).

## Phase 2: designed contracts

An accurate graph is an inventory, not a design. For seven projects a designer agent wrote a
contract from `init`'s draft and the project's own docs (lint zones, CLAUDE.md, ADRs, design docs):
a small set of components with responsibilities, layers, and arrows for the dependencies the design
allows, mapped to code with `%% map` lines. A reviewer judged each against
[design-rubric.md](design-rubric.md); the strictest rule is that debt is never declared as an
arrow, so lint shows it. Every design needed a second round; all passed.

| Project | Components | Arrows (test-only) | Violations lint reports |
|---|---|---|---|
| TS/React PWA | 15 | 48 (8) | 10 |
| follow-me | 16 | 53 (15) | 1 |
| TS dashboard, Python scripts | 15 | 34 (12) | 4 |
| mr-cast | 22 | 58 (15) | 31, from 4 root causes |
| Rust web app | 15 | 34 (2) | 38 |
| Python service, TS UI | 17 | 50 (11) | 6 |
| Svelte, Swift, shell | 16 | 19 (2) | 1 |

Each project's `docs/` now holds `architecture.mmd`, `architecture-notes.md` (the reasoning and a
fix for each violation) and `driftwall-dogfood.md` (the review). Driftwall features that came out of
this phase: `%% map` lines (no `.arch` files needed), `init` drafts with boundaries, labels from the
code, layer order and cycles, `-->|test|` edges, file-pattern map specificity, and aliases into a
build output counted as dependencies.

Known gaps the designs ran into, recorded in milestone M11 of [DESIGN.md](../DESIGN.md): type-only
edges, per-component package rules, dependencies made through paths in strings or subprocess calls,
and import roots set only in a Makefile `PYTHONPATH`.
