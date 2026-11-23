//! The npm package is published by trusted publishing (an OIDC id-token), never a stored token,
//! and `release.yml` keeps the hand fix for dist 0.34's broken local `uses:` path.

use std::fs;
use std::path::Path;

fn read(path: &str) -> std::io::Result<String> {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
}

#[test]
fn release_calls_the_local_publish_workflow_with_an_id_token() {
    let release = read(".github/workflows/release.yml").unwrap();
    assert!(
        release.contains("uses: ./.github/workflows/publish-npm.yml"),
        "release.yml must call ./.github/workflows/publish-npm.yml; after `dist generate`, fix \
         dist's `uses: $/.github/...` line by hand"
    );
    assert!(
        !release.contains("uses: $/"),
        "dist's broken `uses: $/` path is back"
    );
    assert!(
        release.contains("\"id-token\": \"write\""),
        "the publish job needs id-token: write"
    );
}

#[test]
fn no_workflow_uses_an_npm_token() {
    for workflow in [
        ".github/workflows/release.yml",
        ".github/workflows/publish-npm.yml",
    ] {
        let text = read(workflow).unwrap();
        assert!(
            !text.contains("NPM_TOKEN"),
            "{workflow} must not use NPM_TOKEN"
        );
        assert!(
            !text.contains("NODE_AUTH_TOKEN"),
            "{workflow} must not use NODE_AUTH_TOKEN"
        );
    }
}

#[test]
fn publishing_uses_an_npm_new_enough_for_trusted_publishing() {
    let publish = read(".github/workflows/publish-npm.yml").unwrap();
    assert!(publish.contains("id-token: write"));
    assert!(publish.contains("npm install --global npm@latest"));
}
