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

### Excluded files

Minified and vendored code is not the project's own source, so the scan leaves it out by default.
A source file is excluded when

- it is below a directory named `vendor` or `third_party` (at any depth), reason `vendored`; or
- its name has the form `<name>.min.<ext>` (the second to last dot-separated part is exactly
  `min`: `htmx.min.js`, `app.min.mjs`, `site.min.css`), reason `minified`.

A file that matches both is `vendored`. Excluded files are never owned, never unmapped and never
resolve as import targets. They are not silently dropped: `Ownership::excluded` maps each reason to
the sorted set of excluded paths, so a report can show them. `.arch` files are never excluded.

A project opts files back in with a negated pattern in the `.driftwallignore` at the repository
root (`!vendor/mine/`, `!static/app.min.js`): a file whose path that file whitelists is source
again. Only the root `.driftwallignore` is consulted for opt-in; negations in nested
`.driftwallignore` files still work for ignoring (the walker applies them) but do not opt back in.

### Asset files

Every other file the walk reaches (`.json`, `.css`, `.svg`, `package.json`, a README, ..) is an asset.
Assets get an owner by the tag rules of source files: the nearest `.arch` decides, and under `node: *`
the module rule names the node by the file name up to its first `.` (so `a.css` joins `a.ts`).
`Ownership::asset_owners` maps each owned asset to its node and `Ownership::asset_unmapped` lists the
ones no `.arch` covers. Assets are never reported as `unmapped-file` and never create `.arch` tags to
validate, and they change nothing about `owners`, `unmapped`, `excluded` or any file count: a node's
file count is its source files. An asset whose derived module name is not a valid node id
(`.eslintrc.json` under `node: *`) is listed in `asset_unmapped` and is never an error, because an
asset must not be able to fail the scan. The vendored and minified rules (see "Excluded files") apply
to source only, so an asset below `vendor/` is still an asset. `.arch` files are not assets, and ignored
files (`.gitignore`, `.driftwallignore`) are not walked, so they are neither assets nor resolvable.

### Module nodes (`node: *`)

A `.arch` file may contain `node: *` instead of an id. Then every module directly in that
directory is its own node, named by a fixed rule, with no further tagging:

- A source file is a module. Its node id is the file name up to the first `.`, so `gitlab.rs` is
  `gitlab` and `gitlab.test.ts` joins `gitlab`.
- Entry files (`mod.rs`, `lib.rs`, `main.rs`, `index.*`, `__init__.py`) are named by their file stem
  too (`main`, `lib`), except `mod.rs`, `index.*` and `__init__.py`, which are named after their
  directory.
- A subdirectory whose name equals a sibling module's id (Rust `gitlab.rs` plus `gitlab/`) belongs to
  that module's node. Any other subdirectory is one node named after the directory.
- A nested `.arch` still overrides, as everywhere else.
- A derived id that is not a valid node id is an error at the `.arch` file, never silently renamed.

`init` writes `node: *` into a directory that directly holds two or more modules (see `init`). An
entry file directly in the repository root (`index.ts`, `__init__.py`) is named `root`, since the
root directory has no name of its own. `unknown-node` reports a derived node the contract does not
declare at the `node: *` line of its `.arch` file.

## Import semantics

- An import is an edge only when it resolves to a repository file owned by a different node. The
  file is a source file or, for TypeScript and JavaScript, an asset (see "Asset files").
- Imports inside one node are ignored. Third party and standard library imports are ignored.
- A relative import that resolves to nothing is an `unresolved-import` warning, never silently
  dropped, whatever extension the specifier has (`./missing.json` included).
- Files with no owner never produce edge findings (no cascade from `unmapped-file`).
- Resolution per language: TypeScript and JavaScript are described under "TypeScript and JavaScript
  resolution" below. Python handles relative imports, `__init__.py`, and absolute imports rooted at
  the repo root or at `src/`. Rust handles `use crate::`, `use super::`, and `mod` declarations are
  containment, not edges.
- Extraction is tree-sitter based and driven by a per-language table (extensions, grammar, query),
  not a trait hierarchy. Adding a language means adding a table entry and a query.

### TypeScript and JavaScript resolution

A specifier is resolved in this order, the first step that applies deciding.

1. **Suffix.** A bundler query or fragment is not part of the file name: everything from the first
   `?`, or from the first `#` that is not the first character, is removed before resolution
   (`./a.svg?raw`, `./w.ts?worker`, `./a.css?inline`, `./x.png?url&v=2`, `./a#frag`). Messages
   (`unresolved-import`, skipped references) keep the specifier as written. A specifier that starts
   with `#` is a package `imports` key.
2. **Relative** (`.`, `..`, `./x`, `../x`): the path is joined onto the directory of the importer and
   probed, see "Probing". A relative specifier that probing does not satisfy is an
   `unresolved-import`, whatever its extension.
3. **Aliases.** The `paths` and `baseUrl` of the governing config (see "Governing config") yield
   candidate paths that are probed like a relative path.
4. **Workspace packages**, see below.
5. **Package `imports`**, for a specifier that starts with `#`, see below.
6. **Package check.** A Node builtin or a declared dependency is ignored, anything else is skipped as
   `undeclared-package`.

The first of steps 3 to 5 that finds a file wins. A `paths` pattern that matches the specifier and
finds no file is an alias: it ends the resolution silently unless a workspace package or `imports` key
resolves the specifier.

#### Probing

A candidate path (a relative path, an alias target, a package entry) is looked up against the files
the scan knows. The first that exists wins, in this order:

1. the path itself as a source file, then the path itself as an asset (see "Asset files"). A candidate
   written with a trailing `/` is a directory and skips this step and the next two;
2. the path with an extension appended, in this order: `ts`, `tsx`, `d.ts`, `mts`, `cts`, `d.mts`,
   `d.cts`, `js`, `jsx`, `mjs`, `cjs`. The extension is appended, so `user.service` finds
   `user.service.ts`;
3. when the path is written with a JavaScript extension, the TypeScript files that compile to it:
   `.js` finds `.ts`, `.tsx`, `.d.ts`; `.jsx` finds `.tsx`; `.mjs` finds `.mts`, `.d.mts`; `.cjs` finds
   `.cts`, `.d.cts`;
4. `index.<ext>` in that directory, with the extensions of step 2.

A declaration file (`decl.d.ts`) is a source file like any other, so
`import type { D } from '../types/decl'` resolves to it. A probe never adds an asset extension: `./data`
does not find `data.json`.

An import whose probe ends on an asset is a resolved import of kind `asset` that uses no symbol. This
holds for relative specifiers, alias targets and package entries alike.

#### Governing config

A file is governed by the `tsconfig.json` (else `jsconfig.json`) in its own directory or, failing that,
in the nearest ancestor directory. A solution-style config (`"files": []` and non-empty
`references`) that defines neither `paths` nor `baseUrl` after `extends` has no aliases of its own.

When the nearest config is solution-style, its `references` are candidates, in the order written. Each
`path` is a directory (its `tsconfig.json`, else `jsconfig.json`) or a config file, relative to the
solution config. A reference that is missing, malformed, or leaves the repository is skipped, and a
referenced config that is itself solution-style is not followed further. The file is governed by the
first referenced config that includes it, and that config's `paths` and `baseUrl` (after its own
`extends`) apply. When no reference includes the file, the solution config is as if it were absent: the
nearest config above it applies, as it did before references were read.

A referenced config includes a file when

- the file is listed in its `files` (a path relative to the config), or
- the file matches a pattern of `include` and no pattern of `exclude`. A config with neither `files`
  nor `include` has the `include` `**/*`, every file below its directory.

`include`, `exclude` and `files` are read from the config or, when it does not set one, from the
nearest config of its `extends` chain that does, relative to the directory of the file that sets it.
Patterns are relative to that directory: `*` matches any characters inside one path segment, `?` one
character, `**` any number of directories (also none). A pattern also matches everything below it, so
`src` includes `src/a/b.ts`. A leading `./` is ignored and a pattern that leaves its directory matches
nothing. `exclude` does not apply to `files`. `compilerOptions` such as `rootDir` and the file
extensions TypeScript would compile do not decide inclusion.

#### Workspace packages

A `package.json` with `workspaces` (an array of patterns, or an object whose `packages` is such an
array) declares workspace members. Every such `package.json` counts, at the repository root or nested.
A pattern is relative to the directory of its `package.json`, uses `*`, `?` and `**` as above, and a
pattern that starts with `!` removes directories the earlier patterns matched. A pattern is matched
against the directories that hold, or lie above, a TypeScript or JavaScript source file, so ignored
trees are never consulted. A matched directory is a member when its `package.json` is readable and has
a non-empty string `name`. When two members share a name the first in path order wins. pnpm and lerna
workspace files are not read.

A bare specifier whose package name (`@x/b` for `@x/b/util`, `pkg` for `pkg/sub`) is a member
resolves into that member, before the package check and whether or not the importer lists it as a
dependency. Resolution takes the subpath of the specifier, `.` for none and `./util` for `@x/b/util`:

1. When the member has `exports`, the subpath is looked up in it. `exports` is a subpath map when it
   is an object with a key that starts with `.` (keys that do not are ignored); any other value (a
   string, an array, an object of conditions) is the target of `.`. An exact key wins; otherwise among
   the keys with one `*` that match, the longest prefix wins, then the longest key. The `*` stands for
   the matched text, replaced at every `*` of the target. A target is a string starting with `./`, a
   conditions object, or an array of targets tried in order; `null` and any other value yield nothing.
   A conditions object is searched in the fixed order `types`, `import`, `default`, `require` (never
   in the order the keys are written), other conditions are ignored, and the search descends into
   nested objects with the same order. Every target the search yields, in that order, is probed
   against the member directory and the first that names a file wins. So a `types` target that is not
   in the repository (a build output) falls through to the next condition.
2. When the subpath is `.` and `exports` found no file, the fields `module`, `main` and `types` are
   probed in that order against the member directory, then the directory itself (its `index.*`).
3. When the subpath is longer and `exports` found no file, the rest of the specifier (`util`) is
   probed against the member directory like a relative path.
4. Otherwise the specifier is ignored: the package is a repository member whose entry is not a known
   file (a build output), which is not an error.

#### Package `imports`

A specifier that starts with `#` is looked up in the `imports` field of the nearest `package.json` at
or above the importer that has an `imports` object. Its keys start with `#`; the matching rules, the
`*` patterns, conditions and the probing of targets are those of `exports` above, and targets are
relative to the directory of that `package.json`. A target that does not start with `./` names a
package, not a repository file, and yields nothing. A key that matches and finds no file ignores the
specifier. A specifier that no key matches, or one with no `imports` field to look in, goes on to the
package check.

#### Packages

A declared dependency is a name listed in `dependencies`, `devDependencies`, `peerDependencies` or
`optionalDependencies` of any `package.json` at or above the file, up to the repository root, because
Node searches every ancestor `node_modules`. A name declared in any one of them is declared.

### Import location

An import's line is the line where its statement starts, not the line of the specifier string: the
`import` or `export` keyword of a TypeScript or JavaScript statement, the first token of a
`require(..)` or `import(..)` call expression, the `import` or `from` keyword of a Python statement,
and the `use` declaration of Rust (its `pub` when it has one, never the attributes above it). A
statement that spans several lines is one location, its first line. A Rust qualified path written
in code (`crate::gitlab::fetch(..)`, or a path inside a macro argument) is not a statement and keeps
the line of the path itself. A skipped Python `importlib.import_module(..)` call is located at the
call start, and a Rust embed at the start of its macro invocation.

### Import kind

Every resolved import carries a `kind` (`Runtime`, `TypeOnly`, `Embed` or `Asset`, serialized as the
kebab-case strings `runtime`, `type-only`, `embed`, `asset`) and a `test_only` flag. Neither changes which
edges exist: the linter treats every kind and test import like any other. They are recorded so that
callers can decide what a type-only or test import is worth.

`type-only`:

- TypeScript and JavaScript: `import type ...`, `export type ... from`, an import or re-export where
  every specifier is written `type X` (`import { type A, type B } from`), and every import in a
  declaration file (`.d.ts`, `.d.mts`, `.d.cts`). `import { type A, B }` is `runtime`, as is a
  default or namespace import beside type specifiers, `import {} from`, and `require(..)` or
  `import(..)`.
- Python: an import anywhere inside the body of `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:`
  (any nesting depth below it). Its `else` and `elif` branches and any other condition are `runtime`.

`test_only`:

- Rust: an import written inside an item or module annotated `#[cfg(test)]`, at any nesting depth
  (`#[cfg(test)] mod tests { fn f() { use crate::a::B; } }`), including a `use` that is itself
  annotated, and inside a module whose body starts with the inner attribute `#![cfg(test)]`. Only
  the exact predicate `test` counts: `cfg(any(test, ..))`, `cfg(all(test, ..))` and `cfg(not(test))`
  do not. An import in a file under a `tests/` directory or named `*_test.rs` or `*_tests.rs` is
  `test_only` whatever it is written in. A file that a `#[cfg(test)] mod tests;` declaration pulls in
  is not followed: its own name has to say it is a test file.
- TypeScript and JavaScript: files named `*.test.*` or `*.spec.*` (a middle name segment equal to
  `test` or `spec`, so `a.test.ts`, `a.spec.tsx`), or under a `__tests__`, `test` or `tests`
  directory at any depth.
- Python: files named `test_*.py`, `*_test.py` or `conftest.py`, or under a `tests` or `test`
  directory at any depth.

Duplicates merge on (location, target) only when their kind and `test_only` agree. Imports that
reach the same file from the same location with different kinds (`import type { A } from './a';
import { b } from './a';` on one line) stay two entries, so a type-only use is never recorded as
runtime or the other way round. Entries are sorted by location, target, symbols, kind, `test_only`.

`asset`: a TypeScript or JavaScript import that resolves to an existing non-source repository file
(`./data.json`, `./style.css`, `../x.svg`, see "Asset files"). It uses no symbol, so it is never
`private-symbol`, it is a dependency on the node that owns the asset, and it is `asset` even inside a
declaration file. As for every import, an asset owned by nobody or by the importer's own node is no
edge.

### Embeds

A Rust `include_str!("..")`, `include_bytes!("..")` or `include!("..")` (also written `std::..`) with
a plain string literal embeds a file. The path is relative to the directory of the file that holds
the macro. When it is a known repository source file, the embed is a resolved import of kind
`embed` that uses no symbol: an embed is not a use of named items, so it is never `private-symbol`,
and surfaces do not restrict embeds. It still counts as a dependency on that file's node, like any
import. When the target is not a known source file (`.html`, `.css`, `.png`, a missing file, a path
that leaves the repository), it is a skipped reference with reason `embed-non-source`.

### Skipped references

A reference driftwall cannot follow is never silently dropped. `FileImports.skipped` lists each as
a location, the specifier as written and a typed reason (kebab-case in serialization):

| Reason | When |
| --- | --- |
| `undeclared-package` | TypeScript or JavaScript specifier that is not relative, matches no `tsconfig` or `jsconfig` `paths` alias, is not a workspace package, is not a Node builtin and is not a declared dependency |
| `dynamic-specifier` | `import(expr)` or `require(expr)` whose argument is not a string literal or a template without substitutions; Python `importlib.import_module(..)` and `__import__(..)` |
| `embed-non-source` | a Rust embed whose target is not a known source file |

For `undeclared-package`, a specifier that a `paths` pattern matches is an alias and never skipped,
even when no file backs it (that stays silent, as before). `baseUrl` alone is not an alias: a bare
specifier it cannot resolve falls through to the package check. A builtin is a `node:` specifier or
one whose first path segment is a Node builtin module name (`fs`, `path`, `fs/promises`, ..). A
declared dependency is a name listed in `dependencies`, `devDependencies`, `peerDependencies` or
`optionalDependencies` of any `package.json` at or above the file (see "Packages"); `@a/b/sub` is the
package `@a/b` and `pkg/sub` is `pkg`. A file with no readable `package.json` above it declares nothing.
A malformed `package.json` is skipped silently, as a malformed `tsconfig` is, and the others still
count. Declared third party packages stay silently
ignored: they are not repository imports and nothing is wrong. `skipped` is not a lint finding.
For a Python call the specifier is the text of its first argument.

## Symbols and public surfaces

Every resolved import records the symbols it uses, exactly as written in the code, never inferred:

- Rust: names in `use` trees (`use crate::gitlab::{Client, fetch}` uses `Client`, `fetch`), the first
  item segment of a qualified path (`crate::gitlab::fetch_mrs(..)` uses `fetch_mrs`), and paths that
  start with a name the same file brought into scope with `use` or declared with `mod`
  (`use crate::gitlab;` then `gitlab::Client` uses `Client`). A glob (`use crate::gitlab::*;`) uses
  `*`. A module binding (`use crate::gitlab;`, `use crate::gitlab as gl;` or `self` in a group,
  `use crate::gitlab::{self, Client}`) uses no symbol on its own line: it is a dependency on the module file,
  and the items reached through its name are recorded at their own sites.
- TypeScript and JavaScript: named imports and re-exports by their exported name, `default` for a
  default import, `*` for namespace imports, `require`, dynamic `import()` and `export *`.
- Python: `from a import b, c` uses `b`, `c` when they resolve to names inside `a`. `import a.b` and
  a name that resolves to a submodule use `*`.
- A side-effect import (`import './x'`) uses no symbol.

Method calls on values (`client.fetch()`) are not resolved: knowing which `fetch` needs type
information. driftwall reports only what the syntax proves.

A node may declare its public surface in the contract with a directive comment, which Mermaid
renderers ignore:

```
%% exports gitlab: Client, fetch_mrs, post_note
```

At most one `exports` line per node, naming a declared node (checked after the whole file is read,
like edge endpoints). The list is one or more comma separated identifiers (`[A-Za-z_$]` then
letters, digits, `_` or `$`), each at most once. A comment whose first word is `exports` is always
this directive, so a malformed one (no `:`, empty list, bad or repeated name) is a hard error with
its line, never a silent comment; every other `%%` line stays a plain comment. The renderer writes
the lines after the edges, sorted by node, names sorted.

When a node has a surface, an import from another node (both owners declared, so unmapped and unknown
owners never cascade) that uses a symbol outside it, or uses `*`, is a `private-symbol` error at the
import, one per symbol. An import that uses no symbol (side-effect) is never private, and imports
inside one node are never checked. Nodes without an `exports` line are unrestricted.

`*` is always a violation for a node with a surface: the importer reaches names that are never seen,
so it cannot be checked. A Rust glob (`use crate::gitlab::*;`), TypeScript `import * as` and
`export *`, and Python `import a.b` all record `*`. A Rust module binding records nothing, so
`use crate::gitlab;` is not private by itself, and each `gitlab::item` used through it is checked
against the surface at its own line, one per item.

`diff` reports surface changes as `+ exports gitlab: X` and `- exports gitlab: Y`, one line per
name; a node that loses its whole surface shows only `-` lines and is unrestricted from then on.
`brief` and `neighbors` list the symbols each edge uses (`uses: Client, fetch`, the first six then
`+N more`) and mark a declared surface (`exports: ...`); JSON carries `symbols` on every edge and
`exports` on a node that has a surface.

## Rules (violation kinds)

| Kind | Severity | When |
| --- | --- | --- |
| `unmapped-file` | error | source file has no inherited `.arch` tag |
| `unknown-node` | error | `.arch` names a node the diagram does not declare |
| `undeclared-edge` | error | code imports across nodes and the diagram has no such edge |
| `stale-edge` | warning | solid edge with no import backing it |
| `planned-realized` | warning | dotted edge now backed by code, promote it to solid |
| `unresolved-import` | warning | relative import resolves to nothing, whatever its extension |
| `private-symbol` | error | an import uses a symbol outside the target node's declared surface |

A dotted edge permits the import. `--strict` turns every warning into a failure.

## init

`driftwall init` reverse-engineers a baseline from an existing repository so that `driftwall lint`
on the result is clean at once (with the same `--contract`). It writes `architecture.mmd` and the
`.arch` tags (a contract without tags would leave every file unmapped); `--no-tags` writes the
contract only.

Source files are discovered like `lint` does: ignores are honored, vendored and minified files are
excluded (see "Excluded files"), and no source files is an error. `init` decides, in this order,
which node every source file belongs to. A file is decided by the first rule that covers it.

1. **Existing tags.** Every file that already has an owner through an `.arch` file keeps it, with
   or without `--no-tags`, with or without `--force`. These are fixed decisions: `init` plans only
   for the files that have no owner, never rewrites or removes an `.arch` file it did not just
   create, and declares in the contract every node the existing tags name (also a tag whose
   directory holds no source). A hand written `src/.arch` with `node: *` therefore steers `init`.
   An existing `.arch` below a directory `init` tags is fine: the nearer tag wins, as everywhere.
   `--force` only lets `init` overwrite files it writes itself: the contract, and an `.arch` at a
   path it plans for that exists but that the scan did not see (a gitignored one).
2. **Code units** (only without `--depth`). Directories that a manifest or the layout names as one
   buildable or importable unit become nodes, see below.
3. **The automatic cut** (only without `--depth`) for everything that is left.
4. **`--depth N`** replaces rules 2 and 3: every file left belongs to its ancestor directory at
   depth N from the repo root, or to its own directory when that is shallower, one directory node
   each. Module nodes and units never appear under an explicit `--depth`, so both layouts stay
   reachable without a flag.

### Code units

A unit is a directory. Units are found from manifests and the file layout:

- **Rust crate**: a directory whose `Cargo.toml` has a `[package]` and that holds a `src/`
  directory; and every directory a `[workspace] members` list of any `Cargo.toml` names that holds
  a `Cargo.toml` (members are relative to that manifest, `exclude` removes directories). A member
  entry may use `*` and `?` inside a path segment (`crates/*`, `crates/app-?`), expanded against
  the directories that exist. Other glob syntax (`**`, `[a-z]`) and entries that leave the
  workspace (`..`) match nothing.
- **JavaScript package**: every directory a `package.json` `workspaces` list (an array, or an
  object with `packages`) names that holds a `package.json`; `*` and `?` as above, and a `!`
  entry removes directories. The workspace root itself is not a unit. pnpm and lerna workspace
  files are not read.
- **Python package**: a directory with an `__init__.py` that has no ancestor directory with an
  `__init__.py` (the outermost package; sub-packages belong to it).

Manifests are read only in directories that hold, or lie above, a source file, so ignored trees are
never consulted. A manifest that cannot be read or parsed is skipped, like a broken `Cargo.toml` is
for import resolution. A unit with no unowned source file is dropped. A directory found by two
rules is one unit.

Each kind is decided on its own:

- **Two or more units of a kind**: each is one node covering its whole directory (everything
  below it that no nearer tag or unit takes), so a workspace of crates `core`, `cli`, `wasm` is
  three nodes with the edges between them. A crate that contains another crate unit covers only its
  `src/` directory (a root package next to its `members`).
- **Exactly one Rust crate**: the crate is decomposed. When its `src/` directly holds two or more
  modules (distinct derived names, `a.ts` and `a.test.ts` are one), `src/` gets `node: *`, so
  every module and subdirectory of it is a node named by the module rule. The rest of the
  repository (`scripts/`, `tests/`, `static/`, ...) is cut by rule 3. Without two modules, or when
  a derived id is not a valid node id or is already taken (see "Ids"), nothing is decided here
  and rule 3 cuts the crate like any directory.
- **Exactly one JavaScript or Python unit**: nothing is decided here, rule 3 sees the files.

### The automatic cut

- Starting at the root, descend while the directory holds no source file itself and exactly one
  child directory holds sources. The directory where that stops (own source files, or two or more
  source-bearing children) is the fork; N is its depth plus one. So code under `src/` alone yields
  `src/auth` and `src/billing`, not one `src` node, while a repo with one root module plus `lib/`
  yields `root` and `lib`, and one directory holding one module yields a single node.
- When the fork directly holds two or more modules, it gets `node: *` and every module and
  subdirectory is a node named by the module rule, so contract ids are exactly the ids the linter
  derives, labelled `<fork>/<id>`. A fork holding one module keeps directory nodes. When any file
  under the fork would derive an id the contract cannot declare (`_private.py`, `.eslintrc.js`,
  `end.rs`, `__tests__/`) or an id that is already taken, `init` falls back to directory nodes
  instead of writing a baseline that cannot lint.
- **Root files.** The repository root is never a `node: *` source root when it also has source
  directories: its own files (`vite.config.ts`, `eslint.config.js`, `build.rs`, `setup.py`) are
  tooling next to the code, and become one node `root` beside one node per source directory. A
  root that holds only files (flat scripts) still gets `node: *`. A project that wants root modules
  writes a root `.arch` with `node: *` itself (rule 1).

### Ids

- Directory node ids (units, directory cuts, the root node) come from the directory path below the
  longest prefix all new directory nodes share, segments joined with `-`, other characters replaced
  by `-` (repeats collapsed, ends trimmed, `n-` prepended to a non-letter start). A directory that
  is the shared prefix itself is named by its last segment, and the root directory is `root`. So
  `engine/core` and `engine/cli` next to `src` are `engine-core`, `engine-cli` and `src`.
- Module ids (`node: *`) are fixed by the module rule and are never renamed. Node ids named by
  existing tags are fixed too. Two existing tags may name the same node on purpose (that is
  grouping); `init` itself never merges two units silently.
- A directory node whose id is a fixed one (module, or existing tag) is named `<id>-dir`, then
  `<id>-dir-2`, `<id>-dir-3`, so a module `src/ui.rs` (node `ui`) and a directory `ui/` give `ui` and
  `ui-dir`. Any other collision between directory nodes, and the reserved words `end`, `flowchart`
  and `subgraph`, get `-2`, `-3`, ... in sorted directory order.
- A `node: *` region (rule 2 or 3) whose derived ids are not valid, or collide with a fixed id or
  with an earlier region's ids, is not created; its files get directory nodes instead.

### Contract and writing

- Every resolved import between files with different owners becomes one solid edge. Nothing is
  dotted, so a fresh baseline has no stale or planned-realized findings.
- `init` refuses (exit `2`) when the contract or an `.arch` it would write exists, listing every
  such path, and checks all of them before writing anything. `--force` overwrites. Running `init`
  again therefore needs `--force` only for the contract, and rewrites it to the same contents,
  because every file is then owned by an existing tag.

## Exit codes

`0` clean. `1` violations. `2` tool error (bad diagram syntax, IO failure, bad arguments).
A failing lint is not an error, so `anyhow` is used only at the entry point and always maps to `2`.

## CLI surface

```
driftwall lint [PATH] [--format text|json|sarif] [--strict]
driftwall init [PATH] [--depth N] [--contract FILE] [--no-tags] [--force]
driftwall where <FILE>          # owning node of a path, with the .arch file that decided it
driftwall brief [PATH] [--contract FILE] [--format text|json]
driftwall neighbors <NODE> [PATH] [--contract FILE] [--format text|json]
driftwall diff --base <REF> [PATH] [--contract FILE] [--format text|json] [--exit-code]
```

`--format json` is the agent interface. Every violation carries `kind`, `severity`, `at`
(`file`, `line`), `message`, `hint`.

`brief`, `neighbors` and `diff` are read-only maps for agents and reviewers:

- `brief` prints every node (directories, file count) and every edge, declared or observed, with
  the number of import sites that realise it. An edge code uses and the contract lacks is marked
  `UNDECLARED`. It is informational: exit `0` unless the tool fails (`2`).
- `neighbors <NODE>` prints one node's incoming and outgoing edges with up to 3 `file:line`
  example sites each. An unknown node is a tool error that lists the declared ids.
- `diff --base <REF>` compares the contract at a git ref with the working tree (nodes and edge
  kinds; labels and boundaries are ignored). The old text comes from `git show REF:./<contract>`;
  a contract absent at that ref counts as empty, any other git failure is exit `2`. Exit `0`
  normally; with `--exit-code`, `1` when anything changed, so CI can require approval.

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
