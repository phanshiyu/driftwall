#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked --quiet
echo "gate: PASS"
