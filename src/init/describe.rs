//! What the code says each node is for: the first sentence of a module doc, a docstring, a `JSDoc`
//! block or a manifest description. See "The draft reads as a design" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::units::Unit;
use crate::code::{DirTag, Language, Scope, Tag};
use crate::contract::NodeId;

/// The longest description, in characters.
const MAX_CHARS: usize = 60;
/// File names, in order of preference, that stand for the directory they sit in.
const ENTRY_STEMS: [&str; 5] = ["mod", "lib", "main", "__init__", "index"];
/// `JSDoc` tags whose text is the description of the whole file.
const FILE_TAGS: [&str; 4] = ["@file", "@module", "@fileoverview", "@fileOverview"];
/// `JSDoc` tags of a block that is a license header, not a description.
const LICENSE_TAGS: [&str; 3] = ["@license", "@copyright", "@preserve"];

/// The description of every node of `owners` that the code states one for.
///
/// `files` are the source files with their language, `owners` the owner of each file, `tags` every
/// tag (existing and planned) and `units` the code units, whose manifests describe a node that
/// stands for the whole unit directory.
pub(super) fn describe(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
    owners: &BTreeMap<PathBuf, NodeId>,
    tags: &BTreeMap<PathBuf, DirTag>,
    units: &[Unit],
) -> BTreeMap<NodeId, String> {
    let mut owned: BTreeMap<&NodeId, Vec<&Path>> = BTreeMap::new();
    for (file, id) in owners {
        if files.contains_key(file) {
            owned.entry(id).or_default().push(file);
        }
    }
    let unit_dirs: BTreeSet<&Path> = units.iter().map(|unit| unit.dir.as_path()).collect();
    let mut found = BTreeMap::new();
    for (id, members) in owned {
        let from_code = entry_file(&members).and_then(|file| {
            let text = fs::read_to_string(root.join(file)).ok()?;
            match files.get(file)? {
                Language::Rust => rust_doc(&text),
                Language::Python => python_doc(&text),
                Language::TypeScript | Language::JavaScript => js_doc(&text),
                _ => None,
            }
        });
        if let Some(text) = from_code {
            found.insert(id.clone(), text);
        }
    }
    for (dir, found_tag) in tags {
        let Tag::Node(id) = &found_tag.tag else {
            continue;
        };
        if found_tag.scope == Scope::Subtree
            && unit_dirs.contains(dir.as_path())
            && !found.contains_key(id)
            && owners.values().any(|owner| owner == id)
            && let Some(text) = manifest_description(root, dir)
        {
            found.insert(id.clone(), text);
        }
    }
    found
}

/// The file that stands for a node: among its files at the smallest depth, the one named like an
/// entry (`mod`, `lib`, `main`, `__init__`, `index`, in that order), else the only module they make.
fn entry_file<'a>(files: &[&'a Path]) -> Option<&'a Path> {
    let depth = |file: &Path| file.components().count();
    let shallowest = files.iter().map(|file| depth(file)).min()?;
    let top: Vec<&Path> = files
        .iter()
        .copied()
        .filter(|file| depth(file) == shallowest)
        .collect();
    let stem = |file: &Path| -> String {
        file.file_name()
            .map(|name| {
                name.to_string_lossy()
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_owned()
            })
            .unwrap_or_default()
    };
    let entry = top
        .iter()
        .filter_map(|file| {
            let rank = ENTRY_STEMS.iter().position(|name| *name == stem(file))?;
            Some((rank, file.as_os_str().len(), *file))
        })
        .min();
    if let Some((_, _, file)) = entry {
        return Some(file);
    }
    let stems: BTreeSet<String> = top.iter().map(|file| stem(file)).collect();
    if stems.len() == 1 {
        return top
            .iter()
            .copied()
            .min_by_key(|file| (file.as_os_str().len(), *file));
    }
    None
}

/// The first sentence of the leading `//!` doc of a Rust file.
pub(super) fn rust_doc(text: &str) -> Option<String> {
    let mut doc: Vec<&str> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("//!") {
            doc.push(rest.trim());
        } else if !doc.is_empty()
            || !(line.is_empty() || line.starts_with("#!") || line.starts_with("//"))
        {
            break;
        }
    }
    sentence(&paragraph(doc.into_iter()))
}

/// The first sentence of the first line of the module docstring of a Python file.
pub(super) fn python_doc(text: &str) -> Option<String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .skip_while(|line| line.is_empty() || line.starts_with('#'));
    let first = lines.next()?;
    let unprefixed = first.trim_start_matches(['r', 'R', 'u', 'U']);
    let quote = ["\"\"\"", "'''", "\"", "'"]
        .into_iter()
        .find(|quote| unprefixed.starts_with(quote))?;
    let rest = unprefixed.strip_prefix(quote).unwrap_or("");
    let body = if rest.trim().is_empty() {
        lines.find(|line| !line.is_empty()).unwrap_or("")
    } else {
        rest
    };
    let body = body.split(quote).next().unwrap_or("");
    sentence(body)
}

/// The first sentence of the leading `JSDoc` block of a TypeScript or JavaScript file: its `@file`,
/// `@module` or `@fileoverview` text, else its first line of text.
pub(super) fn js_doc(text: &str) -> Option<String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .skip_while(|line| line.is_empty() || line.starts_with("#!"));
    let first = lines.next()?.strip_prefix("/**")?;
    let mut block: Vec<String> = Vec::new();
    let mut current = first;
    loop {
        let (content, closed) = match current.split_once("*/") {
            Some((before, _)) => (before, true),
            None => (current, false),
        };
        let content = content.trim().trim_start_matches('*').trim();
        block.push(content.to_owned());
        if closed {
            break;
        }
        current = lines.next()?;
    }
    if block
        .iter()
        .any(|line| LICENSE_TAGS.iter().any(|tag| line.starts_with(tag)))
    {
        return None;
    }
    let tagged = block.iter().position(|line| {
        FILE_TAGS.iter().any(|tag| {
            line.strip_prefix(tag)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
    });
    let from = tagged.unwrap_or(0);
    let body = block.iter().skip(from).enumerate().map(|(index, line)| {
        if index == 0 && tagged.is_some() {
            line.split_once(' ').map_or("", |(_, rest)| rest)
        } else {
            line.as_str()
        }
    });
    let text = paragraph(
        body.take_while(|line| !line.starts_with('@'))
            .skip_while(|line| line.is_empty()),
    );
    sentence(&text)
}

/// The lines up to the first blank one, joined with spaces.
fn paragraph<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    lines
        .take_while(|line| !line.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` cut after its first sentence, cleaned for a label and trimmed to [`MAX_CHARS`].
pub(super) fn sentence(text: &str) -> Option<String> {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = flat
        .match_indices(['.', '!', '?'])
        .map(|(at, _)| at)
        .find(|at| {
            flat.get(at + 1..)
                .is_none_or(|rest| rest.is_empty() || rest.starts_with(' '))
        })
        .and_then(|at| flat.get(..at))
        .unwrap_or(&flat);
    let clean: String = cut
        .chars()
        .filter(|c| !matches!(c, '`' | '[' | ']'))
        .map(|c| if c == '"' { '\'' } else { c })
        .collect();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| trim_to_words(clean))
}

/// `text` shortened to [`MAX_CHARS`] characters at a word boundary, with `…` when cut.
fn trim_to_words(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_owned();
    }
    let head: String = text.chars().take(MAX_CHARS).collect();
    let at_boundary = text.chars().nth(MAX_CHARS).is_some_and(char::is_whitespace);
    let kept = if at_boundary {
        head.as_str()
    } else {
        head.rsplit_once(' ')
            .map_or(head.as_str(), |(before, _)| before)
    };
    let kept = kept.trim_end_matches(|c: char| c.is_whitespace() || ",;:-(".contains(c));
    format!("{kept}…")
}

/// The `description` of the manifest in `dir`: `Cargo.toml` `[package]`, `package.json`, or
/// `pyproject.toml`, the first that has one.
fn manifest_description(root: &Path, dir: &Path) -> Option<String> {
    let read = |name: &str| fs::read_to_string(root.join(dir).join(name)).ok();
    let from_cargo = || {
        let manifest: toml::Value = toml::from_str(&read("Cargo.toml")?).ok()?;
        manifest
            .get("package")?
            .get("description")?
            .as_str()
            .and_then(sentence)
    };
    let from_package = || {
        let manifest: serde_json::Value = serde_json::from_str(&read("package.json")?).ok()?;
        manifest.get("description")?.as_str().and_then(sentence)
    };
    let from_pyproject = || {
        let manifest: toml::Value = toml::from_str(&read("pyproject.toml")?).ok()?;
        let project = manifest
            .get("project")
            .and_then(|table| table.get("description"));
        let poetry = || manifest.get("tool")?.get("poetry")?.get("description");
        project.or_else(poetry)?.as_str().and_then(sentence)
    };
    from_cargo().or_else(from_package).or_else(from_pyproject)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_takes_the_first_sentence_of_the_inner_doc() {
        let text = "// license\n#![forbid(unsafe_code)]\n//! Parses and renders the contract.\n//!\n//! More.\nuse a;\n";
        assert_eq!(
            rust_doc(text).as_deref(),
            Some("Parses and renders the contract")
        );
        assert_eq!(rust_doc("use a;\n//! late\n"), None);
        assert_eq!(
            rust_doc("//! Reads the `scan` of\n//! [`Files`] first.\n").as_deref(),
            Some("Reads the scan of Files first")
        );
    }

    #[test]
    fn python_takes_the_first_line_of_the_docstring() {
        assert_eq!(
            python_doc(
                "#!/usr/bin/env python\n\"\"\"Walks the tree.\n\nLong text.\"\"\"\nimport os\n"
            )
            .as_deref(),
            Some("Walks the tree")
        );
        assert_eq!(
            python_doc("'''\n  Sends mail.\n'''\n").as_deref(),
            Some("Sends mail")
        );
        assert_eq!(python_doc("import os\n\"\"\"late\"\"\"\n"), None);
    }

    #[test]
    fn js_prefers_a_file_tag_then_the_first_text_line() {
        let tagged =
            "/**\n * Helpers.\n * @file Talks to the GitLab API.\n */\nimport x from 'y';\n";
        assert_eq!(js_doc(tagged).as_deref(), Some("Talks to the GitLab API"));
        let plain =
            "/**\n * Formats dates for the list.\n * @param a\n */\nexport function f() {}\n";
        assert_eq!(js_doc(plain).as_deref(), Some("Formats dates for the list"));
        assert_eq!(js_doc("/** One line. */\n").as_deref(), Some("One line"));
        assert_eq!(js_doc("/**\n * @license MIT\n */\n"), None);
        assert_eq!(js_doc("// not a block\n/** later */\n"), None);
    }

    #[test]
    fn long_text_is_cut_at_a_word_and_quotes_become_apostrophes() {
        let long = "Reads every manifest and import statement of the repository to build the graph";
        let cut = sentence(long).unwrap();
        assert_eq!(
            cut,
            "Reads every manifest and import statement of the repository…"
        );
        assert!(cut.chars().count() <= MAX_CHARS + 1);
        assert_eq!(sentence("Say \"hi\" now").as_deref(), Some("Say 'hi' now"));
        assert_eq!(sentence("   "), None);
    }

    #[test]
    fn a_manifest_describes_the_unit_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let write = |name: &str, text: &str| fs::write(dir.path().join(name), text).unwrap();
        let read = || manifest_description(dir.path(), Path::new(""));
        assert_eq!(read(), None);
        write(
            "pyproject.toml",
            "[project]\ndescription = \"From python. More.\"\n",
        );
        assert_eq!(read().as_deref(), Some("From python"));
        write("package.json", "{\"description\": \"From npm\"}");
        assert_eq!(read().as_deref(), Some("From npm"));
        write("Cargo.toml", "[package]\ndescription = \"From cargo\"\n");
        assert_eq!(read().as_deref(), Some("From cargo"));
        write("Cargo.toml", "[package]\ndescription.workspace = true\n");
        assert_eq!(read().as_deref(), Some("From npm"));
    }

    #[test]
    fn the_entry_file_is_the_entry_name_or_the_only_module() {
        let pick = |files: &[&str]| {
            let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
            let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            entry_file(&refs).map(|file| file.display().to_string())
        };
        assert_eq!(
            pick(&["src/a/mod.rs", "src/a/x.rs", "src/a/deep/lib.rs"]).as_deref(),
            Some("src/a/mod.rs")
        );
        assert_eq!(
            pick(&["src/a.ts", "src/a.test.ts", "src/a/b.ts"]).as_deref(),
            Some("src/a.ts")
        );
        assert_eq!(pick(&["src/a.ts", "src/b.ts"]), None);
        assert_eq!(
            pick(&["x/index.ts", "x/index.test.ts"]).as_deref(),
            Some("x/index.ts")
        );
    }
}
