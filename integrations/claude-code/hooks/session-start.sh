#!/usr/bin/env bash
set -uo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0
[ -f architecture.mmd ] || exit 0
echo "Architecture contract (architecture.mmd), enforced by driftwall. Solid arrows are enforced dependencies, dotted arrows are planned."
cat architecture.mmd
