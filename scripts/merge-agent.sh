#!/usr/bin/env bash
# Usage: scripts/merge-agent.sh <branch> [worktree-path]
# Rebases an agent branch onto main, fast-forwards main, runs the gate, pushes, and only then
# removes the worktree and branch. Any failure stops before anything is deleted.
set -euo pipefail
branch=$1
worktree=${2:-}
cd "$(git rev-parse --show-toplevel)"
[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "not on main" >&2; exit 1; }
if [ -n "$worktree" ]; then
  git -C "$worktree" rebase main
else
  git rebase main "$branch"
  git checkout -q main
fi
git merge --ff-only "$branch"
scripts/gate.sh
git push -q origin main
if [ -n "$worktree" ]; then git worktree remove --force "$worktree"; fi
git branch -D "$branch"
git worktree prune
echo "merged: $branch"
