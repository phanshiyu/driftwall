//! Ownership decided by the `%% map` lines of the contract (see "Contract map" in
//! `docs/DESIGN.md`), instead of `.arch` files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{
    Discovered, Explanation, OwnershipError, Verdict, classify, discover, peers_without,
    relative_to_root,
};
use super::{folds, module_name, trivial_files};
use crate::model::{
    DirTag, Location, MapGlob, MapRule, MapSite, MapTarget, NodeId, Ownership, Scope, Specificity,
    Tag,
};

/// The contract's mapping: where it is, for messages, and its rules.
#[derive(Debug, Clone, Copy)]
pub struct MapContext<'a> {
    /// The contract path, relative to the repository root.
    pub contract: &'a Path,
    /// The `%% map` rules of the contract, in file order.
    pub rules: &'a [MapRule],
}

/// One glob with the rule it belongs to.
struct Entry<'a> {
    rule: &'a MapRule,
    glob: &'a MapGlob,
    specificity: Specificity,
}

impl Entry<'_> {
    fn site(&self) -> MapSite {
        MapSite {
            line: self.rule.line,
            glob: self.glob.to_string(),
        }
    }
}

/// Two globs that map one file with the same specificity to different targets.
struct Tie<'a>(&'a Entry<'a>, &'a Entry<'a>);

/// Every glob of a contract, ready to answer who owns a path.
pub struct Mapper<'a> {
    context: MapContext<'a>,
    entries: Vec<Entry<'a>>,
}

impl<'a> Mapper<'a> {
    /// Collects the globs of `context`.
    #[must_use]
    pub fn new(context: MapContext<'a>) -> Self {
        let entries = context
            .rules
            .iter()
            .flat_map(|rule| {
                rule.globs.iter().map(move |glob| Entry {
                    rule,
                    glob,
                    specificity: glob.specificity(),
                })
            })
            .collect();
        Self { context, entries }
    }

    /// The most specific entry that matches `file`, `None` when no glob does, or the two entries
    /// that tie for different targets.
    fn best(&self, file: &Path) -> Result<Option<&Entry<'a>>, Tie<'_>> {
        let mut winner: Option<&Entry<'a>> = None;
        let mut clash: Option<&Entry<'a>> = None;
        for entry in self.entries.iter().filter(|e| e.glob.matches(file)) {
            match winner {
                Some(held) if entry.specificity < held.specificity => {}
                Some(held) if entry.specificity == held.specificity => {
                    if entry.rule.target != held.rule.target {
                        clash = Some(entry);
                    }
                }
                _ => {
                    winner = Some(entry);
                    clash = None;
                }
            }
        }
        match (winner, clash) {
            (Some(first), Some(second)) => Err(Tie(first, second)),
            (found, None) => Ok(found),
            (None, Some(_)) => Ok(None),
        }
    }

    /// What `entry` says about `file`. `dirs` holds the module directories with their peers.
    fn verdict(
        &self,
        entry: &Entry<'a>,
        file: &Path,
        dirs: &BTreeMap<PathBuf, DirTag>,
        trivial: bool,
    ) -> Verdict {
        let at = Location {
            file: self.context.contract.to_path_buf(),
            line: entry.rule.line,
        };
        let site = Some(entry.site());
        match &entry.rule.target {
            MapTarget::Node(id) => Verdict {
                node: Ok(id.clone()),
                at,
                folded: false,
                derived: false,
                site,
            },
            MapTarget::Modules(prefix) => {
                let dir = entry.glob.directory().unwrap_or_default();
                let peers = dirs.get(&dir).map(|found| &found.peers);
                let empty = std::collections::BTreeSet::new();
                let peers = peers.unwrap_or(&empty);
                let name = module_name(&dir, file, peers).unwrap_or_default();
                Verdict {
                    node: prefix.derive(&name),
                    at,
                    folded: trivial && folds(&dir, file, peers),
                    derived: true,
                    site,
                }
            }
        }
    }

    /// The node `file` gets, `None` when no glob covers it. `dirs` is `Ownership::tag_dirs` of a
    /// scan made with the same context.
    #[must_use]
    pub fn owner(&self, file: &Path, dirs: &BTreeMap<PathBuf, DirTag>) -> Option<NodeId> {
        let entry = self.best(file).ok().flatten()?;
        self.verdict(entry, file, dirs, false).node.ok()
    }
}

/// Scans `root` like [`scan`](super::scan), with the owner of every file decided by the globs of
/// `context` and no `.arch` file read.
///
/// The `.arch` files that exist are listed in [`Ownership::ignored_tags`], the globs that match no
/// file in [`Ownership::stale_maps`], and the module directories of module-form rules are in
/// [`Ownership::tag_dirs`] (with their peers), the only entries there.
///
/// # Errors
/// As [`scan`](super::scan) for the walk, plus [`OwnershipError::AmbiguousMap`] when two globs tie
/// for a source file, and [`OwnershipError::InvalidModule`] at the map line of a module rule.
pub fn scan_mapped(root: &Path, context: MapContext<'_>) -> Result<Ownership, OwnershipError> {
    let Discovered {
        sources,
        assets,
        arch_files,
        excluded,
        unread,
        ignored,
    } = discover(root)?;
    let mapper = Mapper::new(context);
    let trivial = trivial_files(root, sources.iter().map(PathBuf::as_path));
    let mut dirs: BTreeMap<PathBuf, DirTag> = BTreeMap::new();
    for entry in &mapper.entries {
        if let (MapTarget::Modules(prefix), Some(dir)) =
            (&entry.rule.target, entry.glob.directory())
        {
            let peers = peers_without(&dir, &sources, &trivial);
            dirs.insert(
                dir,
                DirTag {
                    tag: Tag::Modules(prefix.clone()),
                    scope: Scope::Subtree,
                    peers,
                },
            );
        }
    }
    for source in &sources {
        if let Err(Tie(first, second)) = mapper.best(source) {
            let (first, second) = if first.rule.line <= second.rule.line {
                (first, second)
            } else {
                (second, first)
            };
            return Err(OwnershipError::AmbiguousMap {
                contract: context.contract.to_path_buf(),
                file: source.clone(),
                first: first.site(),
                second: second.site(),
            });
        }
    }
    let stale_maps = mapper
        .entries
        .iter()
        .filter(|entry| {
            !sources
                .iter()
                .chain(&assets)
                .chain(excluded.values().flatten())
                .any(|file| entry.glob.matches(file))
        })
        .map(Entry::site)
        .collect();
    let mut ownership = Ownership {
        excluded,
        unread,
        ignored,
        stale_maps,
        ignored_tags: arch_files,
        tag_dirs: dirs.clone(),
        ..Ownership::default()
    };
    let derived = classify(&mut ownership, sources, assets, |file| {
        let entry = mapper.best(file).ok().flatten()?;
        Some(mapper.verdict(entry, file, &dirs, trivial.contains(file)))
    })?;
    ownership.tags = derived.into_iter().collect();
    Ok(ownership)
}

/// What [`locate`](super::locate) needs in map mode: who owns `relative` (a path relative to the
/// root, a directory answered as a file directly in it) and the glob that decided it.
///
/// # Errors
/// As [`scan_mapped`].
pub(super) fn explain_mapped(
    root: &Path,
    file: &Path,
    is_dir: bool,
    context: MapContext<'_>,
) -> Result<(Explanation, Option<MapSite>), OwnershipError> {
    let relative = relative_to_root(root, file)?;
    let own = scan_mapped(root, context)?;
    let folded = own.folded.contains(&relative);
    // why: a file the walk never saw (git ignores it) or a directory is judged by its name, and
    // why: a directory is answered as a file directly in it.
    let probe = if is_dir {
        relative.join("_")
    } else {
        relative.clone()
    };
    let mapper = Mapper::new(context);
    let hit = mapper.best(&probe).ok().flatten();
    let node = own
        .owners
        .get(&relative)
        .or_else(|| own.asset_owners.get(&relative))
        .or_else(|| own.excluded_owners.get(&relative))
        .cloned()
        .or_else(|| {
            let entry = hit.filter(|_| !folded)?;
            mapper
                .verdict(entry, &probe, &own.tag_dirs, false)
                .node
                .ok()
        });
    let decided = node.is_some() || folded;
    Ok((
        Explanation {
            node,
            arch_file: decided.then(|| context.contract.to_path_buf()),
            folded,
        },
        hit.filter(|_| decided).map(Entry::site),
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::model::{MapGlob, Prefix};

    /// The rules of the `%% map` lines in `text`, as the contract parser reads them.
    fn rules_in(text: &str) -> Vec<MapRule> {
        let mut rules = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let Some(rest) = line.strip_prefix("%% map ") else {
                continue;
            };
            let (target, globs) = rest.split_once(':').unwrap();
            let target = match target.strip_suffix('*') {
                Some("") => MapTarget::Modules(Prefix::default()),
                Some(prefix) => MapTarget::Modules(Prefix::new(prefix).unwrap()),
                None => MapTarget::Node(NodeId::new(target).unwrap()),
            };
            rules.push(MapRule {
                line: u32::try_from(index + 1).unwrap(),
                target,
                globs: globs
                    .split(',')
                    .map(|g| MapGlob::parse(g).unwrap())
                    .collect(),
            });
        }
        rules
    }

    fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    fn scan(dir: &tempfile::TempDir, contract: &str) -> Result<Ownership, OwnershipError> {
        let rules = rules_in(contract);
        scan_mapped(
            dir.path(),
            MapContext {
                contract: Path::new("docs/architecture.mmd"),
                rules: &rules,
            },
        )
    }

    fn owner(own: &Ownership, file: &str) -> Option<String> {
        own.owners.get(Path::new(file)).map(ToString::to_string)
    }

    const HEAD: &str = "flowchart LR\n  ui\n  api\n  store\n";

    #[test]
    fn the_most_specific_glob_wins_and_unmatched_files_are_unmapped() {
        let dir = repo(&[
            ("src/ui/a.ts", ""),
            ("src/ui/deep/b.ts", ""),
            ("src/ui/api.ts", ""),
            ("src/other.ts", ""),
        ]);
        let contract = format!("{HEAD}%% map ui: src/ui/\n%% map api: src/ui/api.ts\n");
        let own = scan(&dir, &contract).unwrap();
        assert_eq!(owner(&own, "src/ui/a.ts").as_deref(), Some("ui"));
        assert_eq!(owner(&own, "src/ui/deep/b.ts").as_deref(), Some("ui"));
        assert_eq!(owner(&own, "src/ui/api.ts").as_deref(), Some("api"));
        assert!(own.unmapped.contains(Path::new("src/other.ts")));
        let site = &own.map_sites[Path::new("src/ui/api.ts")];
        assert_eq!((site.line, site.glob.as_str()), (6, "src/ui/api.ts"));
    }

    #[test]
    fn a_tie_between_different_nodes_names_both_lines_and_a_file() {
        let dir = repo(&[("src/ui/a.ts", ""), ("src/ui/a.test.ts", "")]);
        let contract = format!("{HEAD}%% map api: src/ui/*.test.*\n%% map store: src/ui/*.*.ts\n");
        // why: a.ts matches neither glob; a.test.ts matches both with specificity (2, 7).
        let error = scan(&dir, &contract).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("docs/architecture.mmd:5")
                && message.contains("docs/architecture.mmd:6")
                && message.contains("src/ui/a.test.ts"),
            "{message}"
        );
    }

    #[test]
    fn globs_of_one_node_never_tie_and_assets_never_fail() {
        let dir = repo(&[("src/a.ts", ""), ("src/x.json", "")]);
        let contract = format!(
            "{HEAD}%% map ui: src/*.ts, src/a.t*\n%% map api: src/*.json\n%% map store: src/*.js*\n"
        );
        let own = scan(&dir, &contract).unwrap();
        assert_eq!(owner(&own, "src/a.ts").as_deref(), Some("ui"));
        // why: the json asset ties api and store, which leaves it unowned instead of failing.
        assert!(own.asset_unmapped.contains(Path::new("src/x.json")));
    }

    #[test]
    fn module_form_derives_a_node_per_module_and_checks_the_names_later() {
        let dir = repo(&[
            ("src/stores/cart.ts", ""),
            ("src/stores/cart.test.ts", ""),
            ("src/stores/user/index.ts", ""),
        ]);
        let contract = format!("{HEAD}%% map store-*: src/stores/\n");
        let own = scan(&dir, &contract).unwrap();
        assert_eq!(
            owner(&own, "src/stores/cart.ts").as_deref(),
            Some("store-cart")
        );
        assert_eq!(
            owner(&own, "src/stores/cart.test.ts").as_deref(),
            Some("store-cart")
        );
        assert_eq!(
            owner(&own, "src/stores/user/index.ts").as_deref(),
            Some("store-user")
        );
        let names: Vec<(String, u32)> = own
            .tags
            .iter()
            .map(|(at, name)| (name.clone(), at.line))
            .collect();
        assert_eq!(
            names,
            [("store-cart".to_owned(), 5), ("store-user".to_owned(), 5)]
        );
    }

    #[test]
    fn a_plain_glob_inside_a_module_directory_wins() {
        let dir = repo(&[("src/stores/cart.ts", ""), ("src/stores/special.ts", "")]);
        let contract =
            format!("{HEAD}%% map store-*: src/stores/\n%% map api: src/stores/special.ts\n");
        let own = scan(&dir, &contract).unwrap();
        assert_eq!(owner(&own, "src/stores/special.ts").as_deref(), Some("api"));
        assert_eq!(
            owner(&own, "src/stores/cart.ts").as_deref(),
            Some("store-cart")
        );
    }

    #[test]
    fn arch_files_are_listed_not_read_and_stale_globs_are_found() {
        let dir = repo(&[
            ("src/.arch", "this is not valid"),
            ("src/a.ts", ""),
            ("lib/b.ts", ""),
        ]);
        let contract = format!("{HEAD}%% map ui: src/\n%% map api: nowhere/, lib/*.ts\n");
        let own = scan(&dir, &contract).unwrap();
        assert_eq!(own.ignored_tags, [PathBuf::from("src/.arch")]);
        assert_eq!(own.stale_maps.len(), 1);
        assert_eq!(own.stale_maps[0].glob, "nowhere/");
        assert_eq!(own.stale_maps[0].line, 6);
    }

    #[test]
    fn a_glob_shadowed_by_a_more_specific_one_is_not_stale() {
        let dir = repo(&[("src/a.ts", "")]);
        let contract = format!("{HEAD}%% map ui: src/\n%% map api: src/a.ts\n");
        assert_eq!(scan(&dir, &contract).unwrap().stale_maps, Vec::new());
    }

    #[test]
    fn assets_and_excluded_files_have_owners_too() {
        let dir = repo(&[
            ("src/a.ts", ""),
            ("src/a.json", "{}"),
            ("src/vendor/v.js", ""),
        ]);
        let own = scan(&dir, &format!("{HEAD}%% map ui: src/\n")).unwrap();
        assert_eq!(own.asset_owners[Path::new("src/a.json")].as_str(), "ui");
        assert_eq!(
            own.excluded_owners[Path::new("src/vendor/v.js")].as_str(),
            "ui"
        );
    }

    #[test]
    fn explain_names_the_glob_and_judges_unwalked_files_and_directories_by_name() {
        let dir = repo(&[("src/a.ts", ""), (".gitignore", "src/hidden.ts\n")]);
        let rules = rules_in(&format!("{HEAD}%% map ui: src/\n"));
        let context = MapContext {
            contract: Path::new("c.mmd"),
            rules: &rules,
        };
        let ask = |file: &str, is_dir: bool| {
            explain_mapped(dir.path(), Path::new(file), is_dir, context).unwrap()
        };
        let (found, site) = ask("src/a.ts", false);
        assert_eq!(found.node.unwrap().as_str(), "ui");
        assert_eq!(found.arch_file.as_deref(), Some(Path::new("c.mmd")));
        assert_eq!(site.unwrap().glob, "src/");
        assert_eq!(ask("src/hidden.ts", false).0.node.unwrap().as_str(), "ui");
        assert_eq!(ask("src", true).0.node.unwrap().as_str(), "ui");
        assert_eq!(
            ask("elsewhere.ts", false),
            (
                Explanation {
                    node: None,
                    arch_file: None,
                    folded: false
                },
                None
            )
        );
    }
}
