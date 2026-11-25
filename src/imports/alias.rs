//! The typed model of a resolved `tsconfig.json` or `jsconfig.json`: path aliases and `baseUrl`.
//!
//! Everything here is pure. It decides which repository paths a specifier could stand for; the
//! caller probes them against the known files. The rules follow TypeScript's module resolution:
//!
//! - A `paths` key holds at most one `*`. A key with more is invalid and dropped, as is a target
//!   with more than one `*`.
//! - An exact key (no `*`) that equals the specifier wins over every pattern. Otherwise, among the
//!   star patterns that match, the one with the longest prefix wins and the first one declared
//!   wins a tie. Only the winner's targets are tried: a lost pattern is never a fallback.
//! - Targets are tried in order, each with the text matched by `*` substituted for its own `*`.
//! - When `baseUrl` is set, a specifier that no pattern resolves is also tried as
//!   `<baseUrl>/<specifier>`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::paths::join_normalised;

/// The alias configuration that governs the files below one directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AliasConfig {
    /// Repository-relative directory that `paths` targets are relative to: the `baseUrl` when one
    /// is set, else the directory of the config file that defines `paths`.
    pub base: PathBuf,
    /// Whether `baseUrl` is set, so that a bare specifier is also tried against [`base`].
    ///
    /// [`base`]: AliasConfig::base
    pub bare_imports: bool,
    /// The `paths` entries in the order they were declared.
    pub patterns: Vec<Pattern>,
}

/// One `paths` entry: a key that may hold a `*`, and the targets it maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    /// The key up to its `*`, or the whole key when it has none.
    pub prefix: String,
    /// The key after its `*`. `None` marks an exact key with no `*`.
    pub suffix: Option<String>,
    /// The targets in the order they are tried.
    pub targets: Vec<TargetTemplate>,
}

/// One entry of a `paths` target list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetTemplate {
    /// A target with no `*`: the path as written.
    Fixed(String),
    /// A target with one `*`, which is replaced by the text the key's `*` matched.
    Star {
        /// The target up to its `*`.
        prefix: String,
        /// The target after its `*`.
        suffix: String,
    },
}

/// A repository path a specifier may stand for. Not yet known to exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The lexically normalised repository-relative path, before extension and index probing.
    pub path: PathBuf,
    /// The path was written with a trailing `/`, so it can only name a directory.
    pub directory_only: bool,
}

/// What a pattern matched in a specifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture<'s> {
    /// An exact key equal to the whole specifier.
    Exact,
    /// A star pattern, with the text its `*` stood for.
    Star(&'s str),
}

impl Pattern {
    /// Parses one `paths` entry. `None` when the key holds more than one `*`. Targets with more
    /// than one `*` are dropped.
    #[must_use]
    pub fn parse(key: &str, targets: &[String]) -> Option<Self> {
        let (prefix, suffix) = split_star(key)?;
        Some(Self {
            prefix: prefix.to_owned(),
            suffix: suffix.map(str::to_owned),
            targets: targets
                .iter()
                .filter_map(|target| TargetTemplate::parse(target))
                .collect(),
        })
    }

    fn capture<'s>(&self, specifier: &'s str) -> Option<Capture<'s>> {
        self.suffix.as_ref().map_or_else(
            || (specifier == self.prefix).then_some(Capture::Exact),
            |suffix| {
                specifier
                    .strip_prefix(self.prefix.as_str())
                    .and_then(|rest| rest.strip_suffix(suffix.as_str()))
                    .map(Capture::Star)
            },
        )
    }
}

impl TargetTemplate {
    /// Parses one target. `None` when it holds more than one `*`.
    #[must_use]
    pub fn parse(target: &str) -> Option<Self> {
        let (prefix, suffix) = split_star(target)?;
        Some(suffix.map_or_else(
            || Self::Fixed(prefix.to_owned()),
            |suffix| Self::Star {
                prefix: prefix.to_owned(),
                suffix: suffix.to_owned(),
            },
        ))
    }

    /// The target text for a match. A target with a `*` has none for an exact key, which never
    /// matched any text.
    fn expand(&self, capture: Capture<'_>) -> Option<String> {
        match (self, capture) {
            (Self::Fixed(path), _) => Some(path.clone()),
            (Self::Star { prefix, suffix }, Capture::Star(text)) => {
                Some(format!("{prefix}{text}{suffix}"))
            }
            (Self::Star { .. }, Capture::Exact) => None,
        }
    }
}

/// Splits `text` around its only `*`. `None` when there are several, `(text, None)` when none.
fn split_star(text: &str) -> Option<(&str, Option<&str>)> {
    let Some((prefix, suffix)) = text.split_once('*') else {
        return Some((text, None));
    };
    if suffix.contains('*') {
        return None;
    }
    Some((prefix, Some(suffix)))
}

impl AliasConfig {
    /// The paths `specifier` could stand for, in the order they are tried. Empty when no pattern
    /// matches and `baseUrl` is not set. `specifier` must not be relative.
    ///
    /// A candidate that would leave the repository root is dropped.
    #[must_use]
    pub fn candidates(&self, specifier: &str) -> Vec<Candidate> {
        let mut found: Vec<Candidate> = self
            .select(specifier)
            .into_iter()
            .flat_map(|(pattern, capture)| {
                pattern
                    .targets
                    .iter()
                    .filter_map(move |target| target.expand(capture))
            })
            .filter_map(|target| self.candidate(&target))
            .collect();
        if self.bare_imports {
            found.extend(self.candidate(specifier));
        }
        found
    }

    /// Whether a `paths` pattern matches `specifier`, so it is an alias even when no file backs it.
    /// `baseUrl` alone is not an alias.
    #[must_use]
    pub fn matches(&self, specifier: &str) -> bool {
        self.select(specifier).is_some()
    }

    /// The pattern TypeScript would choose: an exact key equal to `specifier`, else the matching
    /// star pattern with the longest prefix, the first declared on a tie.
    fn select<'s>(&self, specifier: &'s str) -> Option<(&Pattern, Capture<'s>)> {
        let mut best: Option<(&Pattern, Capture<'s>)> = None;
        for pattern in &self.patterns {
            let Some(capture) = pattern.capture(specifier) else {
                continue;
            };
            if capture == Capture::Exact {
                return Some((pattern, capture));
            }
            let longer = best.is_none_or(|(held, _)| pattern.prefix.len() > held.prefix.len());
            if longer {
                best = Some((pattern, capture));
            }
        }
        best
    }

    fn candidate(&self, text: &str) -> Option<Candidate> {
        Some(Candidate {
            path: join_normalised(Some(&self.base), text)?,
            directory_only: text.ends_with('/'),
        })
    }
}

/// The entry of `configs` for `importer`, in its own directory or the nearest ancestor that has one.
///
/// `configs` is keyed by repository-relative directory and holds alias configurations or the
/// packages a `package.json` declares.
#[must_use]
pub fn nearest<'a, T>(configs: &'a BTreeMap<PathBuf, T>, importer: &Path) -> Option<&'a T> {
    importer
        .parent()?
        .ancestors()
        .find_map(|directory| configs.get(directory))
}
