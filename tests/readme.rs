//! The README showcases the real contract and the real CLI, so it must not drift from either.

use std::fs;
use std::path::Path;
use std::process::Command;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn readme() -> String {
    fs::read_to_string(root().join("README.md")).unwrap_or_default()
}

#[test]
fn readme_embeds_the_current_contract() {
    let contract = fs::read_to_string(root().join("architecture.mmd")).unwrap();
    let readme = readme();
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

/// Help text of the built binary for `args`, or `None` when it exits non-zero.
fn help(args: &[&str]) -> Option<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_driftwall"))
        .args(args)
        .arg("--help")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether `help` documents `flag` as an option of its own (not a prefix of a longer one).
fn documents(help: &str, flag: &str) -> bool {
    help.split(flag).collect::<Vec<_>>().windows(2).any(|pair| {
        let before = pair.first().and_then(|text| text.chars().next_back());
        let after = pair.get(1).and_then(|text| text.chars().next());
        before.is_some_and(|c| c == ' ' || c == ',')
            && after.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-'))
    })
}

/// The `driftwall ...` command lines of every fenced block that is not Mermaid.
fn command_lines(readme: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut fence: Option<bool> = None;
    for line in readme.lines() {
        if line.starts_with("```") {
            fence = match fence {
                Some(_) => None,
                None => Some(line.trim_start_matches('`') != "mermaid"),
            };
            continue;
        }
        if fence == Some(true) {
            let command = line.trim().trim_start_matches("$ ");
            let command = command.split('#').next().unwrap_or(command).trim();
            if command.split_whitespace().next() == Some("driftwall") {
                lines.push(command.to_string());
            }
        }
    }
    lines
}

#[test]
fn readme_commands_exist_in_the_cli() {
    let readme = readme();
    let lines = command_lines(&readme);
    assert!(!lines.is_empty(), "README shows no `driftwall` command");
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().skip(1).collect();
        let args: Vec<&str> = words
            .first()
            .filter(|word| !word.starts_with('-'))
            .copied()
            .into_iter()
            .collect();
        let help_text = help(&args)
            .unwrap_or_else(|| panic!("README runs `{line}` but the CLI has no such subcommand"));
        for word in &words {
            if let Some(flag) = word.strip_prefix("--") {
                let flag = format!("--{}", flag.split('=').next().unwrap_or(flag));
                assert!(
                    documents(&help_text, &flag),
                    "README runs `{line}` but the CLI has no `{flag}` there"
                );
            }
        }
    }
}

/// GitHub's heading anchor: lowercase, drop punctuation except `-`, spaces to `-`.
fn anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

#[test]
fn readme_links_resolve() {
    let readme = readme();
    let design = fs::read_to_string(root().join("docs/DESIGN.md")).unwrap();
    let anchors: Vec<String> = design
        .lines()
        .filter(|line| line.starts_with('#'))
        .map(|line| anchor(line.trim_start_matches('#')))
        .collect();
    for part in readme.split("](").skip(1) {
        let target = part.split(')').next().unwrap_or("");
        if target.starts_with("http") {
            continue;
        }
        let (path, fragment) = target.split_once('#').unwrap_or((target, ""));
        assert!(root().join(path).exists(), "README links to missing {path}");
        if path == "docs/DESIGN.md" && !fragment.is_empty() {
            assert!(
                anchors.iter().any(|a| a == fragment),
                "README links to missing DESIGN.md heading #{fragment}"
            );
        }
    }
}
