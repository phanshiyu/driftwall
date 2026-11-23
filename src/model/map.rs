//! The `%% map` directive of the contract: globs that map files to nodes (see "Contract map" in
//! `docs/DESIGN.md`). Pure: no file system.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use super::{NodeId, Prefix};

/// Why a map glob was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GlobError {
    /// Nothing, or only whitespace.
    #[error("empty glob")]
    Empty,
    /// Starts with `!`.
    #[error("`!` negation is not supported")]
    Negated,
    /// A `..` segment.
    #[error("a `..` segment cannot stay inside the repository")]
    ParentDir,
    /// A character with no meaning in a map glob.
    #[error("`{0}` is not supported in a glob")]
    Unsupported(char),
    /// `**` mixed with other characters in a segment.
    #[error("`**` must be a whole segment")]
    DoubleStar,
}

/// A path pattern of a `%% map` line, anchored at the repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapGlob {
    /// The glob as written, trimmed.
    text: String,
    /// The segments without empty and `.` ones.
    segments: Vec<String>,
    /// Whether the glob ends in `/`: it matches every file below the directory.
    subtree: bool,
}

/// How specific a glob is: leading literal segments, then literal prefix length. Bigger wins.
pub type Specificity = (usize, usize);

impl MapGlob {
    /// Parses one glob.
    ///
    /// # Errors
    /// A [`GlobError`] when `raw` is empty, negated, climbs out with `..`, uses a character
    /// outside the glob language, or mixes `**` into a longer segment.
    pub fn parse(raw: &str) -> Result<Self, GlobError> {
        let text = raw.trim();
        if text.is_empty() {
            return Err(GlobError::Empty);
        }
        if text.starts_with('!') {
            return Err(GlobError::Negated);
        }
        if let Some(bad) = text.chars().find(|c| "\\[]{}".contains(*c)) {
            return Err(GlobError::Unsupported(bad));
        }
        let mut segments = Vec::new();
        for segment in text.split('/').filter(|s| !s.is_empty() && *s != ".") {
            if segment == ".." {
                return Err(GlobError::ParentDir);
            }
            if segment.contains("**") && segment != "**" {
                return Err(GlobError::DoubleStar);
            }
            segments.push(segment.to_owned());
        }
        let subtree = text.ends_with('/');
        if segments.is_empty() && !subtree {
            return Err(GlobError::Empty);
        }
        Ok(Self {
            text: text.to_owned(),
            segments,
            subtree,
        })
    }

    /// The glob as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The form that two spellings of one glob share.
    #[must_use]
    pub fn normalized(&self) -> String {
        let joined = self.segments.join("/");
        if self.subtree {
            format!("{joined}/")
        } else {
            joined
        }
    }

    /// The directory of a glob that is exactly a directory without wildcards (`src/ui/`, `./`),
    /// relative to the root (empty for the root), or `None` for any other glob.
    #[must_use]
    pub fn directory(&self) -> Option<PathBuf> {
        (self.subtree && self.segments.iter().all(|s| !has_wildcard(s)))
            .then(|| self.segments.iter().collect())
    }

    /// How specific the glob is (see "Contract map" in `docs/DESIGN.md`).
    #[must_use]
    pub fn specificity(&self) -> Specificity {
        let literal = self
            .segments
            .iter()
            .take_while(|segment| !has_wildcard(segment))
            .count();
        let prefix: usize = self
            .segments
            .iter()
            .take(literal)
            .map(|s| s.len() + 1)
            .sum();
        let partial = self
            .segments
            .get(literal)
            .and_then(|segment| segment.split(['*', '?']).next())
            .map_or(0, str::len);
        (literal, prefix + partial)
    }

    /// Whether the glob matches the repository-relative file `path`.
    #[must_use]
    pub fn matches(&self, path: &Path) -> bool {
        let Some(parts) = path
            .components()
            .map(|component| match component {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<&str>>>()
        else {
            return false;
        };
        let mut pattern: Vec<&str> = self.segments.iter().map(String::as_str).collect();
        if self.subtree {
            pattern.push("**");
        }
        match_segments(&pattern, &parts)
    }
}

impl fmt::Display for MapGlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn has_wildcard(segment: &str) -> bool {
    segment.contains(['*', '?'])
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| {
            path.get(skip..)
                .is_some_and(|tail| match_segments(rest, tail))
        }),
        Some((segment, rest)) => path
            .split_first()
            .is_some_and(|(head, tail)| match_name(segment, head) && match_segments(rest, tail)),
    }
}

/// Whether `name` fits `pattern`, where `*` is any run of characters and `?` one character.
fn match_name(pattern: &str, name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let mut reach = vec![false; name.len() + 1];
    if let Some(first) = reach.first_mut() {
        *first = true;
    }
    for symbol in pattern.chars() {
        let mut next = vec![false; name.len() + 1];
        if symbol == '*' {
            let mut open = false;
            for (slot, was) in next.iter_mut().zip(&reach) {
                open |= *was;
                *slot = open;
            }
        } else {
            for (i, ch) in name.iter().enumerate() {
                let was = reach.get(i).copied().unwrap_or(false);
                if was
                    && (symbol == '?' || symbol == *ch)
                    && let Some(slot) = next.get_mut(i + 1)
                {
                    *slot = true;
                }
            }
        }
        reach = next;
    }
    reach.last().copied().unwrap_or(false)
}

/// What a `%% map` line maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapTarget {
    /// Every matching file is in this node.
    Node(NodeId),
    /// Every module in the directory is its own node, with this prefix (`node: <prefix>*`).
    Modules(Prefix),
}

impl fmt::Display for MapTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(id) => write!(f, "{id}"),
            Self::Modules(prefix) => write!(f, "{prefix}*"),
        }
    }
}

/// One `%% map` line.
#[derive(Debug, Clone)]
pub struct MapRule {
    /// 1-indexed line of the contract. Not part of equality: a rendered contract has other lines.
    pub line: u32,
    /// What the globs map to.
    pub target: MapTarget,
    /// The globs, in the order written.
    pub globs: Vec<MapGlob>,
}

impl PartialEq for MapRule {
    fn eq(&self, other: &Self) -> bool {
        // why: render moves lines around, and a round trip must still compare equal.
        self.target == other.target && self.globs == other.globs
    }
}

impl Eq for MapRule {}

/// The rule and glob that decided a file's owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapSite {
    /// Line of the `%% map` directive.
    pub line: u32,
    /// The glob as written.
    pub glob: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(raw: &str) -> MapGlob {
        MapGlob::parse(raw).unwrap()
    }

    fn hit(raw: &str, path: &str) -> bool {
        glob(raw).matches(Path::new(path))
    }

    #[test]
    fn directories_match_their_whole_subtree_anchored_at_the_root() {
        assert!(hit("src/ui/", "src/ui/a.ts"));
        assert!(hit("/src/ui/", "src/ui/deep/er/a.ts"));
        assert!(hit("./src/ui/", "src/ui/a.ts"));
        assert!(!hit("ui/", "src/ui/a.ts"));
        assert!(!hit("src/ui/", "src/uix/a.ts"));
        assert!(hit("./", "any/where.ts"));
    }

    #[test]
    fn a_glob_without_a_slash_at_the_end_names_files() {
        assert!(hit("src/*", "src/a.ts"));
        assert!(!hit("src/*", "src/ui/a.ts"));
        assert!(!hit("src/ui", "src/ui/a.ts"));
        assert!(hit("src/a.ts", "src/a.ts"));
        assert!(hit("src/*.ts", "src/a.ts"));
        assert!(!hit("src/*.ts", "src/a.rs"));
        assert!(hit("src/a?.ts", "src/ab.ts"));
        assert!(!hit("src/a?.ts", "src/a.ts"));
    }

    #[test]
    fn double_star_spans_any_number_of_segments() {
        assert!(hit("src/**/*.svelte", "src/a.svelte"));
        assert!(hit("src/**/*.svelte", "src/x/y/a.svelte"));
        assert!(!hit("src/**/*.svelte", "src/x/a.ts"));
        assert!(hit("**/*.test.ts", "a/b/c.test.ts"));
        assert!(hit("**", "a/b.ts"));
    }

    #[test]
    fn star_matches_inside_one_segment_only() {
        assert!(hit("a*c/x.ts", "abbc/x.ts"));
        assert!(hit("a*c/x.ts", "ac/x.ts"));
        assert!(!hit("a*c/x.ts", "a/b/c/x.ts"));
        assert!(hit("*.ts", "a.ts"));
        assert!(!hit("*.ts", "d/a.ts"));
    }

    #[test]
    fn specificity_counts_literal_segments_then_prefix_length() {
        assert_eq!(glob("src/ui/").specificity(), (2, 7));
        assert_eq!(glob("src/ui/*.ts").specificity(), (2, 7));
        assert_eq!(glob("src/ui/a*.ts").specificity(), (2, 8));
        assert_eq!(glob("src/*.ts").specificity(), (1, 4));
        assert_eq!(glob("**/x.ts").specificity(), (0, 0));
        assert_eq!(glob("./").specificity(), (0, 0));
        assert_eq!(glob("src/a.ts").specificity(), (2, 9));
        assert!(glob("src/ui/").specificity() > glob("src/").specificity());
    }

    #[test]
    fn bad_globs_are_rejected_with_a_reason() {
        assert_eq!(MapGlob::parse("  "), Err(GlobError::Empty));
        assert_eq!(MapGlob::parse("."), Err(GlobError::Empty));
        assert_eq!(MapGlob::parse("!a/"), Err(GlobError::Negated));
        assert_eq!(MapGlob::parse("../a/"), Err(GlobError::ParentDir));
        assert_eq!(MapGlob::parse("a/{b,c}"), Err(GlobError::Unsupported('{')));
        assert_eq!(MapGlob::parse("a/[b]"), Err(GlobError::Unsupported('[')));
        assert_eq!(MapGlob::parse("a**/b"), Err(GlobError::DoubleStar));
    }

    #[test]
    fn directory_is_only_for_plain_directories() {
        assert_eq!(glob("src/ui/").directory(), Some("src/ui".into()));
        assert_eq!(glob("./").directory(), Some(PathBuf::new()));
        assert_eq!(glob("src/*/").directory(), None);
        assert_eq!(glob("src/a.ts").directory(), None);
        assert_eq!(glob("/src/ui/").normalized(), "src/ui/");
    }
}
