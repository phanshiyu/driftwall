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
- Asset imports: a relative import of an existing non-source file (`./data.json`, `./style.css`) is a
  dependency on the node that owns it, with the new import kind `asset`. A relative import of a
  missing file of any extension is now `unresolved-import`. The import kind `typeonly` is now
  serialized as `type-only`.
