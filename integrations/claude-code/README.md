# Claude Code integration

Copy into a repository that has a contract (`architecture.mmd` or `docs/architecture.mmd`, or any
path named by the `DRIFTWALL_CONTRACT` environment variable):

```
mkdir -p .claude/hooks .claude/skills
cp hooks/session-start.sh .claude/hooks/driftwall-session-start.sh
cp hooks/stop-lint.sh .claude/hooks/driftwall-stop-lint.sh
cp -r skills/driftwall .claude/skills/
```

Then merge `settings.snippet.json` into `.claude/settings.json`. The hooks find `driftwall` on PATH or
in the project's `node_modules/.bin` (an npm devDependency). A repository with no contract is
left alone; one with a contract but no `driftwall` binary gets a note on stderr instead of silence.

- SessionStart injects the contract into context, so reading it is not optional.
- Stop runs `driftwall lint . --contract <contract>` and blocks the agent from finishing while it fails.
- The skill holds only what a tool cannot decide.
