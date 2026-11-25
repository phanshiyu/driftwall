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

/// How specific a glob is: leading literal segments, then whether it matches files by pattern
/// rather than as a subtree, then literal prefix length, then literal suffix length. Bigger wins.
pub type Specificity = (usize, bool, usize, usize);

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
        // why: a glob that picks files by pattern is a deliberate carve-out of its directory.
        let suffix = if self.subtree {
            0
        } else {
            self.segments
                .last()
                .filter(|segment| has_wildcard(segment))
                .and_then(|segment| segment.rsplit(['*', '?']).next())
                .map_or(0, str::len)
        };
        (literal, !self.subtree, prefix + partial, suffix)
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
    /// The `%% each ... map` line this rule is the expansion of, or `None` for a written line.
    /// Not part of equality: the template line is what is written back.
    pub template: Option<MapTemplate>,
}

/// The `%% each <boundary>: map <dir>/` line that an expanded map rule comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapTemplate {
    /// The boundary the `%% each` line names.
    pub boundary: String,
    /// The child boundary of `boundary` this expansion is for.
    pub child: String,
    /// The directory pattern as written, `src/features/{child}/`.
    pub pattern: String,
}

/// The placeholder of an `%% each ... map` directory pattern.
const CHILD: &str = "{child}";

impl MapTemplate {
    /// The template as `%% each` writes it, `each features map src/features/{child}/`.
    #[must_use]
    pub fn text(&self) -> String {
        format!("each {} map {}", self.boundary, self.pattern)
    }
}

/// The name that would make `pattern` (a directory pattern with `{child}`) cover
/// `file`, if there is one: the part of the path where the placeholder stands.
#[must_use]
pub(super) fn child_named_by(pattern: &str, file: &Path) -> Option<String> {
    let parts: Vec<&str> = pattern
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    let names: Vec<&str> = file
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    let at = parts.iter().position(|segment| segment.contains(CHILD))?;
    let (segment, name) = (parts.get(at)?, names.get(at)?);
    let (head, _) = segment.split_once(CHILD)?;
    let tail = segment.rsplit_once(CHILD).map_or("", |(_, tail)| tail);
    let child = name
        .strip_prefix(head)?
        .strip_suffix(tail)
        .filter(|child| !child.is_empty())?;
    let glob = MapGlob::parse(&pattern.replace(CHILD, child)).ok()?;
    (glob.matches(file) && NodeId::new(child).is_ok()).then(|| child.to_owned())
}

impl MapRule {
    /// The `%% each` line this rule expands, as `each features map src/{child}/`, or `None` for a
    /// written line.
    #[must_use]
    pub fn each_text(&self) -> Option<String> {
        self.template.as_ref().map(MapTemplate::text)
    }

    /// Why `dir` cannot be a directory pattern of `%% each <boundary>: map <dir>/`, if it cannot:
    /// it ends in `/`, has `{child}` and no other placeholder, and is a plain directory once a
    /// child is filled in.
    ///
    /// # Errors
    /// The reason, as a sentence fragment.
    pub fn check_pattern(dir: &str) -> Result<(), String> {
        if !dir.ends_with('/') {
            return Err("a directory pattern ends in `/`".to_owned());
        }
        if !dir.contains(CHILD) {
            return Err("a directory pattern has `{child}` in it".to_owned());
        }
        if dir.replace(CHILD, "").contains(['{', '}']) {
            return Err("`{child}` is the only placeholder".to_owned());
        }
        let sample =
            MapGlob::parse(&dir.replace(CHILD, "x")).map_err(|reason| reason.to_string())?;
        if sample.directory().is_none() {
            return Err("a directory pattern has no wildcard besides `{child}`".to_owned());
        }
        Ok(())
    }

    /// The module-form rule at `line` that the `%% each <boundary>: map <dir>` line expands to for
    /// `child`: `%% map <child>-*: <dir with the child filled in>`.
    ///
    /// # Errors
    /// The reason, when `child` makes no prefix or the filled-in directory is no glob.
    pub fn expansion(line: u32, boundary: &str, child: &str, dir: &str) -> Result<Self, String> {
        let prefix = Prefix::new(&format!("{child}-")).map_err(|reason| reason.to_string())?;
        let glob =
            MapGlob::parse(&dir.replace(CHILD, child)).map_err(|reason| reason.to_string())?;
        Ok(Self {
            line,
            target: MapTarget::Modules(prefix),
            globs: vec![glob],
            template: Some(MapTemplate {
                boundary: boundary.to_owned(),
                child: child.to_owned(),
                pattern: dir.to_owned(),
            }),
        })
    }
}

impl PartialEq for MapRule {
    fn eq(&self, other: &Self) -> bool {
        // why: render moves lines around, and a round trip must still compare equal.
        self.target == other.target && self.globs == other.globs
    }
}

impl Eq for MapRule {}

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
        assert_eq!(glob("src/ui/").specificity(), (2, false, 7, 0));
        assert_eq!(glob("src/ui/*.ts").specificity(), (2, true, 7, 3));
        assert_eq!(glob("src/ui/a*.ts").specificity(), (2, true, 8, 3));
        assert_eq!(glob("src/*.ts").specificity(), (1, true, 4, 3));
        assert_eq!(glob("**/x.ts").specificity(), (0, true, 0, 0));
        assert_eq!(glob("./").specificity(), (0, false, 0, 0));
        assert_eq!(glob("src/a.ts").specificity(), (2, true, 9, 0));
        assert!(glob("src/ui/").specificity() > glob("src/").specificity());
    }

    #[test]
    fn a_file_glob_beats_the_subtree_of_its_directory() {
        let ui = glob("ui/src/components/").specificity();
        assert!(glob("ui/src/components/*.test.tsx").specificity() > ui);
        assert!(glob("src/*.ts").specificity() > glob("src/").specificity());
        assert!(glob("src/a*.ts").specificity() > glob("src/*.ts").specificity());
        assert!(glob("src/*.test.ts").specificity() > glob("src/*.ts").specificity());
        // why: depth still comes first, so a deeper subtree beats a shallower file glob.
        assert!(glob("src/ui/").specificity() > glob("src/*.ts").specificity());
        assert_eq!(
            glob("src/*.ts").specificity(),
            glob("src/*.ts").specificity()
        );
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
    fn the_child_a_pattern_would_cover_is_the_name_at_the_placeholder() {
        let named = |pattern: &str, file: &str| child_named_by(pattern, Path::new(file));
        let pattern = "src/features/{child}/";
        assert_eq!(
            named(pattern, "src/features/billing/service/a.ts"),
            Some("billing".to_owned())
        );
        assert_eq!(
            named(pattern, "src/features/billing/a.ts"),
            Some("billing".to_owned())
        );
        assert_eq!(named(pattern, "src/features/a.ts"), None);
        assert_eq!(named(pattern, "src/other/billing/a.ts"), None);
        assert_eq!(
            named("src/{child}-app/", "src/orders-app/a.ts"),
            Some("orders".to_owned())
        );
        assert_eq!(named("src/{child}-app/", "src/orders/a.ts"), None);
        assert_eq!(named("src/{child}/", "src/not an id/a.ts"), None);
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
