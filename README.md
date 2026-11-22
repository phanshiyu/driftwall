<p align="center"><img src="assets/logo.svg" width="128" alt="driftwall logo"></p>

# driftwall

Your architecture diagram is the contract. driftwall fails the build when the code drifts from it.

> Status: proof of concept under construction. This repo is built diagram-first: see
> [`architecture.mmd`](architecture.mmd) for the contract this codebase is held to, and
> [`docs/DESIGN.md`](docs/DESIGN.md) for the design.

- `architecture.mmd` declares nodes and the dependencies allowed between them.
- `.arch` files tag directories with a node.
- `driftwall lint` finds unmapped files, unknown nodes and undeclared dependencies, with file and
  line, in text, JSON or SARIF.
- Built for AI agents: a deterministic map of where code lives and what may depend on what.

## Install

Not published yet. See [docs/RELEASING.md](docs/RELEASING.md). Until then:

```
cargo install --git https://github.com/phanshiyu/driftwall
```

## Use

```
driftwall init            # baseline architecture.mmd and .arch tags from the existing code
driftwall lint            # exit 0 clean, 1 violations, 2 tool error
driftwall lint --format json
driftwall where src/foo.ts
```

Agent integration (Claude Code skill and hooks): [integrations/claude-code](integrations/claude-code).
