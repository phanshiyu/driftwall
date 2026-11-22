# Working on driftwall

Read `docs/DESIGN.md` first, then `architecture.mmd`. The diagram is the contract for this repo.
Before adding a dependency between modules, check the edge exists in the diagram. If it does not,
stop and report it. Never edit `architecture.mmd` to make something pass.

## Gates, run before every commit

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Rules for automated builders

- Work only in your own git worktree and set your own `CARGO_TARGET_DIR`.
- Kill only processes you started, by recorded PID. Never `pkill -f` or `killall`.
- Delete only exact paths you created. No globs in `rm`.
- No `unwrap`, `expect`, `panic`, indexing, or `as` casts in non-test code. No comments that
  narrate what the code does. A comment states a non-obvious why.
- Every public item has a doc comment. Every behavior has a test that fails when it breaks.
- Output must be deterministic: ordered collections only.
- Report what you did not verify. Do not claim a gate passed without running it.
