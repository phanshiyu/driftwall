# Working on driftwall

Read `docs/DESIGN.md`, then `architecture.mmd`. The diagram is the contract for this repo. If a
change needs a dependency between modules that the diagram does not declare, stop and report it.
Never edit `architecture.mmd` to make something pass.

Gates, run before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`. Style, error handling, docs and ordering are enforced by those gates, not here.

Rules a linter cannot see:

- Work only in your own git worktree and set your own `CARGO_TARGET_DIR`.
- Kill only processes you started, by recorded PID. Never `pkill -f` or `killall`.
- Delete only exact paths you created. No globs in `rm`.
- A comment states a non-obvious why, never what the code does.
- Report what you did not verify. Do not claim a gate passed without running it.
