//! The Vite root of an HTML file, read from the `vite.config.*` file above it.
//!
//! The IO shell is [`vite_roots`]; [`configured_root`] is the pure reading of one config. A config
//! is never run: its `root` option is read from the syntax tree when it has one of the forms listed
//! in "Svelte and HTML" in `docs/DESIGN.md`, and anything else means the directory of the config.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use super::spec::{JAVASCRIPT_GRAMMAR, TYPESCRIPT_GRAMMAR};
use crate::imports::syntax::literal::literal_text;
use crate::imports::syntax::paths::directories;
use crate::imports::syntax::paths::join_normalised;
use crate::imports::syntax::spec::Grammar;
use crate::model::Language;

/// Vite config file names in the order Vite itself looks for them in one directory.
pub(super) const CONFIG_FILES: [&str; 6] = [
    "vite.config.js",
    "vite.config.mjs",
    "vite.config.ts",
    "vite.config.cjs",
    "vite.config.mts",
    "vite.config.cts",
];

/// The most `const` names followed from `export default name` to the object it holds.
pub(super) const MAX_ALIASES: usize = 8;

/// Maps each directory that holds a Vite config to the root that config gives, reading from under
/// `root`. Both are repository-relative, with the repository root as the empty path.
///
/// Only the directories at or above an HTML file in `files` are looked at, and only a config that
/// is itself a known file counts. A config that cannot be read still makes its directory the root.
#[must_use]
pub fn vite_roots(root: &Path, files: &BTreeMap<PathBuf, Language>) -> BTreeMap<PathBuf, PathBuf> {
    let pages = files
        .iter()
        .filter(|(_, language)| **language == Language::Html)
        .map(|(path, _)| path.as_path());
    directories(pages)
        .into_iter()
        .filter_map(|dir| {
            let config = CONFIG_FILES
                .iter()
                .map(|name| dir.join(name))
                .find(|path| files.contains_key(path))?;
            let configured = fs::read_to_string(root.join(&config))
                .ok()
                .and_then(|text| configured_root(&text, &config, &dir));
            Some((dir.clone(), configured.unwrap_or(dir)))
        })
        .collect()
}

/// The syntax tree of the config `text`, the file `config`, parsed with the grammar its extension
/// names. `None` when the grammar cannot be loaded or the parse fails.
pub(super) fn parse(text: &str, config: &Path) -> Option<tree_sitter::Tree> {
    let typescript = config
        .extension()
        .is_some_and(|extension| matches!(extension.to_str(), Some("ts" | "mts" | "cts")));
    let grammar: &Grammar = if typescript {
        &TYPESCRIPT_GRAMMAR
    } else {
        &JAVASCRIPT_GRAMMAR
    };
    let mut parser = Parser::new();
    parser.set_language(&(grammar.language)()).ok()?;
    parser.parse(text, None)
}

/// The directory the `root` option of the config `text`, the file `config` in the directory `dir`,
/// names, or `None` when there is no option or it is not of a recognised form.
#[must_use]
pub fn configured_root(text: &str, config: &Path, dir: &Path) -> Option<PathBuf> {
    let tree = parse(text, config)?;
    let program = tree.root_node();
    let object = config_object(program, text)?;
    let value = option_value(object, "root", text)?;
    let reader = Reader { source: text, dir };
    match reader.value(value)? {
        Value::Text(path) | Value::Dir(path) => Some(path),
        Value::File | Value::Url(_) => None,
    }
}

/// What an expression of a config evaluates to.
pub(super) enum Value {
    /// A string literal, joined onto the config directory when it is a path.
    Text(PathBuf),
    /// A directory, repository-relative.
    Dir(PathBuf),
    /// The config file itself, as a path or a URL.
    File,
    /// `new URL(literal, import.meta.url)`: the repository-relative path it names.
    Url(PathBuf),
}

/// Evaluates the few expressions that name a directory relative to the config file.
pub(super) struct Reader<'a> {
    /// The text of the config.
    pub source: &'a str,
    /// The repository-relative directory of the config.
    pub dir: &'a Path,
}

impl Reader<'_> {
    fn text(&self, node: Node<'_>) -> Option<&str> {
        node.utf8_text(self.source.as_bytes()).ok()
    }

    /// The value of `node`, or `None` when it is not one of the recognised forms.
    pub(super) fn value(&self, node: Node<'_>) -> Option<Value> {
        match node.kind() {
            "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "non_null_expression" => self.value(node.named_child(0)?),
            "string" | "template_string" => {
                let literal = literal_text(node, self.source)?;
                Some(Value::Text(join_normalised(Some(self.dir), literal)?))
            }
            "identifier" => match self.text(node)? {
                "__dirname" => Some(Value::Dir(self.dir.to_path_buf())),
                "__filename" => Some(Value::File),
                _ => None,
            },
            "member_expression" => self.member(node),
            "call_expression" => self.call(node),
            "new_expression" => self.url(node),
            _ => None,
        }
    }

    fn member(&self, node: Node<'_>) -> Option<Value> {
        match self.text(node)? {
            "import.meta.dirname" => return Some(Value::Dir(self.dir.to_path_buf())),
            "import.meta.filename" | "import.meta.url" => return Some(Value::File),
            _ => {}
        }
        let property = self.text(node.child_by_field_name("property")?)?;
        match (property, self.value(node.child_by_field_name("object")?)?) {
            ("pathname", Value::Url(path)) => Some(Value::Dir(path)),
            _ => None,
        }
    }

    fn call(&self, node: Node<'_>) -> Option<Value> {
        let callee = node.child_by_field_name("function")?;
        let name = match callee.kind() {
            "member_expression" => self.text(callee.child_by_field_name("property")?)?,
            _ => self.text(callee)?,
        };
        let arguments = node.child_by_field_name("arguments")?;
        let mut cursor = arguments.walk();
        let mut arguments = arguments.named_children(&mut cursor);
        if self.text(callee)? == "process.cwd" && arguments.next().is_none() {
            return Some(Value::Dir(self.dir.to_path_buf()));
        }
        let first = self.value(arguments.next()?)?;
        match (name, first) {
            ("resolve" | "join", Value::Dir(mut path) | Value::Text(mut path)) => {
                for argument in arguments {
                    let literal = literal_text(argument, self.source)?;
                    if literal.starts_with('/') {
                        return None;
                    }
                    path = join_normalised(Some(&path), literal)?;
                }
                Some(Value::Dir(path))
            }
            ("dirname", Value::File) => Some(Value::Dir(self.dir.to_path_buf())),
            ("dirname", Value::Dir(path)) => {
                let parent = path.parent()?.to_path_buf();
                arguments.next().is_none().then_some(Value::Dir(parent))
            }
            ("fileURLToPath", Value::File) => Some(Value::File),
            ("fileURLToPath", Value::Url(path)) => Some(Value::Dir(path)),
            _ => None,
        }
    }

    /// `new URL('<literal>', import.meta.url)`.
    fn url(&self, node: Node<'_>) -> Option<Value> {
        if self.text(node.child_by_field_name("constructor")?)? != "URL" {
            return None;
        }
        let arguments = node.child_by_field_name("arguments")?;
        let mut cursor = arguments.walk();
        let mut arguments = arguments.named_children(&mut cursor);
        let literal = arguments.next()?;
        let base = arguments.next()?;
        if !matches!(self.value(base)?, Value::File) {
            return None;
        }
        let literal = literal_text(literal, self.source)?;
        Some(Value::Url(join_normalised(Some(self.dir), literal)?))
    }
}

/// The object that holds the options of the config: what `export default` or `module.exports =`
/// gives, looking through the wrappers the module documentation lists.
pub(super) fn config_object<'t>(program: Node<'t>, source: &str) -> Option<Node<'t>> {
    let mut cursor = program.walk();
    let exported =
        program
            .named_children(&mut cursor)
            .find_map(|statement| match statement.kind() {
                "export_statement" => statement.child_by_field_name("value"),
                "expression_statement" => {
                    let assignment = statement.named_child(0)?;
                    let left = assignment.child_by_field_name("left")?;
                    let is_exports = left.utf8_text(source.as_bytes()).ok()? == "module.exports";
                    (assignment.kind() == "assignment_expression" && is_exports)
                        .then(|| assignment.child_by_field_name("right"))
                        .flatten()
                }
                _ => None,
            })?;
    object_of(exported, program, source, MAX_ALIASES)
}

/// The object `node` stands for: itself, what a wrapper holds, the first object or function
/// argument of a call, what a function returns, or the initializer of a top level `const`.
pub(super) fn object_of<'t>(
    node: Node<'t>,
    program: Node<'t>,
    source: &str,
    aliases: usize,
) -> Option<Node<'t>> {
    match node.kind() {
        "object" => Some(node),
        "parenthesized_expression"
        | "as_expression"
        | "satisfies_expression"
        | "non_null_expression" => object_of(node.named_child(0)?, program, source, aliases),
        "call_expression" => {
            let arguments = node.child_by_field_name("arguments")?;
            let mut cursor = arguments.walk();
            arguments
                .named_children(&mut cursor)
                .find_map(|argument| object_of(argument, program, source, aliases))
        }
        "arrow_function" | "function_expression" | "function" => {
            let body = node.child_by_field_name("body")?;
            if body.kind() == "statement_block" {
                let mut cursor = body.walk();
                let returned = body
                    .named_children(&mut cursor)
                    .find(|statement| statement.kind() == "return_statement")?
                    .named_child(0)?;
                object_of(returned, program, source, aliases)
            } else {
                object_of(body, program, source, aliases)
            }
        }
        "identifier" => {
            let name = node.utf8_text(source.as_bytes()).ok()?;
            let value = declared(program, name, source)?;
            object_of(value, program, source, aliases.checked_sub(1)?)
        }
        _ => None,
    }
}

/// The initializer of the top level `const`, `let` or `var` named `name`.
pub(super) fn declared<'t>(program: Node<'t>, name: &str, source: &str) -> Option<Node<'t>> {
    let mut cursor = program.walk();
    program
        .named_children(&mut cursor)
        .filter(|statement| {
            matches!(
                statement.kind(),
                "lexical_declaration" | "variable_declaration"
            )
        })
        .find_map(|statement| {
            let mut inner = statement.walk();
            statement
                .named_children(&mut inner)
                .filter(|declarator| declarator.kind() == "variable_declarator")
                .find_map(|declarator| {
                    let declared = declarator.child_by_field_name("name")?;
                    (declared.utf8_text(source.as_bytes()).ok()? == name)
                        .then(|| declarator.child_by_field_name("value"))
                        .flatten()
                })
        })
}

/// The value of the key `key` written directly in `object`.
pub(super) fn option_value<'t>(object: Node<'t>, key: &str, source: &str) -> Option<Node<'t>> {
    let mut cursor = object.walk();
    object.named_children(&mut cursor).find_map(|pair| {
        if pair.kind() != "pair" {
            return None;
        }
        let name = pair.child_by_field_name("key")?;
        let written = match name.kind() {
            "property_identifier" => name.utf8_text(source.as_bytes()).ok()?,
            _ => literal_text(name, source)?,
        };
        (written == key)
            .then(|| pair.child_by_field_name("value"))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(config: &str, text: &str) -> Option<String> {
        let path = Path::new(config);
        configured_root(text, path, path.parent()?).map(|root| root.display().to_string())
    }

    #[test]
    fn a_string_literal_is_relative_to_the_config_directory() {
        for (text, expected) in [
            ("export default { root: 'ui' };", "app/ui"),
            ("export default { root: './ui/' };", "app/ui"),
            ("export default { root: \"..\" };", ""),
            ("export default { root: '.' };", "app"),
            ("export default { 'root': `web` };", "app/web"),
        ] {
            assert_eq!(
                root("app/vite.config.ts", text).as_deref(),
                Some(expected),
                "{text}"
            );
        }
    }

    #[test]
    fn the_directory_forms_name_the_config_directory() {
        for text in [
            "export default { root: __dirname };",
            "export default { root: import.meta.dirname };",
            "export default { root: path.resolve(__dirname) };",
            "export default { root: path.dirname(fileURLToPath(import.meta.url)) };",
            "export default { root: dirname(__filename) };",
            "export default { root: process.cwd() };",
            "export default { root: fileURLToPath(new URL('.', import.meta.url)) };",
        ] {
            assert_eq!(
                root("ui/vite.config.ts", text).as_deref(),
                Some("ui"),
                "{text}"
            );
        }
    }

    #[test]
    fn resolve_join_and_url_forms_join_their_literals() {
        for (text, expected) in [
            (
                "export default { root: path.resolve(__dirname, 'web') };",
                "ui/web",
            ),
            (
                "export default { root: resolve(__dirname, './a', 'b') };",
                "ui/a/b",
            ),
            (
                "export default { root: path.join(__dirname, '..', 'site') };",
                "site",
            ),
            (
                "export default { root: fileURLToPath(new URL('./web', import.meta.url)) };",
                "ui/web",
            ),
            (
                "export default { root: new URL('./web', import.meta.url).pathname };",
                "ui/web",
            ),
        ] {
            assert_eq!(
                root("ui/vite.config.ts", text).as_deref(),
                Some(expected),
                "{text}"
            );
        }
    }

    #[test]
    fn the_config_object_is_found_through_the_usual_wrappers() {
        for text in [
            "export default defineConfig({ root: 'web' });",
            "export default defineConfig(() => ({ root: 'web' }));",
            "export default defineConfig(({ mode }) => { return { root: 'web' }; });",
            "export default { root: 'web' } satisfies UserConfig;",
            "const config = defineConfig({ root: 'web' });\nexport default config;",
            "module.exports = { root: 'web' };",
        ] {
            assert_eq!(
                root("ui/vite.config.js", text).as_deref(),
                Some("ui/web"),
                "{text}"
            );
        }
    }

    #[test]
    fn anything_else_gives_no_root() {
        for text in [
            "export default {};",
            "export default { root: path.resolve(__dirname, name) };",
            "export default { root: path.resolve(__dirname, '/abs') };",
            "export default { root: path.resolve('/abs') };",
            "export default { root: '/abs' };",
            "export default { root: '../../../out' };",
            "export default { root };",
            "export default { test: { root: 'web' } };",
            "export default defineConfig(base);",
            "const a = b;\nconst b = a;\nexport default a;",
            "export default {",
        ] {
            assert_eq!(root("ui/vite.config.ts", text), None, "{text}");
        }
    }
}
