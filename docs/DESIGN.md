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

A `.arch` file contains `node: <id>` and, optionally, `scope: here` (comments start with `#`; each
directive at most once, `node:` is required). It tags its directory and all subdirectories, until a
nested `.arch` overrides it. Source files are found by walking the repo with the `ignore` crate, so
`.gitignore` is honored. `.driftwallignore` (gitignore syntax) adds exclusions. Only files with a
supported extension count as source. One exception: an HTML file
(`.html`, `.htm`) counts as source only when it holds a `<script` element, see "Svelte and HTML";
any other HTML file is an asset, so documentation and fixture pages never become unmapped files.
A file with no extension counts as source when its first line is a shebang that names a supported
interpreter, see "Source languages by file".

`scope: here` limits the tag to the files directly in its directory: it is not inherited by any
subdirectory, so a source file added below it finds the next `.arch` further up, or none and is
`unmapped-file`. Without `scope:` the tag covers the whole subtree, as above. `here` is the only
accepted value, and a `scope:` line without a `node:` line is the same `missing node` error as an
empty file. The scope is a typed field of the tag (`Scope::Subtree`, the default, or `Scope::Here`).
`where` and `scan` answer through the same rule: a `scope: here` file is skipped for any file that is
not directly in its directory, and the search continues in the directories above it.

### Source languages by file

The table of file names to languages is data in `src/ownership/language.rs`. Matching is
case-sensitive.

| Language | Files |
| --- | --- |
| TypeScript | `.ts`, `.tsx`, `.mts`, `.cts` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py`, and extensionless files whose shebang names Python |
| Rust | `.rs` |
| Svelte | `.svelte` |
| Html | `.html`, `.htm`, only with a `<script` element |
| Swift | `.swift` |
| Shell | `.sh`, `.bash`, `.zsh`, and extensionless files whose shebang names a shell |

**Extensionless scripts.** Only a file whose name has no extension (`deploy`, `pre-commit`, and a
hidden name such as `.profile`: no `.` followed by text) is sniffed, so `deploy.old` or `run.txt`
never is. The first line must start with `#!` (no space, no byte order mark before it; later lines
are never read, and at most the first 256 bytes of the line). After `#!`, spaces are skipped and the
interpreter is the first word, and the name is the last `/` segment of that word. When the name is
`env`, the interpreter is instead the first following word that does not start with `-` and holds
no `=` (`#!/usr/bin/env -S python3 -u` is `python3`), and again its last segment is the name. Then:

- `sh`, `bash`, `zsh` or `dash` makes the file Shell;
- `python`, or `python` followed only by digits and dots (`python3`, `python3.12`), makes it Python;
- anything else (`node`, `perl`, `fish`, `ksh`, a missing interpreter) does not: the file is an
  asset.

A file whose first line is not UTF-8, or that cannot be read, is an asset, so sniffing never fails a
scan. A sniffed script is source like any other: it is owned or unmapped, excluded under `vendor/`,
and an import target. An extensionless Python script is never imported by Python code: only `.py`
files are Python import targets.

### Excluded files

Minified, vendored and generated code is not the project's own source, so the scan leaves it out by
default. A source file is excluded when

- it is below a directory named `vendor` or `third_party` (at any depth), reason `vendored`; or
- its name has the form `<name>.min.<ext>` (the second to last dot-separated part is exactly
  `min`: `htmx.min.js`, `app.min.mjs`, `site.min.css`), reason `minified`; or
- it is below a directory named `dist` (at any depth), reason `generated`; or
- one of its first 5 lines contains, ignoring case, `@generated`, `do not edit`, `auto-generated` or
  `autogenerated`, reason `generated`. Only the first 5 lines are read, and a line that is not UTF-8
  is read lossily.

A file that matches several gets the first of `vendored`, `minified`, `generated`. Excluded files are
never parsed, never in `Ownership::owners` and never `unmapped-file`: their own imports are not read.
They are not silently dropped: `Ownership::excluded` maps each reason to the sorted set of excluded
paths, so a report can show them. `.arch` files are never excluded.

**Excluded files are import targets.** Leaving a file out of the scan does not make it disappear
from the repository, and an import of it is real code that depends on it. An excluded file is
probed exactly like a source file (see "Probing"), so `./font-metrics.mjs` finds the excluded
`font-metrics.mjs` before it would find `font-metrics.d.mts`, and `./g.mjs` is no
`unresolved-import` when `g.mjs` exists and was excluded. Such an import is a resolved import of kind
`runtime` unless the import says otherwise (see "Import kind"), it records its symbols like any
other, and it is a dependency on the node that owns the excluded file. The owner is decided by the
tag rules of source files, as for assets: `Ownership::excluded_owners` maps each owned excluded file
to its node, an excluded file that no tag covers (or whose derived name under `node: *` is not a node
id) is in no map and is a target with no owner, which makes no edge. When the owner is not a node
the contract declares, the import is recorded as an `undeclared-owner` skipped reference, as for an
asset (see "Asset files"), with the message ``excluded file `<path>` is owned by `<node>`, which the
contract does not declare``. Excluded files stay out of `owners`, `unmapped`, every file count and
every module-node derivation: they only resolve and have an owner.

A project opts files back in with a negated pattern in the `.driftwallignore` at the repository
root (`!vendor/mine/`, `!static/app.min.js`, `!dist/`, `!src/api.gen.ts`): a file whose path that file whitelists is source
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

An import of an asset whose owner is not a node the contract declares (a tag names an undeclared
node, or `node: *` derives one the contract lacks, so no edge can exist) is not silent. When the
importing file is owned by a declared node, `lint` records it as a skipped reference with reason
`undeclared-owner` (see "Skipped references"). Its specifier is the repository path of the asset,
since the import as written is not kept, and its message is ``asset `<path>` is owned by `<node>`,
which the contract does not declare``. It is attributed to the node of the importing file, shows in
`brief` and `neighbors`, and is no lint finding. An import of a source file keeps its behaviour:
a target owned by an undeclared node is no edge and no record, as `unknown-node` already reports
the tag. An asset that no tag covers, and an importer that no declared node owns, produce no record.

### Blind spots

What driftwall cannot see must never be silent. Beside the source files it reads, the scan keeps a
census of every file it leaves unread, and the import collection keeps every reference it cannot
follow. The census is data, never a finding: it changes no owner, no `unmapped-file`, no edge and no
exit code. It lives on `Ownership` next to `excluded`:

- **Not read**, `Ownership::unread`: a map from a key to the sorted set of walked files of that key.
  The key is the file extension (no dot, case-sensitive) when it is in the fixed table of common code
  extensions `sh bash zsh swift c h cc cpp hpp m mm go java kt kts rb php cs lua dart scala ex exs
  erl hs ml clj vue astro pl r sql ps1 cmd bat mc zig nim svelte html htm`, minus every extension the language
  table supports (so adding a language removes its extensions from `unread` by itself: `ts` is never
  here). A file with no extension at all whose first two bytes are `#!` has the key `shebang`. The
  files are the walked ones: ignored files (`.gitignore`, `.driftwallignore`) are not walked and are
  not counted here. An unread file is also an asset and resolves as one, as before.
- **Ignored**, `Ownership::ignored`: the files with a source extension that only `.driftwallignore`
  hides. The scan finds them by walking a second time with the `.driftwallignore` files disabled
  (`.gitignore` stays on: a file git ignores is not part of the repository, so it is never counted),
  and keeping the files of the second walk that the first did not see. The second walk only happens
  when the first one met a `.driftwallignore`. A hidden file in a language without support is not
  counted: nothing would have read it anyway.
- **Excluded**, `Ownership::excluded`, as described above.
- **Skipped references** and **unresolved imports**, the `skipped` and `unresolved` lists of
  `FileImports` (see "Skipped references").

Every census file is attributed to the node that owns its path by the tag rules of source files (the
nearest `.arch`, `scope: here` and `node: *` included, see `owner_in`), or to no node when no tag
covers it, which reports call `unmapped`. A reference is attributed to the node that owns the file it
is written in. Unread, excluded and ignored files never produce `unmapped-file`.

`lint::observe` builds the whole picture as one `BlindSpots` value (`Observation::blind_spots`): each
unread file with its owner and its number of lines (newline bytes, plus one for a last line without a
newline; a file that cannot be read has 0), each excluded and ignored file with its owner, each
skipped reference with its reason and each unresolved import with its owner. All of it is sorted, and
`init` builds the same value without owners and lines, and without the `undeclared-owner` records
only `lint` can make (they need a contract). The two notes therefore agree on a fresh baseline, because
`init` declares every owner it gives an asset (see "Contract and writing"), and differ only by the
`undeclared-owner` references a hand-edited contract adds.

Reports: see "CLI surface". The note line that `lint` and `init` print has the form

```
note: 50 files not read (swift 40, sh 10), 3 skipped references; see driftwall brief
```

Its parts, each present only when non-zero and in this order, are `N files not read (ext n, ..)`
(extensions by file count, descending, then by name, at most 6 then `+N more`), `N files excluded
(reason n, ..)`, `N files ignored by .driftwallignore`, `N skipped references` and `N unresolved
imports`, joined by `, `, then `; see driftwall brief`. With nothing to say there is no note line.

### Module nodes (`node: *`)

A `.arch` file may contain `node: *` instead of an id. Then every module directly in that
directory is its own node, named by a fixed rule, with no further tagging:

- A source file is a module. Its node id is the file name up to the first `.`, so `gitlab.rs` is
  `gitlab` and `gitlab.test.ts` joins `gitlab`.
- **A test file never creates a node of its own.** A file is a test file when its name has a part
  `test` or `spec` between two dots (`a.test.ts`, `a.b.spec.d.ts`, `snap.test.json`), or is
  `test_<x>.py`, `<x>_test.py`, `<x>_test.rs` or `<x>_tests.rs` (`<x>` not empty). Its name is
  derived as above (`rows.test.ts` is `rows`, `test_rows.py` is `test_rows`), but it keeps that
  name only when a non-test module directly in the same directory has it: a source file that is
  not a test file and derives it, or a subdirectory of that name that holds source files at any
  depth. Otherwise it joins the node of the directory itself, the node an `index.*` file there
  gets (`rows.test.ts` next to no `rows.*` module, in `src/`, is the node `src`; in the repository
  root it is `root`). So `rows.test.ts` beside `rows.ts` is `rows`, and every orphan test of a
  directory is one and the same node. The rule applies to assets too, and to files directly in
  the directory only: a file in a subdirectory is named by the subdirectory, as above. The
  non-test modules of a directory are fixed by the files the scan sees, so `lint`, `where` and
  `init` agree.
- Entry files (`mod.rs`, `lib.rs`, `main.rs`, `index.*`, `__init__.py`, `__main__.py`) are named by
  their file stem too (`main`, `lib`), except `mod.rs`, `index.*`, `__init__.py` and `__main__.py`,
  which are named after their directory: `__main__.py` joins its directory's node like
  `__init__.py` does.
- A subdirectory whose name equals a sibling module's id (Rust `gitlab.rs` plus `gitlab/`) belongs to
  that module's node. Any other subdirectory is one node named after the directory.
- Leading underscores are stripped from every derived name, file or directory: `_private.py` is
  `private`, `__tests__/` is `tests__`, `_x.py` and `x.py` share the node `x`. A name that is empty
  after stripping (`_.py`, `__/`, a hidden file such as `.eslintrc.js`) is invalid.
- A nested `.arch` still overrides, as everywhere else.
- A derived id that is not a valid node id is an error at the `.arch` file, never silently renamed.

`node: <prefix>*` derives the same modules but gives each id the prefix: the node of `components/`
under `node: ui-*` is `ui-components`. A prefix matches `[A-Za-z][A-Za-z0-9_-]*-` (it ends in `-`);
anything else before the `*` is an invalid id error at the `.arch` file. The prefix is a typed part
of the tag (`Tag::Modules(Prefix)`; a plain `*` has the empty prefix). The derived id is the prefix
followed by the module name, and it must be a valid node id with a non-empty module name, so `ui-`
plus `9lives` is valid but `ui-` plus the empty name is not.

`init` writes `node: *` into a directory that directly holds two or more modules (see `init`). An
entry file directly in the repository root (`index.ts`, `__init__.py`) is named `root`, since the
root directory has no name of its own. `unknown-node` reports a derived node the contract does not
declare at the `node: *` line of its `.arch` file.

## Import semantics

- An import is an edge only when it resolves to a repository file owned by a different node. The
  file is a source file, an excluded file (see "Excluded files") or, for TypeScript and JavaScript,
  an asset (see "Asset files").
- Imports inside one node are ignored. Third party and standard library imports are ignored.
- A relative import that resolves to nothing is an `unresolved-import` warning, never silently
  dropped, whatever extension the specifier has (`./missing.json` included). So is a Python absolute
  import into a first-party package that holds no such module, see "Python resolution".
- Files with no owner never produce edge findings (no cascade from `unmapped-file`).
- Resolution per language: TypeScript and JavaScript are described under "TypeScript and JavaScript
  resolution" below, Svelte and HTML under "Svelte and HTML" (their scripts resolve exactly like
  TypeScript and JavaScript). Python handles relative imports, `__init__.py`, and absolute imports against
  an ordered list of import roots, see "Python resolution" below. Rust handles `use crate::`, `use super::`, and `mod` declarations are
  containment, not edges.
  Swift imports resolve through the targets of `Package.swift` (see "Swift resolution") and shell
  `source` commands through literal and script-relative paths (see "Shell resolution").
- Extraction is tree-sitter based and driven by a per-language table (extensions, grammar, query),
  not a trait hierarchy. Adding a language means adding a table entry and a query.

### TypeScript and JavaScript resolution

A specifier is resolved in this order, the first step that applies deciding.

1. **Suffix.** A bundler query or fragment is not part of the file name: everything from the first
   `?`, or from the first `#` that is not the first character, is removed before resolution
   (`./a.svg?raw`, `./w.ts?worker`, `./a.css?inline`, `./x.png?url&v=2`, `./a#frag`); a `raw`, `url`
   or `inline` query decides the kind of the import, see "Import kind". Messages
   (`unresolved-import`, skipped references) keep the specifier as written. A specifier that starts
   with `#` is a package `imports` key.
2. **Relative** (`.`, `..`, `./x`, `../x`): the path is joined onto the directory of the importer and
   probed, see "Probing". A relative specifier that probing does not satisfy is an
   `unresolved-import`, whatever its extension.
3. **Aliases.** The `paths` and `baseUrl` of the governing config (see "Governing config") yield
   candidate paths that are probed like a relative path. Then the `resolve.alias` entries of the Vite
   config that governs the file (see "Vite aliases") are tried.
4. **Workspace packages**, see below.
5. **Package `imports`**, for a specifier that starts with `#`, see below.
6. **Package check.** A Node builtin or a declared dependency is ignored, anything else is skipped as
   `undeclared-package`.

The first of steps 3 to 5 that finds a file wins. A `paths` pattern that matches the specifier and
finds no file is an alias: it ends the resolution silently unless a Vite alias, a workspace package
or an `imports` key resolves the specifier.

#### Probing

A candidate path (a relative path, an alias target, a package entry) is looked up against the files
the scan knows. The first that exists wins, in this order:

1. the path itself as a source file (an excluded file counts as one, see "Excluded files"), then the
   path itself as an asset (see "Asset files"). A candidate
   written with a trailing `/` is a directory and skips this step and the next two;
2. the path with an extension appended, in this order: `ts`, `tsx`, `d.ts`, `mts`, `cts`, `d.mts`,
   `d.cts`, `js`, `jsx`, `mjs`, `cjs`, `svelte`. The extension is appended, so `user.service` finds
   `user.service.ts`. `svelte` comes last, so a specifier that resolves without it never changes
   meaning when a component of that name is added. Neither TypeScript nor Vite appends `.svelte`
   themselves (the Svelte tooling expects the extension written out), so this step is a leniency of
   driftwall and not a mirrored behaviour: `import X from './X'` finds `X.svelte` only when no
   TypeScript or JavaScript file of that name exists;
3. when the path is written with a JavaScript extension, the TypeScript files that compile to it:
   `.js` finds `.ts`, `.tsx`, `.d.ts`; `.jsx` finds `.tsx`; `.mjs` finds `.mts`, `.d.mts`; `.cjs` finds
   `.cts`, `.d.cts`;
4. `index.<ext>` in that directory, with the extensions of step 2.

Step 1 is why an existing file always wins over an appended extension: `../b/both.svelte` is the
component `both.svelte` even when `both.svelte.ts` (a Svelte 5 module) exists next to it, and
`../b/both` finds `both.ts` or another listed extension before `both.svelte`.

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
character, `**` any number of directories (also none). A pattern whose last segment has no `.`, `*`
or `?` names a directory and everything below it, so `src` includes `src/a/b.ts`; any other pattern
names files, so `src/*` includes `src/b.ts` and not `src/a/b.ts`. A leading `./` is ignored and a
pattern that leaves its directory matches nothing. `exclude` does not apply to `files`. `compilerOptions` such as `rootDir` and the file
extensions TypeScript would compile do not decide inclusion.

#### Vite aliases

A Vite config (`vite.config.js`, `.mjs`, `.ts`, `.cjs`, `.mts` or `.cts`, the first in that order when a
directory holds several, and only a config that is a known file) governs the files at or below its
directory. Its `resolve.alias` is read like the `root` option (see "Vite root"): the config is never run,
and the object is found by the same rules, only a `resolve` key directly in it counts and an
`alias` key directly in that. The alias is read when it is

- an object, `{ 'mrc-wasm': <path>, other: <path> }`, each key a plain string literal or a plain
  identifier, in the order written;
- an array of objects, `[{ find: 'x', replacement: <path> }]`, in the order written, `find` a plain
  string literal and `replacement` a path.

An entry whose key or `find` is anything else (a regular expression, a computed key, a spread) or
whose path is not recognised is ignored. A `<path>` is read with the forms the Vite root reader
understands: a plain string literal, relative to the config directory (`'./src'`), `__dirname` and
the other forms of the config directory, `path.resolve(__dirname, 'a', 'b')` and `path.join(..)`,
`fileURLToPath(new URL('./a', import.meta.url))`, also written `new URL(..).pathname`. A path that is
absolute or leaves the repository is not recognised.

A specifier matches an entry when it is the key or starts with the key and `/`. The first entry that
matches, nearest config first (a config in a directory nearer to the file than another one is tried
before it), decides; the rest of the specifier after the key is appended to the path. That target is
probed like a relative path (see "Probing"), so an alias to a directory finds its `index` file, and
the import resolves like any other: a source file, an excluded file or an asset. When it finds no
file:

- the alias target exists in the repository when some known file (source, asset or excluded) is at
  or below it. Then the specifier is an alias with no file behind it and ends the resolution
  silently, as a `paths` pattern does;
- otherwise the target does not exist (typically a build output that git ignores: `engine/wasm/pkg`
  for `'mrc-wasm'`) and the import is skipped as `alias-target-missing`, never as `undeclared-package`
  and never silently: its message names the alias and the target, ``alias `mrc-wasm` points to
  `engine/wasm/pkg`, which has no file in the repository``. In a report that knows the owners
  (`lint`, `brief`, `neighbors`) the message ends with ``; the target lies in the directory of node
  `engine-wasm` `` when a tag covers the directory the target is in (the nearest `.arch` above it).

The Vite aliases apply after `paths`, so a `tsconfig` alias that finds a file wins, and before the
workspace packages, the package `imports` and the package check.

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

#### JSDoc type imports

A block comment that starts with `/**` in a TypeScript, JavaScript or Svelte script may name a module
in a type: `@type {import('../../src/types').ImageManifest}`, `@returns {import('./x').T}`,
`@param {import("y")} p`. Each such `import(` is an import of kind `type-only`, resolved like the
specifier of an `import` statement (relative, aliases, workspace packages, package check, assets,
queries), so an unresolvable relative one is an `unresolved-import` and an undeclared package is
skipped as `undeclared-package`. It is read by this form only: the word `import` not preceded by an
identifier character or a `.`, optional whitespace, `(`, optional whitespace, a string in single or
double quotes with no escape, optional whitespace and `)`. When `.` and an identifier
follow the `)`, the import uses that member as its symbol (`ImageManifest`, `T`); with none it uses
no symbol. Its location is the line of the `import` word, inside the comment. A comment that starts
with `/*` but not `/**`, a `//` comment, a string and a `/**/` comment are not read. The comment
is found by the grammar, so a `/**` inside a string literal is not one. It is test code by the file
rules of "Import kind". A JSDoc `@import {T} from './x'` tag is not read.

### Python resolution

A module reference is the dotted module of `import a.b`, `import a.b as c`, `from a.b import c`,
`from .a import c` and the literal dynamic imports below. Spaces inside it are ignored.

**Relative** (leading dots): the module is joined onto the directory of the importer, climbed one
level for each dot after the first. A relative import that climbs above the repository root or finds
no file is an `unresolved-import`.

**Absolute** (no leading dot): the module is looked up under each import root of the importing file,
in order, and the first root that holds a file wins.

#### Import roots

The import roots of a file, in this order, with a later duplicate of an earlier root dropped:

1. **Own directory**, when the directory of the importing file holds no `__init__.py`. Python runs a
   script, and pytest in its default import mode runs a test module, with that directory first on
   `sys.path`, so sibling files import each other by bare name. A file in a package directory (one with
   an `__init__.py`) never gets this root: its siblings are reached through the package, or relatively.
2. **Configured roots**, see below.
3. **The repository root.**
4. **`src/`** at the repository root.

#### Configured roots

`collect` reads project configuration once. A configuration directory is the repository root and every
directory below it that holds a `pyproject.toml`, among the directories at or above a Python file. In
each, these files are read, in this order, and give roots in the order written:

| File | Setting | Root |
| --- | --- | --- |
| `pyproject.toml` | `[tool.pytest.ini_options] pythonpath` | each entry |
| `pyproject.toml` | `[tool.pyright] extraPaths`, then `[tool.basedpyright] extraPaths` | each entry |
| `pyproject.toml` | `[tool.setuptools] package-dir`, the entry whose key is `""` | its value |
| `pyproject.toml` | `[tool.setuptools.packages.find] where` | each entry |
| `pyproject.toml` | `[tool.hatch.build.targets.wheel] packages` | the parent directory of each entry (`src/x` gives `src`, `x` gives `.`) |
| `pyproject.toml` | `[tool.poetry] packages`, each table with an `include` | its `from` (`.` without one) |
| `setup.cfg` | `[options] package_dir`, the entry with an empty key (`= src`) | its value |
| `setup.cfg` | `[options.packages.find] where` | each entry |
| `setup.cfg` | `[tool:pytest] pythonpath` | each entry |
| `pytest.ini` | `[pytest] pythonpath` | each entry |
| `pyrightconfig.json` | `extraPaths` | each entry |

A root is relative to the directory of the configuration that names it, `.` being that directory
itself; it is normalised lexically. An entry that is absolute, leaves the repository, or is not a
string is dropped. A list setting is an array of strings in TOML and JSON, and in the INI files a list
separated by commas or whitespace (a lone string works everywhere). A `package-dir` entry for a named
package, other tables of `pyproject.toml` and every other file (`tox.ini`, a `Makefile`, an
environment variable such as `PYTHONPATH`) are not read. A file that cannot be read or parsed, or a
setting of the wrong type, is skipped silently like a malformed `Cargo.toml`, and the other files still
count. `pyrightconfig.json` is JSONC.

The roots a configuration directory gives apply to the files at or below it. For a file, the roots of
the nearest configuration directory come first, then those of each directory above it up to the
repository root. So a root-level `pythonpath = ["scripts", "."]` applies to the whole repository, and
a `lib` configured in `services/api/pyproject.toml` means `services/api/lib` for the files below
`services/api` only.

#### Probing

A module `m` with imported names `n` (none for `import m`, and for `from m import *`) resolves under
one root to the first of these files that exists, `<root>/` being omitted for the repository root:

1. for each name, in the order written, and independently: the submodule `<m>/<n>.py`, then the
   package `<m>/<n>/__init__.py`;
2. the module `<m>.py`, then the package `<m>/__init__.py`. With names this is the file the names
   are found in. `from . import x` has the package `__init__.py` as its module.

The first root with a hit decides, so a name that is a submodule under one root and a plain name
inside another module under an earlier root resolves under the earlier. An import of the importing
file itself is dropped.

#### Misses

An absolute module that no root resolves is decided by its first segment, the top-level name:

1. when some root holds a first-party module or regular package of that name (`<root>/<name>.py`, or
   a directory `<root>/<name>/` that holds an `__init__.py`) the import is an `unresolved-import`, as
   a relative miss is. Exception: the module itself exists as such a directory and the miss is only a
   name imported from it, which is ignored;
1a. else when some root holds a directory `<root>/<name>/` with no `__init__.py` (a namespace
   directory, below which any Python file lies) the import is skipped as `undeclared-package`, with a
   message naming the directory. A namespace directory is first-party only for the paths that exist
   below it (those resolve as usual): in Python an installed regular package of the same name beats
   a namespace portion, so a miss below it is most likely that package, as with any undeclared
   third party. The existing reason is reused so the reference stays visible, and the import is not
   a finding. Exception as in 1: the module itself exists as the directory;
2. else when some repository path matches the name, anywhere, the import is skipped as
   `unreachable-module`. Only a Python module or package matches: a file `<name>.py`, or a
   directory `<name>/` that holds an `__init__.py` or another `.py` file directly. A directory that
   merely has Python files somewhere deeper (a Rust crate `engine/dizione` with a script in
   `engine/dizione/scripts/`) is not a Python package and never matches. Its message names the
   first such path in path order and says that no import
   root of the importing file reaches it. This is how a script reachable only through a `Makefile`
   `PYTHONPATH` is seen;
3. else it is a third party or standard library module and is ignored.

#### Dynamic imports

`importlib.import_module(x)` and `__import__(x)` (the call spelled exactly so; a bare
`import_module(x)` is not recognised) resolve like `import x` when `x` is a plain string literal:
no prefix but `r` or `u`, no escapes, no f-string, no implicit concatenation. The import has kind
`runtime`, is located at the start of the call and uses no symbol (a module import, see "Symbols").
Its specifier in `unresolved` and `skipped` is the string contents.

A literal that starts with dots is relative to a package. The package is the second argument, given
positionally or as `package=`, and must be a plain string literal, an absolute dotted name. The module
is that name with `dots - 1` trailing segments removed, then the rest of the literal appended, and it
resolves as an absolute import (roots, probing, misses). When the package is missing, not a literal,
or has too few segments, the call stays `dynamic-specifier`, with the string contents as specifier.
Every other argument (a name, a call, an f-string, a concatenation, a bytes or escaped string, a
literal that is not a dotted name) stays `dynamic-specifier` with the source text of the argument as
specifier.

#### File loaders

`importlib.machinery.SourceFileLoader(name, path)` and `importlib.util.spec_from_file_location(name,
path)` (spelled exactly so) load a Python file by path. When `path`, the second argument, is a plain
string literal (as above) that holds no `*`, `?`, `[`, `{`, `%` or `$`, does not start with `/` or `~`,
and names a known source file, either relative to the importing file's directory (tried first) or to
the repository root, the call is an import of that file, kind `runtime`, located at the start of the
call, using no symbol. The path is normalised lexically and may not leave the repository. Every other
path (a name, a call such as `os.path.join(..)`, a glob or format string, an absolute path, a file
that is not known) is skipped as `dynamic-specifier`, with the source text of the argument as
specifier; a literal that finds no file is not `unresolved-import`, because the file may be made at
run time or live outside the repository.

### Swift resolution

Files of one Swift module never import each other: the compiler sees the module as a whole. The
dependencies are between modules, so Swift imports resolve to modules that a `Package.swift`
declares, never to files by path.

**Targets.** `collect` reads every known file named exactly `Package.swift` and finds the calls
`.target(`, `.executableTarget(`, `.testTarget(`, `.macro(` and `.plugin(` (the leading `.` written,
in any position of the file). The manifest is parsed with the Swift grammar, so comments and strings
never produce a target. For each call:

- `name:` and `path:` are read from the arguments of the call itself, never from a nested call
  (`dependencies: [.product(name: "X", package: "Y")]` names nothing). Each must be a plain string
  literal: no interpolation, no escape. A target whose `name:` is missing or not such a literal, or
  whose `path:` is present and not such a literal, is skipped, because its directory is unknown;
- a call nested inside another matched call (`dependencies: [.target(name: "Core")]`) is a reference
  to a target and declares none;
- `.plugin(` declares a target only when it has a `capability:` argument; without one it is the
  plugin product of `products:`;
- the directory of the target is `path:` or, without it, `Sources/<name>` (`Tests/<name>` for
  `.testTarget(`), joined onto the directory of the `Package.swift` and normalised lexically. A
  directory that leaves the repository is dropped. `sources:` and `exclude:` are not read, and the
  fallback directories SwiftPM tries (`Source`, `src`, `srcs`) are not consulted;
- the module name is the target name with every character other than ASCII letters, digits and `_`
  replaced by `_` (`app-cli` is `app_cli`), as SwiftPM names it.

A manifest that cannot be read declares no targets, like an unreadable `Cargo.toml`.

**Imports.** `import X`, `@testable import X`, `@_exported import X`, `import X.Y` and
`import struct X.Y` are read from the `import` declarations of the grammar, so an import inside a
comment or a string is not one, and an import inside `#if` is. The module is `X`, the first dotted
segment. When `X` is the module name of no target, the import is ignored: Foundation, SwiftUI and
every other package are not repository files. Otherwise the target is chosen, then its entry file:

1. Among the targets with that module name, the nearest to the importer. A package that contains
   the importer (its `Package.swift` directory is an ancestor of the importing file) beats one that
   does not; then the longer shared leading directory path between the package directory and the
   importer wins; then the package directory first in path order. So a module name declared by
   several manifests resolves to the nearest one.
2. The entry file of the target is, among the known files:
   `<dir>/<name>.swift` for the target name, else `<dir>/<module>.swift` for the module name when it
   differs, else the first `.swift` file below `<dir>` (at any depth) in path order, ignoring a file
   named `Package.swift`. A target with no such file is ignored: its directory may be generated or
   excluded, and that is not an error.

The import is a resolved import of kind `runtime` at the line where the declaration starts (its
`@testable` or `@_exported` attribute included), and uses no symbol. The exception is an import
qualified with a declaration keyword (`struct`, `class`, `enum`, `protocol`, `func`, `var`, `let`,
`typealias`) and a dotted path `X.Y`: it uses the last segment, so `import struct AppCore.Frame`
uses `Frame`. `import X.Y` without a keyword imports a submodule and uses no symbol. Because an
import always lands on the entry file, a node must own the whole target directory: see "Code
units" for why `init` makes one node per target.

### Shell resolution

A Shell file is read with the bash grammar; zsh and dash scripts are read the same way.

**Commands.** `source X` and `. X`: a command whose name is exactly `source` or exactly `.` (so
`./x.sh` and `bash x.sh` are not imports), found anywhere in the file, in functions, conditionals and
loops, but not in comments, strings or here-documents. Only the first argument is the path, later
ones are ignored. The import is located at the line where the command starts, has kind `runtime`
and uses no symbol. The path has a value, or the reference is skipped as `dynamic-specifier`. The
specifier of a skipped or unresolved path is the text of the argument as written, quotes included.

**The value of a path.** A word or string made of parts joined together. A part is

- literal text: a bare word, a single-quoted string or the text of a double-quoted one. Text with a
  backslash is not literal;
- the script file, `$0`, `${0}`, `$BASH_SOURCE`, `${BASH_SOURCE}` and `${BASH_SOURCE[0]}`, only as the
  argument of `dirname` below;
- the script directory: `$(dirname F)` or `` `dirname F` `` with `F` the script file (quoted or not),
  `${0%/*}`, `${BASH_SOURCE%/*}` and `${BASH_SOURCE[0]%/*}`. It is the directory of the importing
  file;
- `$(cd P && pwd)`, also `pwd -P` and `pwd -L`, with `P` a path whose first part is a script
  directory: the directory `P` names;
- `$NAME` or `${NAME}`: the value of the nearest earlier assignment of `NAME` in the same file,
  where earlier is by position and an assignment may sit anywhere (also after `export`, `local`,
  `readonly` and `declare`). Only an assignment whose value is a path starting with a script
  directory (the forms above, optionally followed by literal text such as `/..`) has a value; when
  the nearest earlier assignment is any other, or there is none, the path is dynamic. An older
  assignment is never used instead of the nearest. `ROOT="$(cd "$(dirname "$0")/.." && pwd)"`
  followed by `. "$ROOT/device/lib/x.sh"` is the standard case.

Any other part (`$HOME`, `$f`, `${x:-y}`, a command substitution that is none of the above, an
arithmetic expansion, a `dirname` of anything but the script file) makes the path dynamic. A path
that starts with a script directory is anchored: the importing file's directory with the rest of the
text appended, `.` and `..` segments resolved lexically; a path that leaves the repository is
ignored. A path that is all literal text is a literal path.

**Resolution.** An anchored path is probed once, against the known files. A literal path that starts
with `/` or `~` names something outside the repository and is ignored. Any other literal path is
tried against the directory of the importing file and then against the repository root; the first
base where it finds a file wins. A path that finds no known file is an `unresolved-import`, except
when it names an existing asset (a tracked `.env`, a config file), which is ignored.

**Globs.** A path with `*` or `?` in its literal text is a pattern: matched against every known file
(any language), `*` and `?` staying inside one `/` segment, and every match, in path order, is an
import, except the importing file itself. A pattern that matches nothing is an `unresolved-import`.
A path that holds `[`, `{`, or a segment `**` is dynamic. Quoting is not looked at: a shell never
expands a quoted `*`, but `source "$ROOT/lib/*.sh"` is written to load those files, and that is the
dependency worth seeing.

### Svelte and HTML

Two more source languages carry TypeScript or JavaScript inside markup. Their scripts are resolved,
typed (`kind`, `test_only`), recorded (`symbols`) and skipped exactly like the imports of a
TypeScript or JavaScript file: aliases, workspace packages, package `imports`, assets, probing and
the declared-package check all apply, governed by the config and `package.json` files above the
Svelte or HTML file. Everything outside the script elements is ignored: markup, text, `<style>`, and
text that merely looks like an import.

**Svelte** (`.svelte`, always source). Every `<script ...>` element is read, whatever its
attributes, so the instance script, `context="module"` and the Svelte 5 `module` attribute all
count. Its text, the characters between the end of the start tag and the end tag, is parsed as
TypeScript when the `lang` attribute is `ts` or `typescript` (ASCII case-insensitive) and as
JavaScript otherwise. Lines are lines of the `.svelte` file, not of the script: a statement on the
third line of the file is reported at line 3. A `src` attribute on a Svelte script is not an import.

**HTML** (`.html`, `.htm`). A file is source when its markup holds a `<script` element, a start tag
whose name is `script` (ASCII case-insensitive) outside comments, whatever its `type`, `src` or
content. A file without one is an asset. A file that cannot be read as UTF-8 is an asset too, so the
check cannot fail a scan. These are read, in source order, each at the line where its start tag
begins:

- A script with no `src` whose `type` is absent, empty, `module`, `text/javascript` or
  `application/javascript` (ASCII case-insensitive): its text is parsed as JavaScript, lines being
  lines of the HTML file. Other types (`importmap`, `application/json`, templates) hold no
  imports and are ignored.
- A script with a `src` and one of those types: an import of that path, see below.
- `<link rel="modulepreload" href="...">`, `rel` being a whitespace separated list of tokens
  (ASCII case-insensitive): an import of the `href`.
- `<link rel="..." href="...">` whose `rel` list holds `stylesheet`, `preload`, `prefetch`, `icon`,
  `apple-touch-icon` or `manifest` (and no `modulepreload`): an asset reference to the `href`. It is
  of kind `asset` and uses no symbol, whatever file it finds, so a dependency on the node that owns
  that file as for an asset import (see "Asset files"). Other `<link>` elements (`canonical`,
  `alternate`, ..) are not read.

An import of a path (`src`, `href`) is of kind `runtime` (`asset` for a resource link) and uses no
symbol: it loads the whole file.
Its value is trimmed and resolved as a document-relative URL: `main.js` means `./main.js`, `../x.js`
and `./x.js` are relative to the directory of the HTML file, and a query or fragment is dropped as
for any specifier. The file is probed like every relative path (see "Probing"), and a path that
finds nothing is an `unresolved-import`, or `unresolved-root-path` when root-absolute, for a
resource link as for a script. A remote `href` and an empty one are ignored.

**Vite root.** A root-absolute specifier is one that starts with a single `/`. In an HTML file, in a
`src` or `href` and in an `import`, `export ... from`, `import()` or `require()` of its inline
scripts, it names a path below the Vite project root. The root of an HTML file is the first of

1. **The Vite config.** The nearest ancestor directory of the HTML file, the file's own directory
   first, that holds a Vite config: a file named `vite.config.js`, `.mjs`, `.ts`, `.cjs`, `.mts` or
   `.cts` (when a directory holds several, the first in that order, which is Vite's own). The root is
   that directory, adjusted by the `root` option of the config when the option can be read, see
   below. A Vite config sits above a nearer `package.json`: `ui/vite.config.ts` makes `ui` the root
   of `ui/index.html` whatever `package.json` files lie around;
2. **The nearest `package.json`.** The nearest ancestor directory, the file's own directory first,
   that holds a `package.json`;
3. **The repository root.**

So `/src/forest-paint.ts` in `dev/index.html` is `<root>/src/forest-paint.ts`.

The `root` option is read from the config with the TypeScript or JavaScript grammar (by extension),
never by running it. The config object is the operand of `export default`, of `module.exports =`, or
the top level `const` an identifier there names, looking through parentheses, `as` and `satisfies`; a
call such as `defineConfig(..)` stands for its first argument that is an object or a function, and a
function stands for the object it returns (an arrow function's expression body, or the first `return`
of its block). Only a `root` key directly in that object counts (`test.root` and the like do not).
Its value is read when it is one of

- a plain string literal, a path relative to the config directory (`'ui'`, `'./ui'`, `'.'`);
- `__dirname` or `import.meta.dirname`, or `path.dirname(fileURLToPath(import.meta.url))` or
  `path.dirname(__filename)`: the config directory;
- `process.cwd()`: the config directory, which is where Vite runs from;
- `path.resolve(D, 'a', 'b')` or `path.join(D, 'a', 'b')` (the callee may be written with or
  without `path.`), where `D` is any of the directory forms of this list or a plain string literal
  and every other argument is a plain string literal: the directory joined with the literals, `..`
  resolved lexically. A literal first argument is relative to the config directory, so
  `path.resolve('engine/wasm/pkg')` with one argument is `engine/wasm/pkg` below it, as Vite's
  working directory is that of the config; an absolute literal is not recognised;
- `fileURLToPath(new URL('<literal>', import.meta.url))`, also written `new URL(..).pathname`: the
  literal joined onto the config directory.

Any other value (a variable, an absolute path, a path that leaves the repository,
an expression driftwall does not recognise) means the config directory, as does a config that
cannot be read or has no `root`. The extraction functions that are given no file system (`extract`)
know no Vite configs, so they use rules 2 and 3.

A root-absolute specifier is probed like a relative path against the root. A file that Vite would
serve from `<root>/public/` (`/logo.svg` for `<root>/public/logo.svg`) is ignored. A root-absolute
specifier that still finds nothing is never dropped: it is skipped with reason
`unresolved-root-path` (see "Skipped references"), whose message names the root that was used, so
a wrong root is visible. Root-absolute specifiers are not special in TypeScript, JavaScript or
Svelte files.

A remote URL is ignored: a specifier that starts with `//` or with a scheme (`https:`, `http:`,
`data:`, `blob:`, ..: a letter, then letters, digits, `+`, `-` or `.`, then `:`). In an HTML file
this holds for `src`, `href` and for the specifiers of inline scripts, which are not skipped as
undeclared packages either.

#### The markup scanner

Svelte and HTML are read by a small scanner, not a parser. It walks the text for `<` and handles:

- comments `<!-- ... -->` (to the first `-->`, or the end of the file when unterminated): nothing
  inside is read, so a commented-out script is ignored. `<!doctype ...>` and `<? ... ?>` are skipped
  to their `>`, end tags are skipped, and a `<` not followed by a letter, `/`, `!` or `?` is text;
- start tags, with attributes in any order and any case of the name: a name, optionally `=` and a
  value written in double quotes, single quotes or unquoted (up to whitespace or `>`). A quoted
  value may span lines and contain `>`. A repeated attribute counts once, the first. In Svelte
  a value may also be a `{ ... }` expression, read to its matching brace with quoted strings inside
  skipped, so `on:click={() => run()}` ends where it should. A tag with no closing `>` ends the scan;
- the raw text elements `<script>` and `<style>`: the content runs to the first `</script` or
  `</style` (ASCII case-insensitive) followed by whitespace, `/` or `>`, or to the end of the file.
  So a string holding `</script>` ends the script as it does in a browser, and a `<script` inside a
  `<style>` is not an element;
- a start tag written `<script ... />` has no content and consumes no text after it, in HTML as in
  Svelte. HTML itself does not treat `/>` on a script as self-closing; driftwall does, because it
  is how such tags are written and the alternative would swallow the rest of the file;
- CRLF and LF line endings: a line is counted at each `\n`, so `\r\n` files report the same lines
  as `\n` files.

It does not handle character references (`&amp;` in a `src` is read as written), `<template>`
content as separate from the document, CDATA sections, `<textarea>` or `<title>` raw text, Svelte
`{...}` expressions in text (`{@html '<script>..</script>'}` is read as a script), conditional
comments, or an unquoted attribute value that contains `>`. A script inside any element, such as
`<svelte:head>`, is found like one at the top level.

### Import location

An import's line is the line where its statement starts, not the line of the specifier string: the
`import` or `export` keyword of a TypeScript or JavaScript statement, the first token of a
`require(..)` or `import(..)` call expression, the `import` or `from` keyword of a Python statement,
and the `use` declaration of Rust (its `pub` when it has one, never the attributes above it). A
statement that spans several lines is one location, its first line. A Rust qualified path written
in code (`crate::gitlab::fetch(..)`, or a path inside a macro argument) is not a statement and keeps
the line of the path itself. A skipped Python `importlib.import_module(..)` call is located at the
call start, and a Rust embed at the start of its macro invocation. In a Svelte or HTML file every
line is a line of that file, not of the script, and a `src` or `href` is located at the start of its
tag (see "Svelte and HTML").

### Import kind

Every resolved import carries a `kind` (`Runtime`, `TypeOnly`, `Embed` or `Asset`, serialized as the
kebab-case strings `runtime`, `type-only`, `embed`, `asset`) and a `test_only` flag. Neither changes which
edges exist: the linter treats every kind and test import like any other. They are recorded so that
callers can decide what a type-only or test import is worth.

`type-only`:

- TypeScript and JavaScript: `import type ...`, `export type ... from`, an import or re-export where
  every specifier is written `type X` (`import { type A, type B } from`), every import in a
  declaration file (`.d.ts`, `.d.mts`, `.d.cts`), every JSDoc `import('x')` type (see "JSDoc type
  imports"), and every `import("x")` written in a TypeScript type (see "Symbols and public
  surfaces"). `import { type A, B }` is `runtime`, as is a
  default or namespace import beside type specifiers, `import {} from`, and `require(..)` or
  `import(..)`.
- Python: an import anywhere inside the body of `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:`
  (any nesting depth below it). Its `else` and `elif` branches and any other condition are `runtime`.

`test_only`:

- Rust: an import written inside an item or module annotated `#[cfg(test)]`, at any nesting depth
  (`#[cfg(test)] mod tests { fn f() { use crate::a::B; } }`), including a `use` that is itself
  annotated, and inside a module whose body starts with the inner attribute `#![cfg(test)]`. Only
  the exact predicate `test` counts: `cfg(any(test, ..))`, `cfg(all(test, ..))` and `cfg(not(test))`
  do not. An import in a file under a `tests/` directory or named `tests.rs`, `*_test.rs` or `*_tests.rs` is
  `test_only` whatever it is written in. So is an import in a file that a `#[cfg(test)]` module
  declaration pulls in: `#[cfg(test)] mod tests;`, `#[cfg(test)] mod fixtures;`, any name, in any
  file and at any depth (also inside an inline module, and a declaration annotated through its
  module, `#[cfg(test)] mod a { mod b; }`). The file is found through the module-path model, the
  same one that resolves `use crate::..` paths: relative to the declaring file and its inline
  modules, `<name>.rs` or `<name>/mod.rs`. A declaration that finds no known file, and one whose
  attribute is not exactly `cfg(test)` (see above), pulls in nothing. The modules a test file
  declares, with or without an attribute (`mod support;` in `tests.rs`), are test code in turn, to
  any depth. `#[path = ".."]` is not followed. The declaring file is not made test code.
- TypeScript, JavaScript, Svelte and HTML: files named `*.test.*` or `*.spec.*` (a middle name segment
  equal to `test` or `spec`, so `a.test.ts`, `a.spec.tsx`, `a.test.svelte`), or under a `__tests__`,
  `test` or `tests` directory at any depth.
- Python: files named `test_*.py`, `*_test.py` or `conftest.py`, or under a `tests` or `test`
  directory at any depth.
- Swift: files under a directory named `Tests`, `Test`, `tests` or `test` at any depth.
- Shell: files whose name up to the first `.` starts with `test_` or ends with `_test`, or under a
  `tests` or `test` directory at any depth.

`brief` and `neighbors` show the kinds. An edge whose sites are all `type-only`, `embed` or `asset`, or
all `test_only`, is annotated after its import count, `a --> b (3 imports) (type-only)`, one
annotation per such property (order `type-only`, `embed`, `asset`, `test-only`). When a property
holds for some sites only, the count is followed by one count per property that occurs, in the same
order: `a --> b (12 imports, 3 type-only, 2 test-only)`. An edge whose sites are all plain runtime
imports shows only `(N imports)`. In JSON every site carries `kind` and `test_only`.

Duplicates merge on (location, target) only when their kind and `test_only` agree. Imports that
reach the same file from the same location with different kinds (`import type { A } from './a';
import { b } from './a';` on one line) stay two entries, so a type-only use is never recorded as
runtime or the other way round. Entries are sorted by location, target, symbols, kind, `test_only`.

`asset`: a TypeScript or JavaScript import that resolves to an existing non-source repository file
(`./data.json`, `./style.css`, `../x.svg`, see "Asset files"). It uses no symbol, so it is never
`private-symbol`, it is a dependency on the node that owns the asset, and it is `asset` even inside a
declaration file. As for every import, an asset owned by nobody or by the importer's own node is no
edge.

`embed` by query: a bundler query makes an import of a source file a load of its text or URL, not of
its code. A TypeScript, JavaScript, Svelte or HTML import (also a root-absolute one, see "Svelte and
HTML") whose specifier has a query with a parameter named `raw`, `url` or `inline` (`./a.ts?raw`,
`./a.ts?url&v=2`, `./a.ts?inline`; the parameter name is what precedes any `=`) and that resolves to a
source file is `embed`: a resolved import that uses no symbol, so it is never `private-symbol`, and
still a dependency on that file's node. Any other query, `?worker` and `?sharedworker` included,
leaves the import `runtime`, and so does any query that holds `worker` or `sharedworker` beside
`inline` (`?worker&inline` is a worker). An import of an asset with such a query stays `asset`, and
one that resolves to nothing is as unresolved as without the query.

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
| `undeclared-package` | TypeScript or JavaScript specifier that is not relative, matches no `tsconfig` or `jsconfig` `paths` alias, is not a workspace package, is not a Node builtin and is not a declared dependency; a Python absolute import that misses below a namespace directory, a top-level directory with no `__init__.py` (see "Python resolution") |
| `dynamic-specifier` | a shell `source` or `.` whose path has no value, see "Shell resolution"; `import(expr)` or `require(expr)` whose argument is not a string literal or a template without substitutions; Python `importlib.import_module(..)` and `__import__(..)` whose first argument is not a plain string literal, or whose relative literal has no usable package, and a Python file loader whose path is not a literal naming a known file (see "Python resolution") |
| `embed-non-source` | a Rust embed whose target is not a known source file |
| `unreachable-module` | a Python absolute import that no import root resolves, whose top-level name matches a first-party module or package elsewhere in the repository (see "Python resolution") |
| `alias-target-missing` | a TypeScript or JavaScript specifier that a Vite `resolve.alias` entry matches whose target path has no file in the repository (see "Vite aliases") |
| `undeclared-owner` | an import of an asset or an excluded file whose owner is not a declared node, recorded by `lint` rather than by import extraction (see "Asset files" and "Excluded files") |
| `unresolved-root-path` | a root-absolute `src`, `href` or import specifier of an HTML file that finds no file below its Vite root (see "Svelte and HTML") |

A Vite virtual module is ignored, not skipped: a specifier that starts with `virtual:` or with the
character U+0000 (Rollup's prefix for modules a plugin generates, `virtual:pwa-register`) has no
repository file and no package to declare, so it is neither `undeclared-package` nor any other
skipped reason, and no census count. It is checked last, so an alias, a workspace package or a
`paths` pattern with that name still resolves first.

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
For a skipped Python call the specifier is the text of its first argument.

A skipped reference may carry a `message`, a sentence of explanation. `unreachable-module` has one:
it names the repository path the top-level name matches. So has `unresolved-root-path`: it names
the Vite root that was searched. So has `alias-target-missing`: it names the alias and its target, and
the node whose directory holds the target when the report knows the owners. And `undeclared-owner`: it names the asset or excluded file and its owner. `message` is absent from the
serialized form when there is none.

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
  default import, `*` for namespace imports, `require`, dynamic `import()` and `export *`. A
  `require(..)` or `import(..)` (awaited or not) that is the whole value of a declarator whose name
  is an object pattern uses the exported names the pattern lists: `const { a, b: c } = require('x')`
  uses `a` and `b`, and `const { a } = await import('x')` uses `a`. A rest element or a computed key
  in the pattern adds `*`, since the names it takes are not written. Any other `require(..)` or
  `import(..)` (`const m = require('x')`, `require('x').y`, an array pattern) uses `*`.
  An `import("x")` written in a type is `type-only` and uses the member written right after it,
  `import("x").A.B` uses `A`, and `typeof import("x")` uses `*`. It is in a type when it is
  below a type annotation (`let v: import("x").T`, a field or parameter), a `typeof` type
  query, type arguments, an `extends` or `implements` clause, the value of a type alias, or the
  type of `as` or `satisfies`; an `import()` in an expression is a dynamic import, whatever
  surrounds it. Its location is the line of the `import` word.
- Python: `from a import b, c` uses `b`, `c` when they resolve to names inside `a`. `from a import *`
  uses `*`. A module binding (`import a.b`, `import a.b as c`, `from a import submodule`, a literal
  `importlib.import_module("a.b")`) uses no symbol, as in Rust: it is a dependency on the module file.
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
`export *`, and Python `from a import *` all record `*`. A Rust or Python module binding records
nothing, so `use crate::gitlab;` is not private by itself, and each `gitlab::item` used through it is
checked against the surface at its own line, one per item. Python records no such item sites: a
Python module import is not checked against a surface.

`diff` reports surface changes as `+ exports gitlab: X` and `- exports gitlab: Y`, one line per
name; a node that loses its whole surface shows only `-` lines and is unrestricted from then on.
`brief` and `neighbors` list the symbols each edge uses (`uses: Client, fetch`, the first six then
`+N more`) and mark a declared surface (`exports: ...`); JSON carries `symbols` on every edge and
`exports` on a node that has a surface.

An import that uses no symbol is a use of the module itself (`use crate::gitlab;`, `from x import
sub`, `import a.b`, a side-effect `import './x'`): it is displayed as `module`, listed first in
`uses:` (`uses: module, Client`), and an edge with such a site has `"module": true` in JSON. It is
never displayed as `*`, which stays a glob or namespace import. An embed or an asset import uses no
module and no symbol and adds nothing to `uses:`.

## Rules (violation kinds)

| Kind | Severity | When |
| --- | --- | --- |
| `unmapped-file` | error | source file has no inherited `.arch` tag |
| `unknown-node` | error | `.arch` names a node the diagram does not declare |
| `undeclared-edge` | error | code imports across nodes and the diagram has no such edge |
| `stale-edge` | warning | solid edge with no import backing it |
| `planned-realized` | warning | dotted edge now backed by code, promote it to solid |
| `unresolved-import` | warning | relative import resolves to nothing, whatever its extension; Python absolute import into a first-party package that holds no such module; shell `source` of a path that finds no file |
| `private-symbol` | error | an import uses a symbol outside the target node's declared surface |

A dotted edge permits the import. `--strict` turns every warning into a failure.

## init

`driftwall init` reverse-engineers a baseline from an existing repository so that `driftwall lint`
on the result is clean at once (with the same `--contract`). It writes `architecture.mmd` and the
`.arch` tags (a contract without tags would leave every file unmapped); `--no-tags` writes the
contract only.

Source files are discovered like `lint` does: ignores are honored, vendored, minified and generated files are
excluded (see "Excluded files"), and no source files is an error. `init` decides, in this order,
which node every source file belongs to. A file is decided by the first rule that covers it.

1. **Existing tags.** Every file that already has an owner through an `.arch` file keeps it, with
   or without `--no-tags`, with or without `--force`. These are fixed decisions: `init` plans only
   for the files that have no owner, never rewrites or removes an `.arch` file it did not just
   create, and declares in the contract every node the existing tags name (also a tag whose
   directory holds no source). A hand written `src/.arch` with `node: *` therefore steers `init`.
   An existing `.arch` below a directory `init` tags is fine: the nearer tag wins, as everywhere.
   `--force` only lets `init` overwrite files it writes itself: the contract, and an `.arch` at a
   path it plans for that exists but that the scan did not see (a gitignored one). An existing
   `scope: here` tag covers only its own directory, so the files below it are planned like any
   others, and a plan that needs an `.arch` at that same path is a conflict as for any existing file.
2. **Code units** (only without `--depth`). Directories that a manifest or the layout names as one
   buildable or importable unit are decomposed into nodes, see below.
3. **The automatic cut** (only without `--depth`) for everything that is left. Large nodes that
   result from rules 2 and 3 are then cut once more, see "The layer split".
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
- **JavaScript unit**: every directory that holds a `package.json`, a `tsconfig.json` or a
  `jsconfig.json`, the repository root included. Workspace members are directories with a
  `package.json`, so they are units without their `workspaces` list being read (pnpm and lerna
  workspace files are not read either).
- **Python package**: a directory with an `__init__.py` that has no ancestor directory with an
  `__init__.py` (the outermost package; sub-packages belong to it).
- **Swift target**: the directory of every target a `Package.swift` declares, see "Swift
  resolution". The repository root is never a Swift target (a target with `path: "."` is not a
  unit), and manifests are read only in directories that hold, or lie above, a source file, like the
  others.

Manifests are read only in directories that hold, or lie above, a source file, so ignored trees are
never consulted. A `Cargo.toml` that cannot be read or parsed is skipped, like a broken one is for
import resolution. The existence of a `package.json`, `tsconfig.json` or `jsconfig.json` is enough
for a JavaScript unit: their contents are not read. A unit with no unowned source file is dropped.
A directory found by two rules is one unit, of the first kind in the order Rust crate, JavaScript
unit, Python package, Swift target. Two Swift targets that share a directory are one unit, named
after the first target in name order.

**Every unit is decomposed, except a Swift target.** A file belongs to the unit of its deepest ancestor unit directory.
Each unit has a source directory: a crate's `src/`; a JavaScript unit's `src/` when it holds a
source file there, else the unit directory; a Python package's own directory. How the source
directory is cut depends on the unit:

- **Module nodes: a Rust crate, a Python package, and the primary JavaScript unit.** Their files
  are modules. When the source directory directly holds two or more modules (source files and
  subdirectories both count, by distinct derived name: `a.ts` and `a.test.ts` are one, and so are
  `a.rs` and `a/`; only files of that unit count), it is tagged `node: *`
  (see "Prefixes"), so every module and subdirectory of it is a node named by the module rule.
  When a derived id is not valid, is reserved or is already taken (see "Ids"), the source
  directory is not decomposed. The primary JavaScript unit is also not decomposed when its source
  directory is the repository root and any source file of the repository lies in a subdirectory
  (see "Root files").
- **Directory nodes: every other JavaScript unit.** A JavaScript or TypeScript unit that is not the
  primary unit, including one declared only by a `tsconfig.json` or `jsconfig.json` and one that
  lies inside another unit's source directory, is a layer of the project and not a set of file
  modules, so its source directory is never `node: *`. It is split by subdirectory only: the files
  directly in the source directory are one group node (tagged `scope: here`, see "Contract and
  writing"), and each subdirectory of the source directory that holds source files of the unit is
  one node covering everything below it that no nearer tag or unit takes. A file added to a
  subdirectory stays in its node; a file added to the source directory joins the group. Every such
  unit is split, whatever it holds.
- **The rest of the unit.** When the source directory is a subdirectory of the unit, the other files
  of the unit are directory nodes too: each subdirectory of the unit directory other than the
  source directory (`test/`, `scripts/`, `dev/`) is one node covering everything below it that no
  nearer tag or unit takes, and the files directly in the unit directory are one node of the unit
  directory (the unit group, tagged `scope: here`, see "Contract and writing").
- **A unit that is not decomposed.** The primary unit (below) decides nothing: its files are cut
  by rule 3 like any other. Every other crate or package is one directory node covering its whole
  directory, where a nested unit and nearer tags still win.

- **A Swift target is one node, never decomposed.** Files of a Swift module never import each other
  and every import of the module resolves to a single entry file (see "Swift resolution"), so a
  `node: *` cut would put each file in a node of its own: the edges to the module would land on the
  entry file's node and every other file of the module would be a node nothing points at. The whole
  target directory is one directory node, tagged with a plain `node:`, and its id is the target
  name sanitised (see "Ids"), without the prefix of any unit. A Swift target is never the primary
  unit and never counts when the primary unit is chosen. Where a Swift target lies inside another
  unit (a `package.json` above `Sources/`), its own files belong to the target, as for every nested
  unit.

A unit that sits inside another unit takes its own files, so a root `package.json` next to a
sub-project `ui/` with its own `package.json` is two units. A sole unit is decomposed like any
other: a lone JavaScript or Python unit gets module nodes like a lone crate does.

The **primary unit** is the unit at the repository root, or else the only unit. It has no prefix;
every other unit has the prefix `<unit-id>-`, where the unit id is the unit directory's path with
its segments joined by `-` and sanitised as an id (see "Ids"): `ui/` is `ui-`, `engine/core` is
`engine-core-`. So `ui/src/components/` becomes the node `ui-components` when `ui/` is not primary,
and `components` when it is.

### The automatic cut

- Starting at the root, descend while the directory holds no source file itself and exactly one
  child directory holds sources. The directory where that stops (own source files, or two or more
  source-bearing children) is the fork; N is its depth plus one. So code under `src/` alone yields
  `src/auth` and `src/billing`, not one `src` node, while a repo with one root module plus `lib/`
  yields `root` and `lib`, and one directory holding one module yields a single node.
- When the fork directly holds two or more modules (here only source files directly in the fork
  count, not subdirectories: a fork with one file and two subdirectories keeps directory nodes,
  unlike a unit's source directory), it gets `node: *` and every module and
  subdirectory is a node named by the module rule, so contract ids are exactly the ids the linter
  derives, labelled `<fork>/<id>`. A fork holding one module keeps directory nodes. When any file
  under the fork would derive an id the contract cannot declare (`.eslintrc.js`, `end.rs`) or an id
  that is already taken, `init` falls back to directory nodes instead of writing a baseline that
  cannot lint.
- **Root files.** The repository root is never a `node: *` source root when it also has source
  directories: its own files (`vite.config.ts`, `eslint.config.js`, `build.rs`, `setup.py`) are
  tooling next to the code, and become one node `root` (tagged `scope: here`) beside one node per
  source directory. A root that holds only files (flat scripts) still gets `node: *`. A project
  that wants root modules writes a root `.arch` with `node: *` itself (rule 1). This holds when
  the root is the source directory of the primary JavaScript unit too: with a root `package.json`
  whose `src/` is itself a unit (it has a `tsconfig.json`), the files of the root unit are the
  tooling files and the directories beside `src/`, so they are cut here and not by `node: *`.

### The layer split

After rules 2 and 3 (never under `--depth`), a node that is too big to read is cut one more level.
A node qualifies when it stands for one directory (a directory node, or a module node whose files
all lie below one subdirectory of a `node: *` directory), owns at least 35 source files, and at
least one child directory of it holds at least 8 of those files. Then every child directory with
at least 8 of the node's files becomes a directory node `<id>-<child>` covering everything below it
that no nearer tag takes, and the node keeps the rest (its loose files and the small child
directories) as a directory node of the directory, with the same id it had and scope `subtree`
(small children are not covered by a tag of their own, so they must stay under the parent). A
module node that is split is no longer derived by `node: *`: its id is now declared by the tag at its
directory, and the nearer tags win. When the node has a name (`ui-components`, a Swift target) the
children are `<name>-<child>`; otherwise their ids come from the path like any directory node.

A node with more than 4 qualifying children is not split: that is a flat collection of features
(`src/ui` with `kit/`, `lesson/`, `mascot/`, ..), not layers, and splitting it would only multiply
nodes. The split applies once, to the nodes rules 2 and 3 produced, never to its own results. The
numbers 35, 8 and 4 were chosen on real repositories: they split a screens-plus-`settings/`-plus-`ui/`
directory of 38 files and leave a feature collection of 195 files, and 33-file directories, whole.

### Prefixes

`init` writes `node: *` with no prefix for the primary unit and for the automatic cut, and
`node: <unit-id>-*` for every other Rust crate and Python package. The prefix is part of the derived
id: a prefixed module is "taken" when `<prefix><name>` is. A non-primary JavaScript unit is not
tagged with a module prefix (it has directory nodes), but its subdirectory nodes are named with the
same prefix, see "Ids".

### Ids

- Directory node ids (units, directory cuts, the root node) come from the directory path below the
  longest prefix all new directory nodes share, segments joined with `-`, other characters replaced
  by `-` (repeats collapsed, ends trimmed, `n-` prepended to a non-letter start). A directory that
  is the shared prefix itself is named by its last segment, and the root directory is `root`. So
  `engine/core` and `engine/cli` next to `src` are `engine-core`, `engine-cli` and `src`. The
  directories of a unit that has a prefix (see "Prefixes") are the exception: they use their whole
  path, which does not count for the shared prefix, so the `test/` directory of the unit `ui/` is
  `ui-test`, in step with its module ids `ui-...`, and the group of the unit `ui/` is `ui`.
- A subdirectory of the source directory of a non-primary JavaScript unit is named like the module
  of that name: the unit prefix followed by the directory name, sanitised as above, so
  `ui/src/components/` is `ui-components` (not `ui-src-components`). The loose files of the source
  directory are named by the whole path, so `ui/src` is `ui-src`; the unit group is the unit id
  (`ui`), and a directory of the unit outside the source directory uses its whole path (`ui/dev` is
  `ui-dev`). The normal collision rules apply when two of these are the same id.
- A Swift target directory is named by its target name: the name sanitised as above replaces the
  path derivation, and the directory does not count for the shared prefix of the others. The same
  collision rules as for any directory node apply.
- Module ids (`node: *`) are fixed by the module rule and are never renamed. Node ids named by
  existing tags are fixed too. Two existing tags may name the same node on purpose (that is
  grouping); `init` itself never merges two units silently.
- A directory node whose id is a fixed one (module, or existing tag) is named `<id>-dir`, then
  `<id>-dir-2`, `<id>-dir-3`, so a module `src/ui.rs` (node `ui`) and a directory `ui/` give `ui` and
  `ui-dir`. Any other collision between directory nodes, and the reserved words `end`, `flowchart`
  and `subgraph`, get `-2`, `-3`, ... in sorted directory order. This collision handling is a
  backstop: prefixes keep the ids of different units apart in the ordinary case.
- A `node: *` region (rule 2 or 3) whose derived ids are not valid, or collide with a fixed id or
  with an earlier region's ids, is not created; its files get directory nodes instead.
- **Labels name paths that exist.** A directory node is labelled with its directory (`.` for the
  repository root). A module of a `node: *` directory is labelled `<directory>/<name>`, the path
  of its file without extension (`src/cli` for `src/cli.ts`) or its subdirectory, except the
  module that only test files make (see "Module nodes"): it has no file or directory of its own,
  so it is labelled with the directory (`src`, not `src/src`). A node that only an asset or an
  excluded file names is labelled with the subdirectory below the tag that holds it, or with the
  asset's own path when it lies directly in the directory (`src/index.css`, not `src/index`). The
  same labels come from existing tags (`Existing`). A test asserts that every label `init` writes
  names an existing path.

### Contract and writing

- Every resolved import between files with different owners becomes one solid edge. Nothing is
  dotted, so a fresh baseline has no stale or planned-realized findings. Imports are resolved with
  the walked non-source files (assets) known, as `lint` does: an asset (a `.json`, a `.css`) has the
  owner the tags give it, the existing ones and the ones `init` writes, and an import of an asset
  owned by another declared node is an edge. The same holds for an excluded file (see "Excluded
  files"), which imports resolve to too. An asset or excluded file no tag covers adds no edge,
  exactly as in `lint`. **Fresh-baseline invariant:** an asset, or an excluded file that some import
  resolves to, whose owner a tag gives it is owned by a declared node: when the owner is a name only
  such a file derives under a `node: *` tag (`app.css` beside `App.svelte`, `index.css`, a directory
  of fonts, a generated `route-table.ts` beside `bus.ts`), `init` declares it as a node of its own,
  labelled with the directory or path it comes from, so `lint` right after `init` records no
  `undeclared-owner` skipped reference caused by `init`'s own choices. An excluded file that nothing
  imports declares nothing. Every node that a directory tag `init`
  writes names is declared, so an asset in a directory of a unit (`ui/src/index.css`, imported by
  `ui/src/main.tsx`) is owned by a node the contract declares (`ui-src`): `where` and `lint` never
  name a node `init` left out.
- A directory tag that is a group also gets `scope: here`: the repository root group (the node of
  the files directly in the root, usually `node: root`), a unit group and the group of the loose
  files in the source directory of a non-primary JavaScript unit (see "Code units"), provided every
  file the tag owns sits directly in its directory. A directory added later is then
  not covered by inheritance, and `unmapped-file` reports its sources. Every other directory tag
  inherits as always, because the files below it belong to it.
- `init` refuses (exit `2`) when the contract or an `.arch` it would write exists, listing every
  such path, and checks all of them before writing anything. `--force` overwrites. Running `init`
  again therefore needs `--force` only for the contract, and rewrites it to the same contents,
  because every file is then owned by an existing tag.

## Exit codes

`0` clean. `1` violations. `2` tool error (bad diagram syntax, IO failure, bad arguments).
A failing lint is not an error, so `anyhow` is used only at the entry point and always maps to `2`.

## CLI surface

```
driftwall lint [PATH] [--contract FILE] [--format text|json|sarif] [--strict]
driftwall init [PATH] [--depth N] [--contract FILE] [--no-tags] [--force]
driftwall where <FILE> [--root DIR] [--contract FILE] [--format text|json]
driftwall brief [PATH] [--contract FILE] [--format text|json]
driftwall neighbors <NODE> [PATH] [--contract FILE] [--format text|json] [--files] [--all-sites]
driftwall diff --base <REF> [PATH] [--contract FILE] [--format text|json] [--exit-code]
```

`--format json` is the agent interface. Every violation carries `kind`, `severity`, `at`
(`file`, `line`), `message`, `hint`.

`where <FILE>` says what a path is and who owns it. The path must exist (inside `--root`, default
`.`): a missing one is a tool error, exit `2`, `no such file: <path>`. `--contract` is accepted like
everywhere else and must name a readable, valid contract when given; `where` reads nothing else from
it. The text is `<file>: <answer>`, one of

| What the file is | Answer | Exit |
| --- | --- | --- |
| source file with an owner | `<node> (via <arch file>)` | `0` |
| source file no tag covers | `unmapped` | `1` |
| asset | `asset of node <node> (via <arch file>)`, or `asset, unmapped` | `0` |
| unread code file (see "Blind spots") | `not read (<key>) in node <node> (via <arch file>)`, or `not read (<key>), unmapped` | `0` |
| excluded source file | `excluded (<reason>)` | `0` |
| source file `.driftwallignore` hides | `ignored by .driftwallignore` | `0` |

A directory is answered like a source file, by the tag that covers it. The JSON has `file`, `node`,
`arch_file` and `kind` (`source`, `asset`, `unread`, `excluded`, `ignored` or `directory`), plus
`detail` for the unread key and the exclusion reason (absent otherwise). `node` and `arch_file` are
`null` for an excluded or ignored file, and for anything no tag covers.

`brief`, `neighbors` and `diff` are read-only maps for agents and reviewers:

- `brief` prints every node (directories, file count, see "Node directories" below) and every edge, declared or observed, with
  the number of import sites that realise it and how they are used (kinds, symbols, see "Import
  kind" and "Symbols and public surfaces"). An edge code uses and the contract lacks is marked
  `UNDECLARED`. It is informational: exit `0` unless the tool fails (`2`).
  It ends with the blind spots block, and a node line ends with ` blind: 3 not read, 1 excluded,
  2 ignored, 4 skipped, 1 unresolved` (only the non-zero parts; absent when all are zero):

  ```
  blind spots:
    not read: swift 40 files (1200 lines), sh 10 files (300 lines)
    excluded: vendored 3, generated 2
    ignored by .driftwallignore: 4
    skipped references: undeclared-package 5, dynamic-specifier 1
    unresolved imports: 2
    unmapped: 2
      not read: loose/tool.pl (pl, 1 line)
      excluded: loose/app.min.js (minified)
  ```

  A line is present only when its count is not zero (`unmapped` counts the items attributed to no
  node, over all categories). The `unmapped` line is followed by those items, one per line and four
  spaces indented, in the forms and the order of the `blind spots:` block of `neighbors` below (not
  read, excluded, ignored, skipped, unresolved), so every one is named by its path. At most 20 are
  listed, then one line `    +N more` for the rest. Unread extensions are ordered by file count, then name. Excluded
  reasons and skip reasons are in the order of this document. When every count is zero the block is
  the single line `blind spots: none`. In JSON, `blind_spots` has `unread` (key to `{count, lines}`),
  `excluded` (reason to count), `ignored`, `skipped` (reason to count), `unresolved`, `unmapped`
  (a tally with the same five counts) and `unmapped_items` (every item no node owns, uncapped, in the
  shape of the `blind_spots` of `neighbors` JSON: `unread` per key with `count`, `lines` and `files`,
  `excluded` per reason, `ignored`, `skipped` per reason, `unresolved`, each item with its file and
  `node: null`), and every node has `blind_spots`, a tally with `unread`, `excluded`,
  `ignored`, `skipped` and `unresolved` file or reference counts.
- **Node directories.** The `dirs` of a node, in `brief`, `neighbors` and the JSON `dirs`, are where
  its code lives, not where its `.arch` file lies. They are the union, sorted and without
  duplicates, of

  - the directory of every `.arch` file whose plain `node: <id>` names it (the repository root is
    `.`), also when no file is left under it;
  - the module path of every source file the node owns that a `node: *` or `node: <prefix>*` tag
    owns: the tagged directory joined with the first path segment below it. A file in a
    subdirectory is the directory module `src/ui/**`, shown as `src/ui`; a file directly in the
    tagged directory is a file module, shown as the file itself (`src/gitlab.rs`; `a.ts` and
    `a.test.ts`, which share the node `a`, are two entries). The tagged directory itself is never
    shown for such a node, and a node whose files are all assets has no entry of this kind.

  A `scope: here` group is a plain tag: its directory. The rule reads only the source files the
  node owns and the tags, so it is the same in `brief`, `neighbors` and JSON, and for every run.
- `neighbors <NODE>` prints one node's incoming and outgoing edges with up to 3 `file:line`
  example sites each (`--all-sites` prints every site; JSON always has every site, with no cap
  anywhere). `--files` lists the node's source files under its `files:` line, two spaces indented
  (JSON always has `files`, the sorted list). It also lists, always, everything the node is blind to
  in a `blind spots:` block, one item per line (`blind spots: none` when it has none):
  `  not read: <file> (<key>, <n> lines)`, `  excluded: <file> (<reason>)`,
  `  ignored: <file>`, `  skipped: <file>:<line> <specifier> (<reason>)` (followed by ` - <message>`
  when the skip has one) and `  unresolved: <file>:<line> <specifier>`. JSON has the same items under
  `blind_spots` (`unread` per key with `count`, `lines` and `files`, `excluded` per reason, `ignored`,
  `skipped` per reason, `unresolved`; every item has its `node`, and `lines` for unread files). An
  unknown node is a tool error that lists the declared ids.
- `diff --base <REF>` compares the contract at a git ref with the working tree (nodes and edge
  kinds; labels and boundaries are ignored). The old text comes from `git show REF:./<contract>`;
  a contract absent at that ref counts as empty, any other git failure is exit `2`. Exit `0`
  normally; with `--exit-code`, `1` when anything changed, so CI can require approval.

`lint` text output ends with the note line of "Blind spots" when any blind spot exists, after the
summary line, and its JSON gains a `blind_spots` object (the summary of `brief`, without `unmapped`)
as its last key. Exit codes are those the findings decide, whatever the blind spots; SARIF is
unchanged. `init` prints the same note line after the line that says what it wrote.

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
