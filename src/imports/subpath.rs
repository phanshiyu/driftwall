//! The subpath maps of a `package.json`: `exports` and `imports`.
//!
//! Pure. A map says which files inside a package a subpath names. This module only picks the
//! target texts, in the order they are to be tried; the caller probes them against the repository.
//! See "Workspace packages" in `docs/DESIGN.md` for the rules.

use std::collections::BTreeMap;

use serde_json::Value;

/// The conditions searched in a conditions object, in this order and never in the order the keys
/// are written. Every other condition is ignored.
const CONDITIONS: [&str; 4] = ["types", "import", "default", "require"];

/// One target of a subpath map.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// A path as written.
    Path(String),
    /// An object keyed by condition.
    Conditions(BTreeMap<String, Self>),
    /// Alternatives, tried in order.
    List(Vec<Self>),
    /// `null` and every other value: names nothing.
    Nothing,
}

impl Target {
    fn parse(value: &Value) -> Self {
        match value {
            Value::String(path) => Self::Path(path.clone()),
            Value::Object(conditions) => Self::Conditions(
                conditions
                    .iter()
                    .map(|(condition, target)| (condition.clone(), Self::parse(target)))
                    .collect(),
            ),
            Value::Array(targets) => Self::List(targets.iter().map(Self::parse).collect()),
            Value::Null | Value::Bool(_) | Value::Number(_) => Self::Nothing,
        }
    }

    /// Appends every path this target yields, in the order to try them. A path that does not start
    /// with `./` is not inside the package and yields nothing.
    fn paths(&self, found: &mut Vec<String>) {
        match self {
            Self::Path(path) => {
                if path.starts_with("./") {
                    found.push(path.clone());
                }
            }
            Self::Conditions(conditions) => {
                for condition in CONDITIONS {
                    if let Some(target) = conditions.get(condition) {
                        target.paths(found);
                    }
                }
            }
            Self::List(targets) => {
                for target in targets {
                    target.paths(found);
                }
            }
            Self::Nothing => {}
        }
    }
}

/// An `exports` or `imports` map, keyed by subpath.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subpaths {
    entries: BTreeMap<String, Target>,
}

impl Subpaths {
    /// Reads an `exports` field. An object with a key that starts with `.` is a subpath map, and its
    /// other keys are ignored. Any other value is the target of `.`. `None` for `null` and for
    /// values that cannot be a target.
    #[must_use]
    pub fn exports(value: &Value) -> Option<Self> {
        match value {
            Value::Object(map) if map.keys().any(|key| key.starts_with('.')) => {
                Some(Self::keyed(map, '.'))
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => None,
            other => Some(Self {
                entries: BTreeMap::from([(".".to_owned(), Target::parse(other))]),
            }),
        }
    }

    /// Reads an `imports` field: an object whose keys start with `#`. `None` for anything else.
    #[must_use]
    pub fn imports(value: &Value) -> Option<Self> {
        match value {
            Value::Object(map) => Some(Self::keyed(map, '#')),
            _ => None,
        }
    }

    fn keyed(map: &serde_json::Map<String, Value>, lead: char) -> Self {
        Self {
            entries: map
                .iter()
                .filter(|(key, _)| key.starts_with(lead))
                .map(|(key, target)| (key.clone(), Target::parse(target)))
                .collect(),
        }
    }

    /// The target paths `key` names, in the order to try them. `None` when no entry matches `key`;
    /// an empty list when an entry matches and names nothing.
    ///
    /// An exact entry wins. Otherwise among the entries with one `*` that match, the longest prefix
    /// wins, then the longest key. The text the `*` matched replaces every `*` of a target.
    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<Vec<String>> {
        let (target, matched) = self.select(key)?;
        let mut paths = Vec::new();
        target.paths(&mut paths);
        Some(
            paths
                .into_iter()
                .map(|path| match matched {
                    Some(text) => path.replace('*', text),
                    None => path,
                })
                .collect(),
        )
    }

    fn select<'a>(&'a self, key: &'a str) -> Option<(&'a Target, Option<&'a str>)> {
        if let Some(target) = self.entries.get(key) {
            return Some((target, None));
        }
        self.entries
            .iter()
            .filter_map(|(pattern, target)| {
                let (prefix, suffix) = pattern.split_once('*')?;
                if suffix.contains('*') {
                    return None;
                }
                let text = key.strip_prefix(prefix)?.strip_suffix(suffix)?;
                (!text.is_empty()).then_some(((prefix.len(), pattern.len()), target, text))
            })
            .max_by_key(|(rank, _, _)| *rank)
            .map(|(_, target, text)| (target, Some(text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exports(json: &str) -> Subpaths {
        Subpaths::exports(&serde_json::from_str(json).unwrap()).unwrap()
    }

    fn lookup(map: &Subpaths, key: &str) -> Option<Vec<String>> {
        map.lookup(key)
    }

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn a_string_is_the_target_of_the_package_root() {
        let map = exports(r#""./src/index.ts""#);
        assert_eq!(lookup(&map, "."), Some(strings(&["./src/index.ts"])));
        assert_eq!(lookup(&map, "./util"), None);
    }

    #[test]
    fn a_dot_map_has_one_entry_per_subpath() {
        let map = exports(r#"{".": "./src/index.ts", "./util": "./src/util.ts"}"#);
        assert_eq!(lookup(&map, "."), Some(strings(&["./src/index.ts"])));
        assert_eq!(lookup(&map, "./util"), Some(strings(&["./src/util.ts"])));
        assert_eq!(lookup(&map, "./other"), None);
    }

    #[test]
    fn an_object_of_conditions_is_the_target_of_the_root() {
        let map = exports(r#"{"import": "./a.mjs", "default": "./a.js"}"#);
        assert_eq!(lookup(&map, "."), Some(strings(&["./a.mjs", "./a.js"])));
    }

    #[test]
    fn conditions_are_searched_in_a_fixed_order_not_the_written_one() {
        let map = exports(
            r#"{".": {"require": "./r.cjs", "default": "./d.js", "node": "./n.js", "import": "./i.mjs", "types": "./t.d.ts"}}"#,
        );
        assert_eq!(
            lookup(&map, "."),
            Some(strings(&["./t.d.ts", "./i.mjs", "./d.js", "./r.cjs"]))
        );
    }

    #[test]
    fn nested_conditions_use_the_same_order() {
        let map = exports(
            r#"{".": {"import": {"default": "./i.js", "types": "./i.d.ts"}, "default": "./d.js"}}"#,
        );
        assert_eq!(
            lookup(&map, "."),
            Some(strings(&["./i.d.ts", "./i.js", "./d.js"]))
        );
    }

    #[test]
    fn an_array_is_tried_in_order_and_null_names_nothing() {
        let map = exports(r#"{".": ["./a.js", null, "./b.js"], "./x": null}"#);
        assert_eq!(lookup(&map, "."), Some(strings(&["./a.js", "./b.js"])));
        assert_eq!(lookup(&map, "./x"), Some(strings(&[])));
    }

    #[test]
    fn a_target_outside_the_package_names_nothing() {
        let map = exports(r#"{".": "lodash", "./a": "../a.js"}"#);
        assert_eq!(lookup(&map, "."), Some(strings(&[])));
        assert_eq!(lookup(&map, "./a"), Some(strings(&[])));
    }

    #[test]
    fn a_star_pattern_substitutes_the_matched_text_everywhere() {
        let map =
            exports(r#"{"./features/*": "./src/features/*/index.ts", "./*.js": "./lib/*.js"}"#);
        assert_eq!(
            lookup(&map, "./features/a/b"),
            Some(strings(&["./src/features/a/b/index.ts"]))
        );
        assert_eq!(lookup(&map, "./x.js"), Some(strings(&["./lib/x.js"])));
        assert_eq!(lookup(&map, "./features/"), None);
    }

    #[test]
    fn an_exact_key_beats_a_pattern_and_the_longest_prefix_beats_a_shorter_one() {
        let map = exports(
            r#"{"./*": "./any/*.ts", "./a/*": "./a/*.ts", "./a/special": "./special.ts", "./a/b/*": "./ab/*.ts"}"#,
        );
        assert_eq!(
            lookup(&map, "./a/special"),
            Some(strings(&["./special.ts"]))
        );
        assert_eq!(lookup(&map, "./a/x"), Some(strings(&["./a/x.ts"])));
        assert_eq!(lookup(&map, "./a/b/x"), Some(strings(&["./ab/x.ts"])));
        assert_eq!(lookup(&map, "./z"), Some(strings(&["./any/z.ts"])));
    }

    #[test]
    fn keys_of_a_subpath_map_that_do_not_start_with_a_dot_are_ignored() {
        let map = exports(r#"{".": "./a.ts", "import": "./b.ts"}"#);
        assert_eq!(lookup(&map, "."), Some(strings(&["./a.ts"])));
    }

    #[test]
    fn exports_that_cannot_be_a_target_are_none() {
        assert_eq!(Subpaths::exports(&Value::Null), None);
        assert_eq!(Subpaths::exports(&Value::Bool(true)), None);
    }

    #[test]
    fn imports_keep_only_hash_keys() {
        let value: Value =
            serde_json::from_str(r##"{"#internal/*": "./src/internal/*.ts", "other": "./x.ts"}"##)
                .unwrap();
        let map = Subpaths::imports(&value).unwrap();
        assert_eq!(
            lookup(&map, "#internal/a"),
            Some(strings(&["./src/internal/a.ts"]))
        );
        assert_eq!(lookup(&map, "other"), None);
        assert_eq!(Subpaths::imports(&Value::Null), None);
    }
}
