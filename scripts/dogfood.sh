#!/usr/bin/env bash
# Usage: scripts/dogfood.sh <driftwall-binary> <out-dir> <repo>...
# For each repo: a worktree at <out-dir>/<name> (local branch driftwall-dogfood) reset to the repo's
# HEAD with tracked .arch tags removed, a fresh `driftwall init` writing docs/architecture.mmd, and
# the evidence a reviewer needs in <out-dir>/<name>.evidence/.
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
  # why: the worktree branch is pinned at its creation commit; review the repo's current HEAD.
  git -C "$wt" reset -q --hard "$(git -C "$repo" rev-parse HEAD)"
  git -C "$wt" clean -qfdx -e node_modules -e target
  # why: review a tagless init, so committed .arch tags (driftwall's own) cannot do init's work.
  git -C "$wt" ls-files -z -- '.arch' '*/.arch' | (cd "$wt" && xargs -0 rm -f)
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
