# Changelog

## Unreleased

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
- HTML `<link>` elements with a `rel` of `stylesheet`, `preload`, `prefetch`, `icon`,
  `apple-touch-icon` or `manifest` are asset references to their `href`, resolved like a script `src`.
- Vite virtual modules (`virtual:pwa-register`, a `\0` prefix) are ignored instead of being skipped
  as `undeclared-package`.
- TypeScript types: `let x: import("./a").T`, a field typed `import("./a").T` and `typeof import("./a")`
  are `type-only` imports using `T` (the first member) or `*`, not runtime `*`.
