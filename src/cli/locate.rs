//! `driftwall where`: what a file is, which node owns it, and which `.arch` file decided it.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use driftwall::lint;
use driftwall::model::{DEFAULT_CONTRACT, PackageUse};
use driftwall::ownership::{self, FileKind, Located, Owner, Rule};
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
    let has_contract = parsed.is_some();
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
            owner: Owner::Unowned,
        };
    }
    let shown = slash_path(&args.file);
    let uses = package_uses(args, &file, contract, has_contract, &located)?;
    let stdout = match args.format {
        WhereFormat::Text => render_text_with(&shown, &located, &uses),
        WhereFormat::Json => render_json_with(&shown, &located, &uses)?,
    };
    Ok(Run {
        stdout,
        outcome: Outcome::of_where(&located),
    })
}

/// The package uses of `file`, a source file, which only a scan of the whole repository can tell:
/// none for any other kind of file and when there is no valid contract to scan against.
fn package_uses(
    args: &WhereArgs,
    file: &Path,
    contract: &Path,
    has_contract: bool,
    located: &Located,
) -> Result<Vec<PackageUse>> {
    if !has_contract || located.kind != FileKind::Source {
        return Ok(Vec::new());
    }
    let options = lint::LintOptions {
        contract: contract.to_path_buf(),
    };
    let observation = lint::observe(&args.root, &options)?;
    Ok(observation
        .package_uses
        .get(file)
        .cloned()
        .unwrap_or_default())
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
    let owner = match &located.owner {
        Owner::Owned { node, rule } => Some(format!("{node} (via {})", via(rule))),
        Owner::Unowned | Owner::Folded { .. } => None,
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
        (FileKind::Folded, _) => match &located.owner {
            Owner::Folded { rule } => format!("folded, no node (via {})", via(rule)),
            Owner::Unowned | Owner::Owned { .. } => "folded, no node".to_owned(),
        },
    };
    format!("{file}: {answer}\n")
}

/// [`render_text`], then one line per package use of the file: `  package lit (third-party) line 3`,
/// with ` (test)` when the use is in test code.
fn render_text_with(file: &str, located: &Located, uses: &[PackageUse]) -> String {
    let mut out = render_text(file, located);
    for used in uses {
        let test = if used.test_only { " (test)" } else { "" };
        // why: fmt::Write into a String is infallible, so the discarded Result carries no error.
        let _ = writeln!(
            out,
            "  package {} ({}) line {}{test}",
            used.name,
            used.kind.as_str(),
            used.at.line
        );
    }
    out
}

/// The file that decided the owner, with the line and glob when a `%% map` did.
fn via(rule: &Rule) -> String {
    let file = slash_path(rule.file());
    match rule.site() {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    owned_by: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    packages: Vec<PackageAnswer<'a>>,
}

/// One package use of the file, for `where --format json`.
#[derive(Debug, Serialize)]
struct PackageAnswer<'a> {
    line: u32,
    name: &'a str,
    kind: &'static str,
    test_only: bool,
}

/// The `%% map` line and glob that decided the owner.
#[derive(Debug, Serialize)]
struct MapAnswer<'a> {
    line: u32,
    glob: &'a str,
}

fn render_json_with(file: &str, located: &Located, uses: &[PackageUse]) -> Result<String> {
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
        node: located.owner.node().map(driftwall::model::NodeId::as_str),
        arch_file: located.owner.arch_file().map(slash_path),
        kind,
        detail,
        map: located.owner.site().map(|site| MapAnswer {
            line: site.line,
            glob: &site.glob,
        }),
        owned_by: located.owner.owned_by(),
        packages: uses
            .iter()
            .map(|used| PackageAnswer {
                line: used.at.line,
                name: &used.name,
                kind: used.kind.as_str(),
                test_only: used.test_only,
            })
            .collect(),
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
    use driftwall::model::{Exclusion, MapSite, NodeId};

    use super::*;

    fn render_json(file: &str, located: &Located) -> Result<String> {
        render_json_with(file, located, &[])
    }

    fn located(kind: FileKind, owned: bool) -> Located {
        Located {
            kind,
            owner: if owned {
                Owner::Owned {
                    node: NodeId::new("lint").unwrap(),
                    rule: Rule::Tag {
                        arch_file: PathBuf::from("src/lint/.arch"),
                    },
                }
            } else {
                Owner::Unowned
            },
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
            owner: Owner::Folded {
                rule: Rule::Tag {
                    arch_file: PathBuf::from("src/.arch"),
                },
            },
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
            "{\n  \"file\": \"a.ts\",\n  \"node\": \"lint\",\n  \"arch_file\": \"src/lint/.arch\",\n  \"kind\": \"source\",\n  \"owned_by\": \"tag\"\n}\n"
        );
    }

    fn mapped() -> Located {
        Located {
            kind: FileKind::Source,
            owner: Owner::Owned {
                node: NodeId::new("lint").unwrap(),
                rule: Rule::Map {
                    contract: PathBuf::from("c.mmd"),
                    site: MapSite {
                        line: 7,
                        glob: "src/".to_owned(),
                    },
                },
            },
        }
    }

    #[test]
    fn json_of_a_map_owner_keeps_its_keys_and_adds_owned_by() {
        let json = render_json("src/a.ts", &mapped()).unwrap();
        assert!(
            json.contains("\"node\": \"lint\",\n  \"arch_file\": \"c.mmd\","),
            "{json}"
        );
        assert!(
            json.contains("\"map\": {\n    \"line\": 7,\n    \"glob\": \"src/\"\n  },\n  \"owned_by\": \"map\""),
            "{json}"
        );
    }

    #[test]
    fn json_has_no_owned_by_for_an_unowned_or_folded_file() {
        let unowned = render_json("x.ts", &located(FileKind::Source, false)).unwrap();
        assert!(!unowned.contains("owned_by"), "{unowned}");
        let folded = Located {
            kind: FileKind::Folded,
            owner: Owner::Folded {
                rule: Rule::Tag {
                    arch_file: PathBuf::from("src/.arch"),
                },
            },
        };
        let json = render_json("src/lib.rs", &folded).unwrap();
        assert!(!json.contains("owned_by"), "{json}");
        assert!(json.contains("\"arch_file\": \"src/.arch\""), "{json}");
    }

    #[test]
    fn text_of_a_map_owner_names_the_line_and_glob() {
        assert_eq!(
            render_text("src/a.ts", &mapped()),
            "src/a.ts: lint (via c.mmd:7 (map src/))\n"
        );
    }

    #[test]
    fn json_of_a_map_owner_keeps_the_old_keys_and_adds_owned_by() {
        let mapped = Located {
            kind: FileKind::Source,
            owner: Owner::Owned {
                node: NodeId::new("lint").unwrap(),
                rule: Rule::Map {
                    contract: PathBuf::from("architecture.mmd"),
                    site: MapSite {
                        line: 7,
                        glob: "src/lint/".to_owned(),
                    },
                },
            },
        };
        assert_eq!(
            render_json("a.ts", &mapped).unwrap(),
            "{\n  \"file\": \"a.ts\",\n  \"node\": \"lint\",\n  \"arch_file\": \"architecture.mmd\",\n  \"kind\": \"source\",\n  \"map\": {\n    \"line\": 7,\n    \"glob\": \"src/lint/\"\n  },\n  \"owned_by\": \"map\"\n}\n"
        );
        assert_eq!(
            render_text("a.ts", &mapped),
            "a.ts: lint (via architecture.mmd:7 (map src/lint/))\n"
        );
    }

    #[test]
    fn json_omits_owned_by_when_unowned_or_folded() {
        let unowned = render_json("a.ts", &located(FileKind::Source, false)).unwrap();
        assert!(!unowned.contains("owned_by"), "{unowned}");
        let folded = Located {
            kind: FileKind::Folded,
            owner: Owner::Folded {
                rule: Rule::Tag {
                    arch_file: PathBuf::from("src/.arch"),
                },
            },
        };
        let json = render_json("src/lib.rs", &folded).unwrap();
        assert!(!json.contains("owned_by"), "{json}");
        assert!(json.contains("\"arch_file\": \"src/.arch\""), "{json}");
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
