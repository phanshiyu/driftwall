//! Path patterns as `include` lists and `workspaces` write them: `*`, `?` and `**`.
//!
//! Pure. A pattern is a `/` separated list of segments. Inside a segment `*` matches any run of
//! characters and `?` one character, neither crossing a `/`. A segment that is exactly `**` matches
//! any number of whole segments, none included. A leading `./` and a trailing `/` are ignored.

use std::path::{Component, Path};

/// The segments of a pattern, without `.` and empty ones.
fn pattern_segments(pattern: &str) -> Vec<&str> {
    pattern
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect()
}

/// The segments of `path`. `None` for a path that is not valid UTF-8 or is not plain relative.
fn path_segments(path: &Path) -> Option<Vec<&str>> {
    path.components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str(),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => None,
        })
        .collect()
}

/// Whether `pattern` names exactly `path`.
#[must_use]
pub fn matches(pattern: &str, path: &Path) -> bool {
    path_segments(path)
        .is_some_and(|segments| match_segments(&pattern_segments(pattern), &segments))
}

/// Whether `pattern`, written as an `include` entry, selects the file `path`.
///
/// A pattern whose last segment has no `.`, `*` or `?` names a directory, and selects everything
/// below it, as TypeScript reads `src`. Any other pattern names files, so `src/*` selects only the
/// files directly in `src`. The empty pattern names everything.
#[must_use]
pub fn covers(pattern: &str, path: &Path) -> bool {
    let Some(segments) = path_segments(path) else {
        return false;
    };
    let pattern = pattern_segments(pattern);
    let names_directory = pattern
        .last()
        .is_none_or(|last| !last.contains(['.', '*', '?']));
    if !names_directory {
        return match_segments(&pattern, &segments);
    }
    pattern.is_empty()
        || (1..=segments.len()).any(|length| {
            segments
                .get(..length)
                .is_some_and(|head| match_segments(&pattern, head))
        })
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
            .is_some_and(|(head, tail)| match_segment(segment, head) && match_segments(rest, tail)),
    }
}

fn match_segment(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    match_chars(&pattern, &text)
}

fn match_chars(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len())
            .any(|skip| text.get(skip..).is_some_and(|tail| match_chars(rest, tail))),
        Some(('?', rest)) => text
            .split_first()
            .is_some_and(|(_, tail)| match_chars(rest, tail)),
        Some((expected, rest)) => text
            .split_first()
            .is_some_and(|(found, tail)| found == expected && match_chars(rest, tail)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is(pattern: &str, path: &str) -> bool {
        matches(pattern, Path::new(path))
    }

    #[test]
    fn a_star_stays_inside_one_segment() {
        assert!(is("packages/*", "packages/a"));
        assert!(!is("packages/*", "packages/a/b"));
        assert!(!is("packages/*", "packages"));
        assert!(is("src/*.ts", "src/a.ts"));
        assert!(!is("src/*.ts", "src/a.tsx"));
        assert!(is("a*c", "abbc"));
        assert!(is("a*c", "ac"));
    }

    #[test]
    fn a_question_mark_is_one_character() {
        assert!(is("app-?", "app-1"));
        assert!(!is("app-?", "app-12"));
        assert!(!is("app-?", "app-"));
    }

    #[test]
    fn a_double_star_is_any_number_of_segments_including_none() {
        assert!(is("src/**/*.ts", "src/a.ts"));
        assert!(is("src/**/*.ts", "src/x/y/a.ts"));
        assert!(!is("src/**/*.ts", "lib/a.ts"));
        assert!(is("**", "a/b/c"));
        assert!(is("**/a", "a"));
        assert!(is("a/**", "a"));
        assert!(!is("a/**/b", "a/c"));
    }

    #[test]
    fn a_leading_dot_slash_and_a_trailing_slash_are_ignored() {
        assert!(is("./src/", "src"));
        assert!(is("src/", "src"));
    }

    #[test]
    fn a_pattern_that_leaves_its_directory_matches_nothing() {
        assert!(!is("../a", "a"));
        assert!(!covers("../a", Path::new("a/b.ts")));
    }

    #[test]
    fn covers_includes_everything_below_a_match() {
        assert!(covers("src", Path::new("src/a/b.ts")));
        assert!(covers("src/*", Path::new("src/b.ts")));
        assert!(!covers("src/*", Path::new("src/a/b.ts")));
        assert!(covers("src/**", Path::new("src/a/b.ts")));
        assert!(covers("vite.config.ts", Path::new("vite.config.ts")));
        assert!(!covers("vite.config.ts", Path::new("vite.config.ts/x.ts")));
        assert!(!covers("app.v2", Path::new("app.v2/a.ts")));
        assert!(covers("src/*.ts", Path::new("src/a.ts")));
        assert!(!covers("src/*.ts", Path::new("src/a/b.ts")));
        assert!(!covers("src", Path::new("lib/a.ts")));
        assert!(!covers("src", Path::new("srcs/a.ts")));
        assert!(covers("", Path::new("anything/at/all.ts")));
        assert!(covers("**/*", Path::new("a/b.ts")));
    }
}
