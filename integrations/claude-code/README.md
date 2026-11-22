# Claude Code integration

Copy into a repository that has an `architecture.mmd`:

```
mkdir -p .claude/hooks .claude/skills
cp hooks/session-start.sh .claude/hooks/driftwall-session-start.sh
cp hooks/stop-lint.sh .claude/hooks/driftwall-stop-lint.sh
cp -r skills/driftwall .claude/skills/
```

Then merge `settings.snippet.json` into `.claude/settings.json`. `driftwall` must be on PATH.

- SessionStart injects the contract into context, so reading it is not optional.
- Stop runs `driftwall lint .` and blocks the agent from finishing while it fails.
- The skill holds only what a tool cannot decide.
