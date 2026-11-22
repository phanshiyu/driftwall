#!/usr/bin/env bash
# Usage: scripts/dogfood.sh <driftwall-binary> <out-dir> <repo>...
# For each repo: a worktree on local branch driftwall-dogfood at <out-dir>/<name>, a fresh
# `driftwall init` writing docs/architecture.mmd, and the evidence a reviewer needs in <out-dir>/<name>.evidence/.
set -uo pipefail
bin=$1; out=$2; shift 2
mkdir -p "$out"
for repo in "$@"; do
  name=$(basename "$repo")
  wt="$out/$name"
  ev="$out/$name.evidence"
  if [ ! -d "$wt" ]; then
    git -C "$repo" worktree add -q -B driftwall-dogfood "$wt" HEAD || { echo "$name: worktree failed"; continue; }
  fi
  git -C "$wt" reset -q --hard HEAD
  git -C "$wt" clean -qfdx -e node_modules -e target
  rm -rf "$ev"; mkdir -p "$ev"
  ( cd "$wt"
    git ls-files | sed -n 's/.*\.\([A-Za-z0-9]*\)$/\1/p' | sort | uniq -c | sort -rn > "$ev/extensions.txt"
    start=$(date +%s)
    "$bin" init . --force --contract docs/architecture.mmd > "$ev/init.txt" 2>&1; echo "exit=$?" >> "$ev/init.txt"
    "$bin" lint . --strict --contract docs/architecture.mmd > "$ev/lint.txt" 2>&1; echo "exit=$?" >> "$ev/lint.txt"
    "$bin" lint . --format json --contract docs/architecture.mmd > "$ev/lint.json" 2>/dev/null
    "$bin" brief . --contract docs/architecture.mmd > "$ev/brief.txt" 2>&1
    "$bin" brief . --format json --contract docs/architecture.mmd > "$ev/brief.json" 2>/dev/null
    end=$(date +%s)
    echo "seconds=$((end - start))" > "$ev/timing.txt"
    cp docs/architecture.mmd "$ev/architecture.mmd" 2>/dev/null
    find . -name .arch -not -path './node_modules/*' | sort | while read -r f; do echo "$f: $(cat "$f")"; done > "$ev/arch-files.txt"
  )
  echo "$name: $(tail -1 "$ev/init.txt") $(grep -m1 '^driftwall' "$ev/lint.txt") $(cat "$ev/timing.txt")"
done
