# Changelog

## Unreleased

- `%% deny <from>: <to>[, <to>...]` takes a pair out of an open chain. In `%% order open`, every
  downward pair of layers is allowed and an arrow narrows a pair; there was no way to say that
  layer `ui` may not use layer `core` at all. A denial covers every node of `from` (a node, or a
  boundary at any depth) against every node of `to`, and an import on a covered pair is the error
  `denied-edge`, of any kind of site and with no other finding for it. Contract errors, exit 2,
  at the line of the denial: an unknown id, a malformed line, the same pair twice, a declared edge
  that covers a covered pair, and a denial that no open chain makes necessary. `brief` and
  `neighbors` show the denials in `rules:` (JSON `rules.deny`) and mark a denied pair `DENIED`
  (JSON status `denied`), `diff` reports added and removed denials, and a baseline holds
  `denied-edge` like `undeclared-edge`. See "Denial" in `docs/DESIGN.md`.

- One finding per site for `isolation-breach` and `upward-edge`. Such a site no longer also reports
  `undeclared-edge`: declaring the arrow is a contract error under `%% isolated` and `%% order`, so
  that hint pointed at a fix that cannot work, and a feature-module repo reported 93 errors for 55
  sites. The hints now name only fixes that work, and an upward test-only site says to ask for the
  test edge `a -->|test| b`, which makes it clean. A site that is both keeps both findings. Edge
  status is unchanged: such a pair is still `undeclared` in `brief` and `neighbors`. A baseline
  recorded with 0.2.0 may report `stale-baseline` for `undeclared-edge` entries those sites used to
  need; rewrite it with `driftwall lint --write-baseline`.

## 0.2.0 - 2026-11-25

- Interface budget: `%% budget store: interface 5` limits how many distinct symbols of a node code
  outside it uses in production code, so a node stays deep behind a narrow interface. Re-exports are
  followed to the defining file (a `%% facade` file is the definer), test-only sites and imports
  inside the node do not count, and an import that names no symbol (a namespace, wildcard,
  side-effect or module import) counts once per target file as `<file> (whole module)`. Going over is
  the existing `over-budget` warning. `brief` and `neighbors` show `interface 9/5 over` and the JSON
  `budget.interface`; `neighbors` also lists the entries with their users in an `interface:` block
  (JSON `interface`) for a node with the limit, or with `--all-sites`.
- Debt baseline: `driftwall lint --write-baseline FILE` records today's findings as `<kind> <count>
  <key>` lines, and `--baseline FILE` then fails only on new debt. Keys hold no line numbers, a key
  with more findings than recorded reports all of them ("the baseline allows N, found M"), and fixing
  debt never fails. An entry with room to spare is the warning `stale-baseline`, so `--strict` makes
  the file shrink. Suppressed findings are counted in the text summary, in a JSON `baseline` object,
  and emitted in SARIF with `suppressions`. `driftwall diff --base REF --baseline FILE` reports
  debt added, which `--exit-code` fails on. See "Debt baseline" in `docs/DESIGN.md`.
- Grouped findings: the `lint` text output prints findings that share a key (the debt baseline's
  `<kind> <key>`) once, as a header with `in N places`, one `at file:line (target)` line per site and
  the hint once, instead of N near-identical errors. A lone finding prints as before, test-only sites
  stay a group of their own, and the baseline's "allows N, found M" shows once. The summary still
  counts findings and gains ` (K groups)` when something grouped. JSON and SARIF stay per site;
  JSON violations gain `"key": "<kind> <key>"` so a consumer can group the same way. See "Grouped
  findings" in `docs/DESIGN.md`.
- `diff --base REF` reports ownership moves: every source file whose owning node differs between
  REF and the working tree is listed under `moved between nodes:` as `file: old -> new`, with the
  cause `(mapping changed)` (a `%% map` or `.arch` edit), `(renamed from <path>)` (git rename
  detection) or both, and `(none)` for no owner. Moving code into the importing node, or editing
  a glob so a file changes owner, no longer hides an `undeclared-edge`. Added and deleted files
  are not moves. The owner at REF is computed by scanning the tree at REF like a lint would. JSON
  `moved`. `--exit-code` fails on a move, and there is no flag to allow one: a human approves it in
  review.
- `diff` now reports rule directive changes (`%% order`, `order open`, `isolated`, `each`,
  `budget`, `tests`, `packages`, `forbid-packages`, `facade`, `no-cycles`) as lines added and removed
  under `rules changed:`, and a node changing boundary (or a boundary changing nesting) as `node X
  moved from boundary A to B`. JSON `rules_added`, `rules_removed` and `boundary_moves`.
  `--exit-code` fails on these too. See "Diff beyond the contract" in `docs/DESIGN.md`.
- Edge status: every edge now has one named status (`ok`, `open`, `undeclared`, `stale`, `planned`,
  `planned-realized`, `test-only`), computed by one function that `lint` and `brief` share. `brief`
  and `neighbors` JSON gain `"status"` on every edge and boundary edge (additive: `kind` is
  unchanged, and `kind: null` still means undeclared), and their text ends an edge with `STALE`,
  `PLANNED-REALIZED` or `TEST-ONLY` where `UNDECLARED` stood, so it tells the story of `lint`.
  Lines of `ok` edges are unchanged. `lint` output is unchanged. See "Edge status" in
  `docs/DESIGN.md`.
- `diff --format json`: each `moved` entry gains `cause` (`mapping-changed`, `renamed` or
  `renamed-and-mapping-changed`). `renamed_from` and `mapping_changed` are unchanged.
- `where --format json`: gains `owned_by` (`tag` or `map`), absent when the file has no owner, so
  agents need not infer it from which keys are present. `node`, `arch_file` and `map` are unchanged.

## 0.1.1 - 2026-11-25

- Published to npm: `npm install --global driftwall`. This is the first release published by CI
  through npm trusted publishing; the README install section now leads with npm and the release
  binaries.

## 0.1.0 - 2026-11-24

- Edge templates: `%% each features: controller --> service, controller -.-> view` declares the same
  internal arrows for every child boundary of `features`, so a feature-module layout no longer
  repeats them or falls back on `billing --> billing`. Role `r` of child `billing` is the node
  `billing-r` inside it; a part a feature lacks is skipped, a role no feature has is a contract
  error with the line, and every arrow form of an edge works (`-.->`, `|test|`, `|type|`,
  `|type, test|`). An expansion lints like a declared edge: one `stale-edge` per template arrow (only
  when no child realises it, pointing at the `%% each` line), `redundant-edge` for an explicit edge
  it covers, never an `isolation-breach` arrow, and no narrowing of an `%% order open` pair. `brief`
  and `neighbors` show `via each features: controller --> service, line N` (JSON `via`) and the
  template in `rules:` (`rules.each`); `render` writes the template back.
- Open layer order: `%% order open: app > features > shared > core` marks a chain in which every
  layer may depend on any layer below it with no arrow, so seven layers need no layer-to-layer
  arrows. Upward imports stay `upward-edge`, and imports inside one layer still need arrows. Any
  declared edge of any kind (solid, dotted, `test`, `type`, boundary) from inside an upper layer to
  inside a lower one narrows that pair of layers to the declared edges; `undeclared-edge` then says
  that narrowing arrows exist and the pair is not among them. A plain `%% order:` is unchanged. The
  implicit allowance is ignored by `%% no-cycles`, is never stale, and does not make an arrow
  `redundant-edge`. `brief` and `neighbors` show `via order open: <chain>, line N` (JSON `via`) and
  `rules.open_order`.
- Nested boundaries: a `subgraph` may contain `subgraph`s, to any depth. A boundary's members are the
  nodes inside it at any depth, and every rule uses them: `features --> shared` covers the nodes of
  every feature inside `features`, `%% order` lists an outer boundary as one layer (listing a
  boundary with one nested in it, in one chain, is a contract error), and `%% no-cycles: boundaries`
  checks every boundary at every level. A boundary edge between nested ends is allowed. `render`
  writes the nesting back. An `end` closes the innermost `subgraph`; an unclosed one is reported at
  the innermost.
- Isolation: `%% isolated: <boundary>[, <boundary>]` keeps the children of a boundary (each child
  boundary, and each node written directly in it) from depending on each other. An import from one
  child to another is the error `isolation-breach`, test and type-only sites included, and no arrow
  can allow it: a declared edge between two children, node or boundary edge of any kind, is a
  contract error with its line. An isolated boundary with fewer than two children is the warning
  `stale-isolation`. `brief` shows `isolated: features (a, b, c)` in `rules:` (JSON `rules.isolated`).

- Boundary edges: an edge endpoint may be a `subgraph` id, so `features --> core` allows every node
  of `features` to depend on every node of `core` (and `a --> sub`, `sub --> a`, `sub --> sub` work
  too, with every arrow and label: dotted, `|test|`, `|type|`, `|type, test|`). `features --> features`
  lets the members depend on each other. Lint allows a dependency when any covering edge allows its
  site, and `undeclared-edge` stays as it was for a pair nothing covers. A boundary edge is stale
  only when none of its pairs is realised, and `planned-realized` and `test-only-edge` follow the
  same rule over all pairs. A node edge that a boundary edge of a kind at least as wide already
  covers is the new warning `redundant-edge`. `%% order` rejects a boundary edge that points up, and
  `%% no-cycles` expands boundary edges to node pairs and names their lines. `brief` and
  `neighbors` show `via features --> core, line 31` for each pair and list the boundary edges
  (JSON `via`, `boundary_edges`), and `diff` reports them (`added_boundary_edges` and friends).
  `init` still writes node edges. An edge to a boundary with no node is a contract error.

- Facades: `%% facade <node>[, <node>]` marks a node that deliberately presents other nodes' items
  as its own. An importer outside it that imports a name through a re-export in one of its files
  depends on that file, and the walk does not go past it; the facade's own re-exports keep their
  edges to the definitions, and inside the node nothing changes. `exports` is judged at the facade
  node. A facade node that re-exports nothing is the new warning `stale-facade`; an unknown node
  or a node listed twice is a contract error with its line. `brief` shows `facade: ...` in its
  `rules:` block and JSON `rules.facades`.
- Lint marks findings at test sites: the text puts `(test code)` after the kind
  (`src/a.test.ts:3: error[undeclared-edge] (test code): ...`), JSON has `"test_only": true`, and a
  SARIF result has `"properties": {"testOnly": true}`, each only on a finding at a site written in
  test code (`undeclared-edge`, `private-symbol`, `upward-edge`). Other findings are unchanged.
- A dotted edge `a -.-> b` is no longer `planned-realized` when only test code uses it. It is the
  warning `test-only-edge` instead: the app does not use the dependency yet, and the message says to
  declare `-.->|test|` or `-->|test|` if tests alone should use it. One production site still makes
  it `planned-realized`; a dotted `|test|` edge realised by tests is unchanged.
- Re-exports: an import of a name through a module that re-exports it is attributed to the file
  that defines the name, so `use crate::ui::Tone;` with `pub use tone::Tone;` in `ui/mod.rs` is a
  dependency on `ui/tone.rs`, and that file can be its own node. Rust `pub use` (also `pub(crate)`,
  groups, `as`), TypeScript and JavaScript `export { X } from`, `export type`, `export *`, and
  Python `from .x import Y` in an `__init__.py` are followed, through chains, with cycle protection.
  A glob is followed only when exactly one source can define the name. Module imports, `*`, names
  defined in the module itself and names nothing explains stay on the module file, and the
  re-exporting module keeps its own edge. `exports` and `private-symbol` are judged at the
  definition, under its name. See "Re-exports" in DESIGN.md.
- `brief` and `init`: summary lines count in the singular when there is one (`1 error`, `1 edge`, `1 .arch file`), as `lint` does.
- README: rewritten in plain language (the idea, a worked example with real output, quick start, a contract cheat sheet linked to DESIGN.md). `tests/readme.rs` now fails when a README command or flag is not in the CLI, or a link or DESIGN.md anchor is missing.

- Layer order: a contract may have several `%% order` lines, each an independent chain, for repos
  with separate stacks. A boundary may be in several chains only if they agree on every pair they
  share; a contradiction is a contract error naming both lines. Upward-edge checks apply per
  chain, one finding per site. JSON `rules.order` is now a list of chains (it was one list).
- Release: the npm package is published through npm trusted publishing (an OIDC id-token from
  `.github/workflows/publish-npm.yml`), so no `NPM_TOKEN` secret is needed. `tests/release.rs` pins
  it, and the hand fix for dist 0.34's `uses: $/` path in the generated `release.yml`.
- Test isolation: `%% tests: isolated` makes a production import site of a test file the error
  `production-imports-test`, on any pair and inside one node, by the same test classification
  that decides `test_only` (see "Import kind"). `brief` shows `tests: isolated` in its `rules:`
  block (JSON `rules.tests_isolated`).
- Budgets: `%% budget <node>: fan-out <n>, files <n>, lines <n>` (any subset) limits a node, and
  exceeding a limit is the new warning `over-budget`, at the budget line, naming the measure, the
  limit and the actual value. Fan-out counts the distinct nodes the node depends on in production
  code, files and lines are the source files the node owns. `brief` and `neighbors` show
  `budget: fan-out 3/5, files 21/20 over` per node (JSON `budget`).
- No cycles: `%% no-cycles` makes the declared edges, test edges excluded, a DAG over nodes, and
  `%% no-cycles: boundaries` does the same for the boundary graph. A declared cycle is a contract
  error at the directive's line that lists the cycle with its edge lines. `brief` shows both lines
  in its `rules:` block (JSON `rules.no_cycles`, `rules.no_cycles_boundaries`).
- Layer order: `%% order: entry > engine > shared` lists boundaries from the top to the
  foundations, and an import from a node in a lower listed boundary to one in a higher boundary is
  the new error `upward-edge`, whatever arrows the contract draws. A declared arrow that points
  upward is a contract error at its line (a `-->|test|` edge may point up, and exempts its
  test-only sites). Nodes in one boundary or outside the listed ones are unconstrained. `brief`
  and `neighbors` show each node's layer and the order (JSON `layer`, `rules.order`).
- Type edges: `a -->|type| b` (and the planned `a -.->|type| b`) allow the dependency from
  type-only import sites (TypeScript `import type`, type-position `import()`, JSDoc types, Python
  `if TYPE_CHECKING:`) and nothing else. A runtime import on a pair with only a type edge is an
  `undeclared-edge` error that says only type-only imports are allowed; a type edge no type-only
  import realises is `stale-edge`; a dotted one a type-only import realises is `planned-realized`.
  The label `type, test` allows type-only or test-only sites. Rust, Swift and shell have no
  type-only imports. There is no warning for a solid edge realised only by type imports: `brief` and
  `neighbors` mark it `could be -->|type|` (JSON `type_candidate`). `brief`, `neighbors` and `diff`
  show the kinds (`type`, `planned-type`, `type-test`, `planned-type-test` in JSON).
- Package visibility: `brief` and `neighbors` list the packages each node's files use (name, kind,
  number of uses and how many are in test code), third party first, as `packages: lit
  (third-party, 4), node:fs (stdlib, 2)` in text and `packages` (`kind`, `name`, `count`, `tests`)
  in JSON, absent for a node that uses none. `where <file>` lists the package uses of a source file
  (`  package lit (third-party) line 3`, JSON `packages`). `init` drafts a `%% observed packages
  <node>: ..` comment per node with third-party packages in production code: a plain comment, a
  starting point that is never enforced until written as a `%% packages` rule.

- Package rules: `%% packages <node>: <name>, ...` is an allow-list of third-party packages and
  `%% forbid-packages <node>: <name>, ...` a deny-list of any package, the standard library
  included (`node:child_process`, `subprocess`, `lit`). A use that is not allowed, or is denied, is
  the error `forbidden-package` at the import line. A trailing `*` makes a prefix (`@scope/*`,
  `serde*`, `node:*`), several lines per node add up, the deny-list wins, an allow-list leaves the
  standard library alone, and `%% packages config:` (no name) allows no third-party package. Test
  code is exempt from both. An allow entry no non-test code uses is the warning `stale-package`.
  An unknown node, a malformed or contradictory line is a contract error with its line number.

- Package uses: every import of code outside the repository is recorded per file as a package use
  (`FileImports::packages`: name, kind `third-party` or `stdlib`, location, `test_only`), the data
  the coming package rules read. TypeScript, JavaScript, Svelte and HTML: a Node builtin is stdlib
  named `node:<module>` (`fs/promises` is `node:fs`), any other bare specifier is third-party named
  by its package (`@scope/pkg`, `pkg`); workspace packages, aliases and virtual modules are no
  package. Python: a module no import root resolves and no first-party name matches, stdlib by an
  embedded `sys.stdlib_module_names` of Python 3.13. Rust: `std`, `core`, `alloc` and the crates the
  nearest `Cargo.toml` lists. Swift: an `import` of a module no `Package.swift` target declares,
  stdlib when it is an Apple system framework on a documented list. Skipped references are unchanged:
  `undeclared-package` (TypeScript, JavaScript, Python namespace directories) is also a third-party
  use, and no other reason is a package.

- Contract map specificity: a glob that matches files by pattern now beats the subtree glob of
  its directory (`ui/src/components/*.test.tsx` over `ui/src/components/`, `src/*.ts` over `src/`),
  then a longer literal prefix wins (`src/a*.ts` over `src/*.ts`), then a longer literal suffix
  (`src/*.test.ts` over `src/*.ts`). Depth still comes first, and true ties stay contract errors.
- Ids are one namespace: a node and a `subgraph` with the same id is a contract error at the line of
  the second declaration (it was accepted silently). Two subgraphs with one id already were.
- A Vite alias to a build output (`mrc-wasm` to `engine/wasm/pkg`, no file in the repository) inside
  a directory a node owns now counts as a dependency on that node: an edge site of kind runtime with
  `via: "alias-to-build-output"` in JSON, so a solid edge is realised, a planned edge is
  `planned-realized`, and an undeclared one is `undeclared-edge`. It also stays listed as the
  skipped `alias-target-missing` reference. `brief` and `neighbors` count it as `alias-to-build-output`.
- Test edges: `a -->|test| b` (and the planned `a -.->|test| b`) allow the dependency from
  test-only import sites and nothing else. A production import on a pair with only a test edge is
  an `undeclared-edge` error that says only test imports are allowed; a test edge no test import
  realises is `stale-edge`; a solid edge realised only by test code is the new warning
  `test-only-edge` (declare it `-->|test|`). `init` writes an edge only tests use as `-->|test|`
  and leaves it out of the layer order. `brief`, `neighbors` and `diff` show the kind (`test`,
  `planned-test` in JSON); `a -- test --> b` is a contract error and other labels stay ignored.
- `init` drafts read as an architecture: nodes are grouped into `subgraph` boundaries by code unit
  and directory, labelled `id: description` where a Rust `//!` doc, a Python docstring, a JSDoc
  file block or a manifest description states one (else the path), written from entry points to
  foundations, with a `%% cycles: a <-> b` comment per dependency cycle (tests-only cycles named
  apart) and a summary comment. Presentation only: the nodes, tags and edges are unchanged and
  `lint` stays clean on a fresh baseline.
- Contract map: a `%% map <node>: <glob>, <glob>` line in the contract maps files to nodes, so a
  hand-designed diagram (even `docs/architecture.mmd`) needs no `.arch` files. The most specific
  glob wins and an equal tie is a contract error, `%% map <prefix>*: <dir>/` is the module form,
  a glob that matches nothing is the warning `stale-map`, and `.arch` files are not read once the
  contract has map lines (the warning `ignored-tag-files` lists them). `where` names the contract
  line and glob, `brief` and `neighbors` show the globs, and `init --map` writes the mapping as
  map lines instead of `.arch` files.
- Rust: `#[path = "b/impl.rs"] mod a;` places module `a` at that file (relative to the declaring
  file, with the inline-module and non-mod-rs rules), so `crate::a::..` resolves to it instead of
  producing a false edge to the crate root file. A `#[cfg(test)] #[path = ..] mod` makes its file
  test code.
- Rust: an inline module that has `use super::*;` (typically `#[cfg(test)] mod tests`) sees the
  names its parent binds, so `depgraph::build(..)` after `use crate::depgraph;` is recorded there
  (test-only inside `cfg(test)`). `use super::name;` re-imports the parent's binding.
- Initial proof of concept: contract parser, ownership tags, TypeScript, JavaScript, Python and
  Rust import extraction, `lint`, `init` and `where`.
- `brief`, `neighbors` and `diff`: a deterministic map of the architecture for agents, and a
  deterministic diff of contract changes for reviewers.
- Module nodes: `node: *` in a `.arch` file makes every module in that directory its own node, and
  `init` writes it when the source root directly holds two or more modules.
- Public surfaces: a `%% exports node: Name, Name` line in the contract declares what a node
  exposes, and the new `private-symbol` rule fails imports of anything else. `brief` and
  `neighbors` list the symbols each edge uses, and `diff` reports surface changes.
- Rust module bindings (`use crate::gitlab;`, `as gl`, `self` in a group) use no symbol; only a
  glob uses `*`, and `*` is always `private-symbol` for a node with a surface.
- An import is located at the line its statement starts on, not at its specifier.
- Every resolved import records its `kind` (`runtime`, `typeonly`, `embed`) and `test_only`. Rust
  `include_str!`, `include_bytes!` and `include!` of a source file are `embed` imports that use no
  symbol, and surfaces do not restrict them.
- Skipped references: undeclared packages, computed specifiers and embeds of non-source files are
  listed in `FileImports.skipped` instead of being dropped. A malformed `package.json` is ignored.
- TypeScript and JavaScript resolution: workspace packages imported by name (`exports`, `module`,
  `main`, `types`), tsconfig `references`, package `imports` (`#internal/*`), `.d.ts` probing,
  bundler query suffixes (`?raw`, `?url`), and every ancestor `package.json` for declared packages.
- `init` v3: every unit (crate, directory with a `package.json`, `tsconfig.json` or `jsconfig.json`,
  Python package) has its source directory split into module nodes, other units get prefixed ids
  (`node: ui-*`), the root group is tagged `scope: here` so new directories are `unmapped-file`,
  `__main__.py` and leading underscores are valid module names, and `dist/` and `@generated` files
  are excluded as `generated`. `init` counts asset imports like `lint`.
- Asset imports: a relative import of an existing non-source file (`./data.json`, `./style.css`) is a
  dependency on the node that owns it, with the new import kind `asset`. A relative import of a
  missing file of any extension is now `unresolved-import`. The import kind `typeonly` is now
  serialized as `type-only`.
- Svelte and HTML sources: `.svelte` files are read through their `<script>` elements (TypeScript
  with `lang="ts"`, else JavaScript) with the lines of the component. An `.html` or `.htm` file is
  source only when it holds a `<script` element; its inline scripts, `<script src>` and
  `<link rel="modulepreload">` are imports, root-absolute paths resolve against the Vite root and
  remote URLs are ignored. An existing `.svelte` file wins over `x.svelte.ts`, and an
  extensionless specifier finds a component after every TypeScript and JavaScript extension.
- Blind spots: the scan keeps a census of what it cannot see, the code files in a language it does
  not read (by extension, with lines), the files only `.driftwallignore` hides, excluded files, skipped
  references and unresolved imports, each attributed to its node. `brief` ends with a `blind spots`
  block and node lines carry their counts, `neighbors` lists a node's items (and its files with
  `--files`), `lint` and `init` print one `note:` line (and `lint --format json` gains `blind_spots`),
  and none of it changes an exit code. Edges show `(type-only)`, `(test-only)`, `(embed)`, `(asset)`
  or per-kind counts, imports of a module itself list `module` in `uses:`, and `neighbors --all-sites`
  prints every site. `where` accepts `--contract`, fails with `no such file` for a missing path and
  says what a non-source file is.
- Swift and shell sources. `.swift` files are read for `import` declarations, resolved through the
  targets of `Package.swift` (`.target`, `.executableTarget`, `.testTarget`, `.macro`, `.plugin`,
  default `Sources/<name>` and `Tests/<name>`) to the entry file of the module, and `init` makes one
  node per Swift target, named after it. `.sh`, `.bash` and `.zsh` files are read for `source` and
  `.` commands with literal paths, script-relative paths (`$(dirname "$0")`, `${BASH_SOURCE%/*}`,
  `ROOT="$(cd ... && pwd)"` variables) and globs; any other path is skipped as
  `dynamic-specifier`. Files with no extension whose first line is a `#!` shebang naming `sh`,
  `bash`, `zsh`, `dash` or Python are source too. `.githooks/` and `integrations/` are ignored by
  the repository's own `.driftwallignore`.
- `init` grouping: only the primary unit's source directory, Rust crates and Python packages get one
  node per module. Every other JavaScript unit (including one declared only by a `tsconfig.json`) is
  split by subdirectory, with the loose files of its source directory as one `scope: here` node. The
  root files are one `root` node when the root also has source directories. A test file (`*.test.*`,
  `*.spec.*`, `test_*.py`, `*_test.py`, `*_test.rs`, `*_tests.rs`) joins the module it tests, or else the
  node of its directory, and never makes a node of its own.
- Python namespace directories: a miss below a top-level directory with no `__init__.py` (for
  example `from vendorkit.core.x import Y` when `vendorkit/` is an installed package that the
  repository only extends) is no longer an `unresolved-import`. It is skipped as `undeclared-package`
  with a message; directories with an `__init__.py` keep the warning.
- `init` layer split: a node of 35 or more source files with at least one child directory holding 8 or
  more of them (and at most 4 such children) is cut one level further: one node per large child
  directory (`components-settings`, `components-ui`), the rest stays in the parent node.
- `init` fresh-baseline invariant: an asset owned by a name that only an asset derives (`app.css`
  beside `App.svelte`, `src/index.css`, a directory of fonts) is declared as a node, so `lint` right
  after `init` has no `undeclared-owner` skipped reference.
- Python file loaders: `importlib.machinery.SourceFileLoader(name, "<path>")` and
  `importlib.util.spec_from_file_location(name, "<path>")` with a literal path naming a repository
  file are runtime imports of that file; any other path is skipped as `dynamic-specifier`.
- The not-read census also counts `cmd` and `bat` files (`ps1` already was). The `init` and `lint`
  notes agree on a fresh baseline: the only skipped references `lint` alone could add,
  `undeclared-owner`, no longer arise from `init`'s own tags.
- Excluded files (vendored, minified, generated) are import targets: an import of one resolves to
  it, is a runtime import with symbols and an edge to the node that owns it (`Ownership::excluded_owners`),
  and is no longer a false `unresolved-import`. They are still never parsed or unmapped. `init`
  declares the same edges.
- Vite `resolve.alias` (object and array forms, literal paths) is applied after `tsconfig` `paths`.
  An alias whose target has no file in the repository (a build output) is skipped as the new
  `alias-target-missing`, naming the alias, the target and the node whose directory holds it, instead
  of `undeclared-package`.
- Rust: a file pulled in by `#[cfg(test)] mod x;` (and the modules it declares) and any `tests.rs`
  are test code. A destructured `require` or `await import` records the names it takes. JSDoc
  `import('x').T` types in `/**` comments are type-only imports. `unreachable-module` only names
  Python modules and packages.
- Vite config paths: `path.resolve('lit')` and `path.join('lit')` with a literal first argument are
  relative to the config directory, and `process.cwd()` is the config directory, so
  `alias: { w: path.resolve('wasm/pkg/w.js') }` is `alias-target-missing` instead of
  `undeclared-package`.
- `init` labels name paths that exist: the node of orphan tests is labelled with its directory
  (`src`, not `src/src`) and a node only an asset names with the asset's path (`src/index.css`).
- HTML `<link>` elements with a `rel` of `stylesheet`, `preload`, `prefetch`, `icon`,
  `apple-touch-icon` or `manifest` are asset references to their `href`, resolved like a script `src`.
- Vite virtual modules (`virtual:pwa-register`, a `\0` prefix) are ignored instead of being skipped
  as `undeclared-package`.
- TypeScript types: `let x: import("./a").T`, a field typed `import("./a").T` and `typeof import("./a")`
  are `type-only` imports using `T` (the first member) or `*`, not runtime `*`.
- Python `unreachable-module` also matches a dotted module nested below directories that hold no Python
  (`from controllers.generic.carry_ctl import X` with `vendorkit/controllers/generic/carry_ctl.py`): it is
  no longer a silent drop.
- `brief` lists the skipped references the `note:` line points to, grouped by reason with file:line, specifier
  and message (text capped at 20 with `+N more, see --format json`; JSON `blind_spots.skipped_references` is
  complete). A node that owns only assets or excluded files shows them (`app: src/app.css (0 source files,
  1 asset)`); `NodeSummary` JSON gains `assets`, `neighbors` gains an `assets:` line and JSON `asset_files`.
- Rust `include_str!`, `include_bytes!` and `include!` of a non-source file that a tag covers (`static/ui.css`,
  icons, fonts) are `embed` edges to its owner, as a TypeScript `?raw` import is; `embed-non-source` remains
  for a missing file, a path that leaves the repository and a file no tag covers.
- Folded entry files: an `__init__.py` with no statement but a docstring, and a `lib.rs` or `main.rs` with only
  comments, attributes and `mod` declarations, make no node of their own under `node: *` unless a module has
  their name (`src-mybot-mybot`, a trivial `lib` node). They are known files, in no owner map; `where` says
  `folded, no node`, and `init` tagless of this repository reproduces exactly the nodes of its contract.
- `init` reads the imports again when a Rust embed of an asset was skipped before the tags were known, so its
  note counts the same skipped references as `lint` on the baseline it writes.
- File references: `new URL('<literal>', import.meta.url)` in TypeScript, JavaScript, Svelte and HTML scripts is a dependency on the file it names, relative to the importing file: a source file is a `runtime` import that uses no symbol, any other file an `asset`. A directory, a missing file or a path outside the repository is the new skipped reason `file-reference-missing` (never a finding), and a non-literal first argument is `dynamic-specifier`.
- `brief` caps the skipped-reference list per reason (5 each, then `+N more, see --format json` under that reason) instead of 20 across all reasons, so a rare reason such as `alias-target-missing` is never hidden behind a common one.
- `init` no longer lets a module id such as `engine` (from `src/engine/`) share a name family with the ids of another directory (`engine-core-*` of a crate in `engine/`): the plain `node: *` of `src/` is then written as `node: src-*` (`src-engine`). Repositories without such a collision keep their ids.
- `where` names the owner of an excluded or ignored file (`excluded (generated), owned by route-table (via src/.arch)`; JSON `node` and `arch_file` carry it), says `tag file` for an `.arch` file and `contract` for the contract (JSON kinds `tag-file`, `contract`), and none of them is an asset any more.
- Excluded files are counted apart from assets: a node line reads `0 source files, 1 excluded`, `NodeSummary` JSON gains `excluded` (and `assets` counts assets only), `neighbors` gains an `excluded:` line and JSON `excluded_files` (`asset_files` lists assets only). The JSON `dirs` of a node is documented as paths: a directory, or the file of a one-file module.
