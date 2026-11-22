# Working on driftwall

Read `docs/DESIGN.md`, then `architecture.mmd`. The diagram is the contract for this repo. If a
change needs a dependency between modules that the diagram does not declare, stop and report it.

Everything else is enforced by tooling, not by this file:

- `scripts/gate.sh` runs as the pre-commit hook. A failing commit cannot be made.
- `.claude/settings.json` guard blocks pkill, `--no-verify`, pushes, glob deletes, and edits to
  the contract and the lint configuration when working in a worktree. If you need one of those,
  stop and report what and why.
- Style, errors, docs, ordering and comment form are lints and tests.

Judgment calls that remain yours: report what you did not verify, and say where the spec was
ambiguous and what you chose.
