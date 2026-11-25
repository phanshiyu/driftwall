#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
# why: a pre-commit hook exports GIT_DIR, GIT_INDEX_FILE and friends; a test that spawns git would
# inherit them and write to this repository (it once set core.bare=true). Tests get a clean env.
while IFS= read -r name; do unset "$name"; done < <(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p')
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked --quiet
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked --quiet
cargo run --locked --quiet -- lint . --strict
echo "gate: PASS"
