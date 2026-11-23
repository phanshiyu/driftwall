//! `driftwall where`: what a file is, which node owns it, and which `.arch` file decided it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use driftwall::lint;
use driftwall::model::DEFAULT_CONTRACT;
use driftwall::ownership::{self, FileKind, Located};
use serde::Serialize;

use crate::args::{WhereArgs, WhereFormat};
use crate::commands::{Run, require_directory};
use crate::outcome::Outcome;

/// Locates `args.file` under `args.root`.
///
/// # Errors
/// A root that is not a directory, a contract given with `--contract` that is unreadable or
/// invalid, a path outside the root or that does not exist, or an unreadable or invalid `.arch`.
pub fn run(args: &WhereArgs) -> Result<Run> {
    require_directory(&args.root)?;
    let contract = args
        .contract
        .as_deref()
        .unwrap_or_else(|| Path::new(DEFAULT_CONTRACT));
    // why: a given contract must be valid; the default one only matters when it holds `%% map` lines.
    let parsed = if args.contract.is_some() {
        Some(lint::read_contract(&args.root, contract)?)
    } else {
        lint::read_contract(&args.root, contract).ok()
    };
    let rules = parsed.map(|parsed| parsed.maps).unwrap_or_default();
    let file = inside_root(&args.root, &args.file);
    let mut located = if rules.is_empty() {
        ownership::locate(&args.root, &file)?
    } else {
        let context = ownership::MapContext {
            contract,
            rules: &rules,
        };
        ownership::locate_mapped(&args.root, &file, context)?
    };
    // why: only the command knows the contract path, so it names the file; the contract is no asset.
    if located.kind != FileKind::Directory && slash_path(&file) == slash_path(contract) {
        located = Located {
            kind: FileKind::Contract,
            node: None,
            arch_file: None,
            map: None,
        };
    }
    let shown = slash_path(&args.file);
    let stdout = match args.format {
        WhereFormat::Text => render_text(&shown, &located),
        WhereFormat::Json => render_json(&shown, &located)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::of_where(&located),
    })
}

/// Turns an absolute `file` below `root` into the root-relative path `locate` accepts.
///
/// Both are compared as given first, then with symlinks resolved (a temp dir like `/var/..` may
/// really be `/private/var/..`). Anything that still does not fit is returned unchanged so
/// `locate` reports it as outside the root.
fn inside_root(root: &Path, file: &Path) -> PathBuf {
    if !file.is_absolute() {
        return file.to_path_buf();
    }
    if let Ok(relative) = file.strip_prefix(root) {
        return relative.to_path_buf();
    }
    let resolved = fs::canonicalize(root).ok().zip(resolve(file));
    match resolved {
        Some((real_root, real_file)) => real_file
            .strip_prefix(&real_root)
            .map_or_else(|_| file.to_path_buf(), Path::to_path_buf),
        None => file.to_path_buf(),
    }
}

/// Resolves symlinks in `file`, or in its directory when the file itself does not exist.
fn resolve(file: &Path) -> Option<PathBuf> {
    fs::canonicalize(file).ok().or_else(|| {
        let parent = fs::canonicalize(file.parent()?).ok()?;
        Some(parent.join(file.file_name()?))
    })
}

/// `<file>: <answer>`, see "CLI surface" in `docs/DESIGN.md` for the answers.
fn render_text(file: &str, located: &Located) -> String {
    let owner = match (&located.node, &located.arch_file) {
        (Some(node), Some(_)) => Some(format!("{node} (via {})", via(located))),
        _ => None,
    };
    let answer = match (&located.kind, owner) {
        (FileKind::Source | FileKind::Directory, Some(owner)) => owner,
        (FileKind::Source | FileKind::Directory, None) => "unmapped".to_owned(),
        (FileKind::Asset, Some(owner)) => format!("asset of node {owner}"),
        (FileKind::Asset, None) => "asset, unmapped".to_owned(),
        (FileKind::Unread(key), Some(owner)) => format!("not read ({key}) in node {owner}"),
        (FileKind::Unread(key), None) => format!("not read ({key}), unmapped"),
        (FileKind::Excluded(reason), Some(owner)) => {
            format!("excluded ({}), owned by {owner}", reason.as_str())
        }
        (FileKind::Excluded(reason), None) => format!("excluded ({})", reason.as_str()),
        (FileKind::Ignored, Some(owner)) => {
            format!("ignored by .driftwallignore, owned by {owner}")
        }
        (FileKind::Ignored, None) => "ignored by .driftwallignore".to_owned(),
        (FileKind::TagFile, _) => "tag file".to_owned(),
        (FileKind::Contract, _) => "contract".to_owned(),
        (FileKind::Folded, _) => located.arch_file.as_deref().map_or_else(
            || "folded, no node".to_owned(),
            |_| format!("folded, no node (via {})", via(located)),
        ),
    };
    format!("{file}: {answer}\n")
}

/// The file that decided the owner, with the line and glob when a `%% map` did.
fn via(located: &Located) -> String {
    let file = located
        .arch_file
        .as_deref()
        .map(slash_path)
        .unwrap_or_default();
    match &located.map {
        Some(site) => format!("{file}:{} (map {})", site.line, site.glob),
        None => file,
    }
}

/// The JSON document of `where --format json`.
#[derive(Debug, Serialize)]
struct Answer<'a> {
    file: &'a str,
    node: Option<&'a str>,
    arch_file: Option<String>,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    map: Option<MapAnswer<'a>>,
}

/// The `%% map` line and glob that decided the owner.
#[derive(Debug, Serialize)]
struct MapAnswer<'a> {
    line: u32,
    glob: &'a str,
}

fn render_json(file: &str, located: &Located) -> Result<String> {
    let (kind, detail) = match &located.kind {
        FileKind::Source => ("source", None),
        FileKind::Asset => ("asset", None),
        FileKind::Unread(key) => ("unread", Some(key.clone())),
        FileKind::Excluded(reason) => ("excluded", Some(reason.as_str().to_owned())),
        FileKind::Ignored => ("ignored", None),
        FileKind::Directory => ("directory", None),
        FileKind::Folded => ("folded", None),
        FileKind::TagFile => ("tag-file", None),
        FileKind::Contract => ("contract", None),
    };
    let answer = Answer {
        file,
        node: located.node.as_ref().map(driftwall::model::NodeId::as_str),
        arch_file: located.arch_file.as_deref().map(slash_path),
        kind,
        detail,
        map: located.map.as_ref().map(|site| MapAnswer {
            line: site.line,
            glob: &site.glob,
        }),
    };
    let mut out = serde_json::to_string_pretty(&answer)?;
    out.push('\n');
    Ok(out)
}

/// A path with `/` separators regardless of platform.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use driftwall::model::{Exclusion, NodeId};

    use super::*;

    fn located(kind: FileKind, owned: bool) -> Located {
        Located {
            kind,
            node: owned.then(|| NodeId::new("lint").unwrap()),
            arch_file: owned.then(|| PathBuf::from("src/lint/.arch")),
            map: None,
        }
    }

    fn text(kind: FileKind, owned: bool) -> String {
        render_text("f", &located(kind, owned))
    }

    #[test]
    fn text_names_node_and_arch_file() {
        assert_eq!(
            render_text("src/lint/mod.rs", &located(FileKind::Source, true)),
            "src/lint/mod.rs: lint (via src/lint/.arch)\n"
        );
    }

    #[test]
    fn text_says_unmapped() {
        assert_eq!(
            render_text("x.ts", &located(FileKind::Source, false)),
            "x.ts: unmapped\n"
        );
    }

    #[test]
    fn text_says_what_every_other_kind_of_file_is() {
        assert_eq!(
            text(FileKind::Asset, true),
            "f: asset of node lint (via src/lint/.arch)\n"
        );
        assert_eq!(text(FileKind::Asset, false), "f: asset, unmapped\n");
        assert_eq!(
            text(FileKind::Unread("swift".to_owned()), true),
            "f: not read (swift) in node lint (via src/lint/.arch)\n"
        );
        assert_eq!(
            text(FileKind::Unread("shebang".to_owned()), false),
            "f: not read (shebang), unmapped\n"
        );
        assert_eq!(
            text(FileKind::Excluded(Exclusion::Vendored), false),
            "f: excluded (vendored)\n"
        );
        assert_eq!(
            text(FileKind::Excluded(Exclusion::Generated), true),
            "f: excluded (generated), owned by lint (via src/lint/.arch)\n"
        );
        assert_eq!(
            text(FileKind::Ignored, false),
            "f: ignored by .driftwallignore\n"
        );
        assert_eq!(
            text(FileKind::Ignored, true),
            "f: ignored by .driftwallignore, owned by lint (via src/lint/.arch)\n"
        );
        assert_eq!(text(FileKind::TagFile, false), "f: tag file\n");
        assert_eq!(text(FileKind::Contract, false), "f: contract\n");
        let folded = Located {
            kind: FileKind::Folded,
            node: None,
            arch_file: Some(PathBuf::from("src/.arch")),
            map: None,
        };
        assert_eq!(
            render_text("src/lib.rs", &folded),
            "src/lib.rs: folded, no node (via src/.arch)\n"
        );
        assert!(
            render_json("src/lib.rs", &folded)
                .unwrap()
                .ends_with("\"kind\": \"folded\"\n}\n")
        );
        assert_eq!(
            text(FileKind::Directory, true),
            "f: lint (via src/lint/.arch)\n"
        );
    }

    #[test]
    fn json_mapped_has_all_keys_pretty_printed() {
        assert_eq!(
            render_json("a.ts", &located(FileKind::Source, true)).unwrap(),
            "{\n  \"file\": \"a.ts\",\n  \"node\": \"lint\",\n  \"arch_file\": \"src/lint/.arch\",\n  \"kind\": \"source\"\n}\n"
        );
    }

    #[test]
    fn json_unmapped_has_nulls() {
        assert_eq!(
            render_json("a.ts", &located(FileKind::Source, false)).unwrap(),
            "{\n  \"file\": \"a.ts\",\n  \"node\": null,\n  \"arch_file\": null,\n  \"kind\": \"source\"\n}\n"
        );
    }

    #[test]
    fn json_carries_the_unread_key_and_the_exclusion_reason_as_detail() {
        let json = |kind| render_json("f", &located(kind, false)).unwrap();
        assert!(
            json(FileKind::Unread("swift".to_owned()))
                .ends_with("\"kind\": \"unread\",\n  \"detail\": \"swift\"\n}\n")
        );
        assert!(
            json(FileKind::Excluded(Exclusion::Generated))
                .ends_with("\"kind\": \"excluded\",\n  \"detail\": \"generated\"\n}\n")
        );
        assert!(json(FileKind::Ignored).ends_with("\"kind\": \"ignored\"\n}\n"));
    }

    #[test]
    fn json_names_the_owner_of_an_excluded_file_and_the_tool_files() {
        let excluded = located(FileKind::Excluded(Exclusion::Generated), true);
        let json = render_json("f", &excluded).unwrap();
        assert!(json.contains("\"node\": \"lint\""), "{json}");
        assert!(json.contains("\"kind\": \"excluded\""), "{json}");
        let tag = render_json("f", &located(FileKind::TagFile, false)).unwrap();
        assert!(tag.contains("\"kind\": \"tag-file\""), "{tag}");
        let contract = render_json("f", &located(FileKind::Contract, false)).unwrap();
        assert!(contract.contains("\"kind\": \"contract\""), "{contract}");
    }

    #[test]
    fn relative_files_pass_through() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("a/b.ts")),
            Path::new("a/b.ts")
        );
    }

    #[test]
    fn absolute_files_below_the_root_become_relative() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("/r/a/b.ts")),
            Path::new("a/b.ts")
        );
    }

    #[test]
    fn absolute_files_elsewhere_are_left_for_locate_to_reject() {
        assert_eq!(
            inside_root(Path::new("/r"), Path::new("/other/b.ts")),
            Path::new("/other/b.ts")
        );
    }
}
