#!/usr/bin/env python3
"""PreToolUse guard. Reads the hook JSON on stdin, exits 2 with a reason to block a call."""
import json
import re
import sys

event = json.load(sys.stdin)
tool = event.get("tool_name", "")
args = event.get("tool_input", {})
cwd = event.get("cwd", "")
in_agent_worktree = "/.claude/worktrees/" in cwd

ALWAYS_BLOCKED_BASH = [
    (r"\b(pkill|killall)\b", "pkill/killall kill other agents' processes; kill a recorded PID instead"),
    (r"--no-verify|-n\b.*\bgit commit|git commit\b.*\s-n\b", "hooks are mandatory; --no-verify is not allowed"),
    (r"core\.hooksPath", "do not change core.hooksPath"),
]
AGENT_BLOCKED_BASH = [
    (r"\bgit\s+push\b", "agents do not push; report your branch and the owner merges it"),
    (r"\brm\b[^|;&]*[*?]", "rm with a glob is not allowed; delete exact paths you created"),
]
PROTECTED = [
    "architecture.mmd", "clippy.toml", "rustfmt.toml", "rust-toolchain.toml", "deny.toml",
    "renovate.json", ".githooks/", "scripts/", ".claude/settings", ".github/",
]


def block(reason):
    print(f"blocked by driftwall guard: {reason}", file=sys.stderr)
    sys.exit(2)


if tool == "Bash":
    command = args.get("command", "")
    for pattern, reason in ALWAYS_BLOCKED_BASH:
        if re.search(pattern, command):
            block(reason)
    if in_agent_worktree:
        for pattern, reason in AGENT_BLOCKED_BASH:
            if re.search(pattern, command):
                block(reason)
        if re.search(r"(>|tee|sed\s+-i|mv|cp)\b[^|;&]*(" + "|".join(re.escape(p) for p in PROTECTED) + ")", command):
            block("protected file: it is owner-only, report that you need a change")

if tool in ("Edit", "Write", "NotebookEdit") and in_agent_worktree:
    path = args.get("file_path", "")
    relative = re.sub(r"^.*?/\.claude/worktrees/[^/]+/", "", path)
    if any(relative == p or relative.startswith(p) for p in PROTECTED):
        block(f"{path} is owner-only; report the change you need instead of making it")
