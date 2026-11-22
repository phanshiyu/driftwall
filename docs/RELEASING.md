# Releasing

Releases are built by [cargo-dist](https://axodotdev.github.io/cargo-dist/) from
`dist-workspace.toml`. Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`, which builds
macOS and Linux binaries (arm64 and x86_64), creates a GitHub release, and publishes the npm package
`driftwall` (a thin installer that downloads the matching binary).

## Prerequisites, once

1. The repository must be public. The npm package downloads binaries from the GitHub release, and a
   private repository's release assets are not downloadable by npm users.
2. Add an `NPM_TOKEN` repository secret (an npm automation token with publish rights).
3. Confirm the name is still free: `npm view driftwall` should 404 before the first publish.

## Each release

1. Update `CHANGELOG.md`, set `version` in `Cargo.toml`, and let `scripts/gate.sh` pass.
2. Preview: `dist plan` lists every artifact the tag would produce.
3. Commit, then `git tag vX.Y.Z && git push origin main vX.Y.Z`.

Windows is not a release target yet: path handling has only been exercised on Unix.
`.github/workflows/release.yml` is generated. Change `dist-workspace.toml` and run `dist generate`,
never edit the workflow by hand.
