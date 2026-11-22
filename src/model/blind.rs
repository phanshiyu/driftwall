//! What the analysis cannot see: files it does not read and references it cannot follow.
//!
//! Nothing here is a finding. [`BlindSpots`] lists every such item with the node it belongs to, so
//! a report can say how much of a repository its answers cover. See "Blind spots" in
//! `docs/DESIGN.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

use super::{Exclusion, Location, NodeId, Ownership, SkipReason, Skipped, UnresolvedImport};

/// A walked file the analysis does not use, and the node whose tags cover its path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct BlindFile {
    /// Repository-relative path.
    pub file: PathBuf,
    /// The node that owns the path by the tag rules, or `None` when no tag covers it.
    pub node: Option<NodeId>,
    /// The number of lines, known only for unread files. Omitted from JSON when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
}

/// An import driftwall saw and did not turn into an edge, and the node of the file it is in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct BlindRef {
    /// Where the statement starts.
    #[serde(flatten)]
    pub at: Location,
    /// The specifier as written.
    pub specifier: String,
    /// The node that owns the file the statement is in, or `None` when no tag covers it.
    pub node: Option<NodeId>,
    /// A sentence of explanation, when the skip has one. Omitted from JSON otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The unread files of one census key (an extension or `shebang`), sorted by path.
///
/// Serialized as `count`, `lines` (the sum over files that have a count) and `files`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnreadGroup {
    /// The files, sorted by path.
    pub files: Vec<BlindFile>,
}

impl UnreadGroup {
    /// The total number of lines of the files.
    #[must_use]
    pub fn lines(&self) -> usize {
        self.files.iter().filter_map(|file| file.lines).sum()
    }
}

impl Serialize for UnreadGroup {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Shape<'a> {
            count: usize,
            lines: usize,
            files: &'a [BlindFile],
        }
        Shape {
            count: self.files.len(),
            lines: self.lines(),
            files: &self.files,
        }
        .serialize(serializer)
    }
}

/// Everything the analysis of one repository could not see, with the node of each item.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BlindSpots {
    /// Code files in a language driftwall has no table entry for, by census key.
    pub unread: BTreeMap<String, UnreadGroup>,
    /// Source files left out as vendored, minified or generated, by reason.
    pub excluded: BTreeMap<Exclusion, Vec<BlindFile>>,
    /// Source files only `.driftwallignore` hides.
    pub ignored: Vec<BlindFile>,
    /// References that could not be followed, by reason.
    pub skipped: BTreeMap<SkipReason, Vec<BlindRef>>,
    /// Relative imports that matched no file.
    pub unresolved: Vec<BlindRef>,
}

/// Counts of the blind spots of one node (or of what no node owns).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BlindTally {
    /// Unread files.
    pub unread: usize,
    /// Excluded files.
    pub excluded: usize,
    /// Files only `.driftwallignore` hides.
    pub ignored: usize,
    /// Skipped references.
    pub skipped: usize,
    /// Unresolved imports.
    pub unresolved: usize,
}

impl BlindTally {
    /// The sum of all five counts.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.unread + self.excluded + self.ignored + self.skipped + self.unresolved
    }

    /// True when every count is zero.
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.total() == 0
    }
}

/// The files and lines of one unread census key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct UnreadTotal {
    /// Files of the key.
    pub count: usize,
    /// Lines in them.
    pub lines: usize,
}

/// [`BlindSpots`] reduced to counts: what the `lint` note, `lint` JSON and `brief` show.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BlindSummary {
    /// Unread files and lines by census key.
    pub unread: BTreeMap<String, UnreadTotal>,
    /// Excluded files by reason.
    pub excluded: BTreeMap<Exclusion, usize>,
    /// Files only `.driftwallignore` hides.
    pub ignored: usize,
    /// Skipped references by reason.
    pub skipped: BTreeMap<SkipReason, usize>,
    /// Unresolved imports.
    pub unresolved: usize,
}

impl BlindSummary {
    /// True when there is nothing to report.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unread.is_empty()
            && self.excluded.is_empty()
            && self.ignored == 0
            && self.skipped.is_empty()
            && self.unresolved == 0
    }
}

impl BlindSpots {
    /// Collects the blind spots of a scan.
    ///
    /// `owner` names the node that owns a repository-relative path (see
    /// `ownership::owner_in`); it attributes census files and, through the file they are written
    /// in, references. `lines` gives the number of lines of an unread file when it is known. Every
    /// list comes out sorted.
    pub fn build<'a>(
        ownership: &Ownership,
        owner: impl Fn(&Path) -> Option<NodeId>,
        lines: impl Fn(&Path) -> Option<usize>,
        skipped: impl IntoIterator<Item = &'a Skipped>,
        unresolved: impl IntoIterator<Item = &'a UnresolvedImport>,
    ) -> Self {
        let file = |path: &PathBuf, lines: Option<usize>| BlindFile {
            file: path.clone(),
            node: owner(path),
            lines,
        };
        let mut spots = Self::default();
        for (key, files) in &ownership.unread {
            let files = files.iter().map(|path| file(path, lines(path))).collect();
            spots.unread.insert(key.clone(), UnreadGroup { files });
        }
        for (reason, files) in &ownership.excluded {
            let files = files.iter().map(|path| file(path, None)).collect();
            spots.excluded.insert(*reason, files);
        }
        spots.ignored = ownership
            .ignored
            .iter()
            .map(|path| file(path, None))
            .collect();
        for reference in skipped {
            spots
                .skipped
                .entry(reference.reason)
                .or_default()
                .push(BlindRef {
                    at: reference.at.clone(),
                    specifier: reference.specifier.clone(),
                    node: owner(&reference.at.file),
                    message: reference.message.clone(),
                });
        }
        for reference in unresolved {
            spots.unresolved.push(BlindRef {
                at: reference.at.clone(),
                specifier: reference.specifier.clone(),
                node: owner(&reference.at.file),
                message: None,
            });
        }
        for list in spots.skipped.values_mut() {
            list.sort();
        }
        spots.unresolved.sort();
        spots
    }

    /// True when there is nothing to report.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.summary().is_empty()
    }

    /// The counts of everything.
    #[must_use]
    pub fn summary(&self) -> BlindSummary {
        BlindSummary {
            unread: self
                .unread
                .iter()
                .filter(|(_, group)| !group.files.is_empty())
                .map(|(key, group)| {
                    let total = UnreadTotal {
                        count: group.files.len(),
                        lines: group.lines(),
                    };
                    (key.clone(), total)
                })
                .collect(),
            excluded: self
                .excluded
                .iter()
                .filter(|(_, files)| !files.is_empty())
                .map(|(reason, files)| (*reason, files.len()))
                .collect(),
            ignored: self.ignored.len(),
            skipped: self
                .skipped
                .iter()
                .filter(|(_, refs)| !refs.is_empty())
                .map(|(reason, refs)| (*reason, refs.len()))
                .collect(),
            unresolved: self.unresolved.len(),
        }
    }

    /// The counts of the items attributed to `node`, where `None` is the items no node owns.
    #[must_use]
    pub fn tally(&self, node: Option<&NodeId>) -> BlindTally {
        let files = |list: &[BlindFile]| list.iter().filter(|f| f.node.as_ref() == node).count();
        let refs = |list: &[BlindRef]| list.iter().filter(|r| r.node.as_ref() == node).count();
        BlindTally {
            unread: self.unread.values().map(|group| files(&group.files)).sum(),
            excluded: self.excluded.values().map(|list| files(list)).sum(),
            ignored: files(&self.ignored),
            skipped: self.skipped.values().map(|list| refs(list)).sum(),
            unresolved: refs(&self.unresolved),
        }
    }

    /// Only the items attributed to `node`.
    #[must_use]
    pub fn of_node(&self, node: &NodeId) -> Self {
        self.attributed_to(Some(node))
    }

    /// Only the items no node owns.
    #[must_use]
    pub fn of_unowned(&self) -> Self {
        self.attributed_to(None)
    }

    /// Only the items attributed to `own`, where `None` is no node.
    fn attributed_to(&self, own: Option<&NodeId>) -> Self {
        let files = |list: &[BlindFile]| -> Vec<BlindFile> {
            list.iter()
                .filter(|f| f.node.as_ref() == own)
                .cloned()
                .collect()
        };
        let refs = |list: &[BlindRef]| -> Vec<BlindRef> {
            list.iter()
                .filter(|r| r.node.as_ref() == own)
                .cloned()
                .collect()
        };
        Self {
            unread: self
                .unread
                .iter()
                .map(|(key, group)| {
                    (
                        key.clone(),
                        UnreadGroup {
                            files: files(&group.files),
                        },
                    )
                })
                .filter(|(_, group)| !group.files.is_empty())
                .collect(),
            excluded: self
                .excluded
                .iter()
                .map(|(reason, list)| (*reason, files(list)))
                .filter(|(_, list)| !list.is_empty())
                .collect(),
            ignored: files(&self.ignored),
            skipped: self
                .skipped
                .iter()
                .map(|(reason, list)| (*reason, refs(list)))
                .filter(|(_, list)| !list.is_empty())
                .collect(),
            unresolved: refs(&self.unresolved),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn id(raw: &str) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    fn at(file: &str, line: u32) -> Location {
        Location {
            file: PathBuf::from(file),
            line,
        }
    }

    fn set(paths: &[&str]) -> BTreeSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    fn owner(path: &Path) -> Option<NodeId> {
        path.starts_with("a").then(|| id("a"))
    }

    fn sample() -> BlindSpots {
        let ownership = Ownership {
            unread: BTreeMap::from([
                ("swift".to_owned(), set(&["a/x.swift", "loose.swift"])),
                ("sh".to_owned(), set(&["b/run.sh"])),
            ]),
            excluded: BTreeMap::from([(Exclusion::Vendored, set(&["a/vendor/v.js"]))]),
            ignored: set(&["a/old.py"]),
            ..Ownership::default()
        };
        let skipped = [
            Skipped {
                at: at("a/m.ts", 3),
                specifier: "left-pad".to_owned(),
                reason: SkipReason::UndeclaredPackage,
                message: None,
            },
            Skipped {
                at: at("c/n.py", 1),
                specifier: "tool".to_owned(),
                reason: SkipReason::UnreachableModule,
                message: Some("matches x/tool.py".to_owned()),
            },
        ];
        let unresolved = [UnresolvedImport {
            at: at("a/m.ts", 5),
            specifier: "./gone".to_owned(),
        }];
        BlindSpots::build(
            &ownership,
            owner,
            |path| Some(path.as_os_str().len()),
            &skipped,
            &unresolved,
        )
    }

    #[test]
    fn items_are_attributed_to_the_node_the_closure_names() {
        let spots = sample();
        let nodes: Vec<_> = spots.unread["swift"]
            .files
            .iter()
            .map(|f| (f.file.display().to_string(), f.node.clone()))
            .collect();
        assert_eq!(
            nodes,
            [
                ("a/x.swift".to_owned(), Some(id("a"))),
                ("loose.swift".to_owned(), None)
            ]
        );
        assert_eq!(spots.skipped[&SkipReason::UnreachableModule][0].node, None);
        assert_eq!(spots.unresolved[0].node, Some(id("a")));
    }

    #[test]
    fn the_summary_counts_files_lines_and_references() {
        let summary = sample().summary();
        assert_eq!(
            summary.unread["swift"],
            UnreadTotal {
                count: 2,
                lines: "a/x.swift".len() + "loose.swift".len()
            }
        );
        assert_eq!(summary.unread["sh"].count, 1);
        assert_eq!(summary.excluded[&Exclusion::Vendored], 1);
        assert_eq!(summary.ignored, 1);
        assert_eq!(summary.skipped[&SkipReason::UndeclaredPackage], 1);
        assert_eq!(summary.unresolved, 1);
        assert!(!summary.is_empty());
        assert!(BlindSpots::default().is_empty());
    }

    #[test]
    fn a_tally_counts_one_node_and_none_counts_the_unowned() {
        let spots = sample();
        assert_eq!(
            spots.tally(Some(&id("a"))),
            BlindTally {
                unread: 1,
                excluded: 1,
                ignored: 1,
                skipped: 1,
                unresolved: 1
            }
        );
        assert_eq!(
            spots.tally(None),
            BlindTally {
                unread: 2,
                excluded: 0,
                ignored: 0,
                skipped: 1,
                unresolved: 0
            }
        );
        assert!(spots.tally(Some(&id("zzz"))).is_zero());
    }

    #[test]
    fn of_node_keeps_only_its_items_and_drops_empty_groups() {
        let own = sample().of_node(&id("a"));
        assert_eq!(own.unread.keys().collect::<Vec<_>>(), ["swift"]);
        assert_eq!(own.unread["swift"].files.len(), 1);
        assert_eq!(own.excluded.len(), 1);
        assert_eq!(
            own.skipped.keys().collect::<Vec<_>>(),
            [&SkipReason::UndeclaredPackage]
        );
        assert_eq!(own.tally(Some(&id("a"))), sample().tally(Some(&id("a"))));
    }

    #[test]
    fn of_unowned_keeps_only_the_items_no_node_owns() {
        let none = sample().of_unowned();
        assert_eq!(none.unread.keys().collect::<Vec<_>>(), ["sh", "swift"]);
        assert_eq!(none.unread["swift"].files.len(), 1);
        assert_eq!(
            none.unread["swift"].files[0].file,
            PathBuf::from("loose.swift")
        );
        assert!(none.excluded.is_empty());
        assert_eq!(
            none.skipped.keys().collect::<Vec<_>>(),
            [&SkipReason::UnreachableModule]
        );
        assert_eq!(none.tally(None), sample().tally(None));
    }

    #[test]
    fn json_has_counts_for_unread_and_omits_unknown_lines_and_messages() {
        let json = serde_json::to_value(sample()).unwrap();
        assert_eq!(json["unread"]["sh"]["count"], 1);
        assert_eq!(json["unread"]["sh"]["lines"], "b/run.sh".len());
        assert_eq!(json["excluded"]["vendored"][0]["file"], "a/vendor/v.js");
        assert!(json["excluded"]["vendored"][0].get("lines").is_none());
        assert_eq!(
            json["skipped"]["unreachable-module"][0]["message"],
            "matches x/tool.py"
        );
        assert!(
            json["skipped"]["undeclared-package"][0]
                .get("message")
                .is_none()
        );
        assert_eq!(json["skipped"]["undeclared-package"][0]["file"], "a/m.ts");
        assert_eq!(json["unresolved"][0]["specifier"], "./gone");
    }
}
