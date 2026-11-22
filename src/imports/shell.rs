//! Resolution of the files a shell script loads with `source` and `.`.
//!
//! The path of a command is evaluated, never run: a small set of forms that are anchored at the
//! directory of the script (`$(dirname "$0")`, `${BASH_SOURCE%/*}`, variables assigned from those)
//! has a value, and everything else is dynamic. See "Shell resolution" in `docs/DESIGN.md`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use super::glob;
use super::resolve::join_normalised;
use super::{Environment, Reference, Resolution};
use crate::model::SkipReason;

/// The variables that hold the path of the running script.
const SCRIPT_VARIABLES: &[&str] = &["0", "BASH_SOURCE"];
/// The pattern characters of a glob that are expanded.
const GLOB_CHARACTERS: &[char] = &['*', '?'];

/// One piece of a path word.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    /// Text that stands for itself.
    Text(String),
    /// The path of the importing script.
    File,
    /// A directory of the repository, or `None` for one that is outside it.
    Dir(Option<PathBuf>),
}

/// A path that has a value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Located {
    /// Anchored at the script directory: a repository path, possibly a glob pattern.
    Repo(PathBuf),
    /// Anchored at the script directory but outside the repository.
    Outside,
    /// Text only, relative to a working directory the scan cannot know.
    Literal(String),
}

/// The table entry. `reference.specifier` is the first argument of the command as written and
/// `reference.node` its syntax node.
pub fn resolve(
    reference: &Reference<'_>,
    importer: &Path,
    env: &Environment<'_>,
) -> Vec<Resolution> {
    let mut root = reference.node;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    let evaluator = Evaluator {
        source: reference.source,
        importer,
        root,
    };
    let located = evaluator
        .parts(reference.node, reference.node.start_byte())
        .and_then(|parts| located(&parts));
    let Some(located) = located else {
        return vec![Resolution::Skipped(SkipReason::DynamicSpecifier)];
    };
    match located {
        Located::Outside => vec![Resolution::Ignored],
        Located::Repo(path) => find(&[path], importer, env),
        Located::Literal(text) => {
            if text.starts_with(['/', '~']) {
                return vec![Resolution::Ignored];
            }
            let bases = [
                importer.parent().map(Path::to_path_buf),
                Some(PathBuf::new()),
            ];
            let candidates: Vec<PathBuf> = bases
                .iter()
                .filter_map(|base| join_normalised(base.as_deref(), &text))
                .collect();
            if candidates.is_empty() {
                return vec![Resolution::Ignored];
            }
            find(&candidates, importer, env)
        }
    }
}

/// Resolves the first of `candidates` that names a known file, as a pattern when it holds glob
/// characters. A path that names an asset is ignored, one that names nothing is unresolved.
fn find(candidates: &[PathBuf], importer: &Path, env: &Environment<'_>) -> Vec<Resolution> {
    for candidate in candidates {
        let written = candidate.to_string_lossy();
        if written.contains(GLOB_CHARACTERS) {
            let matched = glob_matches(&written, importer, env.known);
            if !matched.is_empty() {
                return matched.into_iter().map(Resolution::resolved).collect();
            }
        } else if env.known.contains(candidate) {
            return vec![Resolution::resolved(candidate.clone())];
        }
    }
    if candidates
        .iter()
        .any(|candidate| env.assets.contains(candidate))
    {
        return vec![Resolution::Ignored];
    }
    vec![Resolution::Unresolved]
}

/// The known files that the pattern `pattern` matches, in path order, without the importer.
fn glob_matches(pattern: &str, importer: &Path, known: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    known
        .iter()
        .filter(|file| file.as_path() != importer && glob::matches(pattern, file))
        .cloned()
        .collect()
}

/// The value of a path made of `parts`: `None` when it is dynamic.
fn located(parts: &[Part]) -> Option<Located> {
    let (anchor, rest) = match parts.split_first()? {
        (Part::Dir(dir), rest) => (Some(dir), rest),
        (_, _) => (None, parts),
    };
    let mut text = String::new();
    for part in rest {
        match part {
            Part::Text(piece) => text.push_str(piece),
            Part::File | Part::Dir(_) => return None,
        }
    }
    if text.contains(['[', '{']) || text.split('/').any(|segment| segment == "**") {
        return None;
    }
    let Some(dir) = anchor else {
        return (!text.is_empty()).then_some(Located::Literal(text));
    };
    if !text.is_empty() && !text.starts_with('/') {
        return None;
    }
    let Some(dir) = dir else {
        return Some(Located::Outside);
    };
    Some(
        join_normalised(Some(dir), text.trim_start_matches('/'))
            .map_or(Located::Outside, Located::Repo),
    )
}

/// The directory a path with a value names: a script-anchored one, never plain text.
fn directory(path: Located) -> Option<Part> {
    match path {
        Located::Repo(path) => Some(Part::Dir(Some(path))),
        Located::Outside => Some(Part::Dir(None)),
        Located::Literal(_) => None,
    }
}

/// Evaluates the syntax of one file.
struct Evaluator<'a> {
    source: &'a str,
    importer: &'a Path,
    /// The root of the syntax tree the reference is in, where assignments are looked for.
    root: Node<'a>,
}

impl<'a> Evaluator<'a> {
    fn text(&self, node: Node<'_>) -> Option<&'a str> {
        let source: &'a str = self.source;
        node.utf8_text(source.as_bytes()).ok()
    }

    /// The text of `node` when it holds no backslash.
    fn plain(&self, node: Node<'_>) -> Option<&'a str> {
        self.text(node).filter(|text| !text.contains('\\'))
    }

    /// The directory of the importing file.
    fn script_dir(&self) -> Part {
        Part::Dir(Some(
            self.importer
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        ))
    }

    /// The parts of a word, a string or a concatenation, with the assignments before byte `before`
    /// in scope. `None` when any part is dynamic.
    fn parts(&self, node: Node<'_>, before: usize) -> Option<Vec<Part>> {
        match node.kind() {
            "word" | "number" => Some(vec![Part::Text(self.plain(node)?.to_owned())]),
            "raw_string" => {
                let inner = self.text(node)?.strip_prefix('\'')?.strip_suffix('\'')?;
                Some(vec![Part::Text(inner.to_owned())])
            }
            "string" => {
                let mut cursor = node.walk();
                let mut parts = Vec::new();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "\"" => {}
                        "string_content" => parts.push(Part::Text(self.plain(child)?.to_owned())),
                        _ => parts.push(self.expansion(child, before)?),
                    }
                }
                Some(parts)
            }
            "concatenation" => {
                let mut cursor = node.walk();
                let mut parts = Vec::new();
                for child in node.named_children(&mut cursor) {
                    parts.extend(self.parts(child, before)?);
                }
                Some(parts)
            }
            _ => Some(vec![self.expansion(node, before)?]),
        }
    }

    /// The part a `$name`, `${name...}` or `$(...)` stands for.
    fn expansion(&self, node: Node<'_>, before: usize) -> Option<Part> {
        match node.kind() {
            "simple_expansion" => {
                let name = node
                    .named_child(0)
                    .filter(|child| child.kind() == "variable_name")?;
                self.variable(self.text(name)?, before)
            }
            "expansion" => self.braced(node, before),
            "command_substitution" => self.substitution(node, before),
            _ => None,
        }
    }

    /// `${name}`, `${BASH_SOURCE[0]}` and the directory forms `${0%/*}`.
    fn braced(&self, node: Node<'_>, before: usize) -> Option<Part> {
        let mut cursor = node.walk();
        let inner: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        let (first, rest) = inner.split_first()?;
        let name = match first.kind() {
            "variable_name" => self.text(*first)?,
            "subscript" => {
                let index = first.child_by_field_name("index")?;
                if self.text(index)? != "0" {
                    return None;
                }
                self.text(first.child_by_field_name("name")?)?
            }
            _ => return None,
        };
        match rest {
            [] => self.variable(name, before),
            [pattern] => {
                let operator = node.child_by_field_name("operator")?;
                (pattern.kind() == "regex"
                    && self.text(*pattern)? == "/*"
                    && self.text(operator)? == "%"
                    && SCRIPT_VARIABLES.contains(&name))
                .then(|| self.script_dir())
            }
            _ => None,
        }
    }

    /// The value of the variable `name` as it is when the command at byte `before` runs.
    fn variable(&self, name: &str, before: usize) -> Option<Part> {
        if SCRIPT_VARIABLES.contains(&name) {
            return Some(Part::File);
        }
        let assignment = self.assignment(name, before)?;
        let value = assignment.child_by_field_name("value")?;
        let parts = self.parts(value, assignment.start_byte())?;
        directory(located(&parts)?)
    }

    /// The last assignment of `name` that starts before byte `before`, anywhere in the file.
    fn assignment(&self, name: &str, before: usize) -> Option<Node<'a>> {
        let mut best: Option<Node<'a>> = None;
        let mut pending = vec![self.root];
        while let Some(node) = pending.pop() {
            // why: a node that starts at or after `before` holds nothing earlier.
            if node.start_byte() >= before {
                continue;
            }
            let named = node.kind() == "variable_assignment"
                && node
                    .child_by_field_name("name")
                    .and_then(|variable| self.text(variable))
                    == Some(name);
            if named && best.is_none_or(|found| node.start_byte() > found.start_byte()) {
                best = Some(node);
            }
            let mut cursor = node.walk();
            pending.extend(node.children(&mut cursor));
        }
        best
    }

    /// `$(dirname F)` and `$(cd P && pwd)`.
    fn substitution(&self, node: Node<'_>, before: usize) -> Option<Part> {
        let mut cursor = node.walk();
        let mut inner = node.named_children(&mut cursor);
        let body = inner.next()?;
        if inner.next().is_some() {
            return None;
        }
        match body.kind() {
            "command" => {
                let arguments = self.call(body, "dirname")?;
                let [file] = arguments.as_slice() else {
                    return None;
                };
                (self.parts(*file, before)? == [Part::File]).then(|| self.script_dir())
            }
            "list" => {
                let mut cursor = body.walk();
                let steps: Vec<Node<'_>> = body.children(&mut cursor).collect();
                let [change, and, print] = steps.as_slice() else {
                    return None;
                };
                if and.kind() != "&&" {
                    return None;
                }
                let flags = self.call(*print, "pwd")?;
                if flags
                    .iter()
                    .any(|flag| !matches!(self.text(*flag), Some("-P" | "-L")))
                {
                    return None;
                }
                let arguments = self.call(*change, "cd")?;
                let [target] = arguments.as_slice() else {
                    return None;
                };
                directory(located(&self.parts(*target, before)?)?)
            }
            _ => None,
        }
    }

    /// The arguments of `command` when it calls the program `name`, after a leading `--`.
    fn call<'t>(&self, command: Node<'t>, name: &str) -> Option<Vec<Node<'t>>> {
        if command.kind() != "command" {
            return None;
        }
        let callee = command.child_by_field_name("name")?;
        if self.text(callee)? != name {
            return None;
        }
        let mut cursor = command.walk();
        let mut arguments: Vec<Node<'t>> = command
            .children_by_field_name("argument", &mut cursor)
            .collect();
        if arguments
            .first()
            .and_then(|first| self.text(*first))
            .is_some_and(|first| first == "--")
        {
            arguments.remove(0);
        }
        Some(arguments)
    }
}
