# driftwall design

driftwall enforces that `architecture.mmd` is the source of truth for a codebase. It extracts the
real import graph, compares it with the diagram, and reports every disagreement with a file and
line an agent can act on.

## Contract format

`architecture.mmd` is a strict subset of Mermaid `flowchart`. Anything outside the subset is a
hard error with a line number. Silently ignoring syntax would let a typo become a missing edge.

```
%% comment line (also blank lines)
flowchart LR                      %% direction: LR | RL | TB | BT, must be first statement
  subgraph shared["Shared"]       %% boundary: grouping only, no semantics, one level deep
    model["model: ids, edges"]    %% node: id["label"] or bare id
  end
  lint -.-> model                 %% planned edge (tolerated until built)
  cli --> lint                    %% implemented edge (must be backed by code)
  cli -->|label| report           %% optional label, ignored by the linter
```

- Node ids match `[A-Za-z][A-Za-z0-9_-]*`. Labels are display only.
- Edge endpoints must be declared nodes. Chained edges (`a --> b --> c`) and `&` fan-out are errors.
- `-->` is `EdgeKind::Implemented`. `-.->` is `EdgeKind::Planned`.
- Edges between nodes in different boundaries are normal. Boundaries never carry edges.

## Ownership

A `.arch` file contains `node: <id>` (comments start with `#`). It tags its directory and all
subdirectories, until a nested `.arch` overrides it. Source files are found by walking the repo
with the `ignore` crate, so `.gitignore` is honored. `.driftwallignore` (gitignore syntax) adds
exclusions. Only files with a supported extension count as source.

## Import semantics

- An import is an edge only when it resolves to a repository file owned by a different node.
- Imports inside one node are ignored. Third party and standard library imports are ignored.
- A relative import that resolves to nothing is an `unresolved-import` warning, never silently
  dropped.
- Files with no owner never produce edge findings (no cascade from `unmapped-file`).
- Resolution per language: TypeScript and JavaScript handle relative specifiers, directory
  `index.*`, extension probing (`.ts .tsx .js .jsx .mjs .cjs`), and `tsconfig` `paths` (later).
  Python handles relative imports, `__init__.py`, and absolute imports rooted at the repo root or
  at `src/`. Rust handles `use crate::`, `use super::`, and `mod` declarations are containment,
  not edges.
- Extraction is tree-sitter based and driven by a per-language table (extensions, grammar, query),
  not a trait hierarchy. Adding a language means adding a table entry and a query.

## Rules (violation kinds)

| Kind | Severity | When |
| --- | --- | --- |
| `unmapped-file` | error | source file has no inherited `.arch` tag |
| `unknown-node` | error | `.arch` names a node the diagram does not declare |
| `undeclared-edge` | error | code imports across nodes and the diagram has no such edge |
| `stale-edge` | warning | solid edge with no import backing it |
| `planned-realized` | warning | dotted edge now backed by code, promote it to solid |
| `unresolved-import` | warning | relative import resolves to nothing |

A dotted edge permits the import. `--strict` turns every warning into a failure.

## Exit codes

`0` clean. `1` violations. `2` tool error (bad diagram syntax, IO failure, bad arguments).
A failing lint is not an error, so `anyhow` is used only at the entry point and always maps to `2`.

## CLI surface

```
driftwall lint [PATH] [--format text|json|sarif] [--strict]
driftwall init [PATH] [--depth N] [--write-tags] [--force]
driftwall where <FILE>          # owning node of a path, with the .arch file that decided it
driftwall neighbors <NODE>      # incoming and outgoing edges, declared and observed
driftwall diff --base <REF>     # edge delta versus a git ref, for human review (later)
```

`--format json` is the agent interface. Every violation carries `kind`, `severity`, `at`
(`file`, `line`), `message`, `hint`.

## Module map

The contract for this repository is `architecture.mmd`, enforced on itself once Rust support
lands. `src/<node>/.arch` tags each module. `model` has no outbound edges. `report` only renders
`model` types. `lint` and `init` are the only modules that combine `diagram`, `ownership` and
`imports`.

## Quality bar

- Lints live in `Cargo.toml` `[lints]`. CI runs `cargo clippy --all-targets -- -D warnings`.
- No `unwrap`, `expect`, `panic`, indexing or slicing outside tests. `unsafe_code` is forbidden.
- `thiserror` in the library, `anyhow` only in `src/cli`. Only `src/cli` prints.
- Every collection that reaches output is ordered. Output is byte-identical across runs.
- Real strictness is tests: fixture repos per language, `insta` snapshots for text and JSON
  output, and driftwall linting itself.

## Milestones

1. M1 ownership: walk, `.arch`, ignores, `where`. No parsing.
2. M2 diagram: parser and renderer for the subset above.
3. M3 imports (TS/JS): extraction, relative and index resolution.
4. M4 lint: reconcile, `report`, JSON output, exit codes. Usable end to end here.
5. M5 imports (Python).
6. M6 init: baseline with `--depth` and `--write-tags`.
7. M7 imports (Rust), then driftwall lints itself in CI and the dotted edges turn solid.
8. M8 agent integration: Claude Code skill and hook, `neighbors`, `diff`.
9. M9 release: cargo-dist builds, npm package, GitHub release.
