# driftwall design

driftwall enforces that `architecture.mmd` is the source of truth for a codebase. It extracts the
real import graph, compares it with the diagram, and reports every disagreement with a file and
line an agent can act on.

## Contract format

`architecture.mmd` is a strict subset of Mermaid `flowchart`. Anything outside the subset is a
hard error with a line number. Silently ignoring syntax would let a typo become a missing edge.

```
%% Comment lines start with %% and must be on their own line (no trailing comments).
%% Direction is LR, RL, TB or BT and must be the first statement.
flowchart LR
  subgraph shared["Shared"]
    model["model: ids, edges"]
  end
  %% node: id["label"] or a bare id. Boundaries group nodes, one level deep, no semantics.
  %% Planned edge, tolerated until built:
  lint -.-> model
  %% Implemented edge, must be backed by code:
  cli --> lint
  %% Optional label, ignored by the linter:
  cli -->|label| report
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
  `index.*`, extension probing (`.ts .tsx .js .jsx .mjs .cjs`), and `tsconfig`/`jsconfig` `paths` and
  `baseUrl` with relative `extends` chains. Project `references` are not followed.
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

## init

`driftwall init` reverse-engineers a baseline from an existing repository so that `driftwall lint`
on the result is clean at once. It writes `architecture.mmd` and the `.arch` tags (a contract
without tags would leave every file unmapped); `--no-tags` writes the contract only.

- Source files are discovered like `lint` does, so ignores are honored. No source files is an error.
- Each source file belongs to its ancestor directory at depth N from the repo root, or to its own
  directory when that is shallower. Root-level files belong to the root directory, node `root`.
  Every owning directory gets a `.arch` and becomes one node, labelled with its path.
- `--depth N` sets N. Without it, N is chosen from the layout: starting at the root, descend
  while the directory holds no source file itself and exactly one child directory holds sources.
  The directory where that stops (own source files, or two or more source-bearing children) is
  the fork; N is its depth plus one. So code under `src/` alone yields `src/auth` and
  `src/billing`, not one `src` node, while a repo with root files plus `lib/` yields `root`
  and `lib`, and one directory holding everything yields a single node.
- Ids come from the directory path below the longest prefix all node directories share, segments
  joined with `-`, other characters replaced by `-` (repeats collapsed, ends trimmed, `n-` prepended
  to a non-letter start). A directory that is the shared prefix itself is named by its last
  segment. Collisions and the reserved words `end`, `flowchart`, `subgraph` get `-2`, `-3`, ...
  in sorted directory order.
- Every resolved import between files with different owners becomes one solid edge. Nothing is
  dotted, so a fresh baseline has no stale or planned-realized findings.
- `init` refuses (exit `2`) when the contract or any `.arch` it would write exists, listing every
  such path, and checks all of them before writing anything. `--force` overwrites. Existing
  `.arch` files nested below a baseline directory would override its tag and are refused even
  with `--force` (remove them or use `--no-tags`).

## Exit codes

`0` clean. `1` violations. `2` tool error (bad diagram syntax, IO failure, bad arguments).
A failing lint is not an error, so `anyhow` is used only at the entry point and always maps to `2`.

## CLI surface

```
driftwall lint [PATH] [--format text|json|sarif] [--strict]
driftwall init [PATH] [--depth N] [--contract FILE] [--no-tags] [--force]
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

## Enforcement over instructions

Anything a tool can check is enforced by a tool and kept out of prose (`AGENTS.md`, the skill).
Style and error handling are clippy lints, unordered collections are `disallowed-types`, missing
docs are `missing_docs`, undeclared dependencies are `driftwall lint`. The agent skill carries
only judgment: when to discuss with the user, when to stop and ask. Everything else is a hook or a
command, for example a hook that runs `driftwall lint --format json` after edits.

The repo applies this to itself. `scripts/gate.sh` is the pre-commit hook, `scripts/hooks/guard.py`
blocks agent actions that weaken the gates (pkill, `--no-verify`, pushes, edits to the contract and
lint configuration from a worktree), and `tests/style.rs` requires every plain comment to start
with `why:`. The golden rule is to leave an agent as little room to err as possible.

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
