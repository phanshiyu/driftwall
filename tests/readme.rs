//! The README showcases the real contract, so it must be a verbatim copy of `architecture.mmd`.

use std::fs;
use std::path::Path;

#[test]
fn readme_embeds_the_current_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let contract = fs::read_to_string(root.join("architecture.mmd")).unwrap();
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let (_, after_start) = readme.split_once("<!-- architecture:start -->").unwrap();
    let (block, _) = after_start.split_once("<!-- architecture:end -->").unwrap();
    let embedded = block
        .trim()
        .trim_start_matches("```mermaid")
        .trim_end_matches("```")
        .trim();
    assert_eq!(
        embedded,
        contract.trim(),
        "README diagram is out of date: run scripts/sync-readme.py"
    );
}
