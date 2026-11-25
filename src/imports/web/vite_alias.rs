//! The `resolve.alias` entries of a Vite config, see "Vite aliases" in `docs/DESIGN.md`.
//!
//! The IO shell is [`vite_aliases`]; [`configured_aliases`] is the pure reading of one config. A
//! config is never run: only the literal forms of the Vite root reader are understood.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use super::vite::{
    CONFIG_FILES, MAX_ALIASES, Reader, Value, config_object, declared, object_of, option_value,
    parse,
};
use crate::code::Language;
use crate::imports::syntax::literal::literal_text;

/// One alias of a Vite config: the specifier prefix it replaces and the repository path it stands
/// for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViteAlias {
    /// The specifier, or the start of one, that the alias matches.
    pub key: String,
    /// The repository-relative path that replaces the key.
    pub target: PathBuf,
}

impl ViteAlias {
    /// The part of `specifier` after the key when the alias matches it: the whole key, or the key
    /// followed by a `/`. `Some("")` is an exact match.
    #[must_use]
    pub fn rest<'s>(&self, specifier: &'s str) -> Option<&'s str> {
        let rest = specifier.strip_prefix(self.key.as_str())?;
        (rest.is_empty() || rest.starts_with('/') || self.key.ends_with('/')).then_some(rest)
    }
}

/// Maps each directory that holds a Vite config to the aliases that config gives, reading from
/// under `root`. Both are repository-relative, with the repository root as the empty path.
///
/// Only a config that is a known file counts, the first in Vite's own order when a directory holds
/// several. A config that cannot be read or has no recognised alias gives no entry.
#[must_use]
pub fn vite_aliases(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> BTreeMap<PathBuf, Vec<ViteAlias>> {
    let mut configs: BTreeMap<PathBuf, (usize, &Path)> = BTreeMap::new();
    for path in files.keys() {
        let rank = path
            .file_name()
            .and_then(|name| CONFIG_FILES.iter().position(|config| name == *config));
        if let (Some(rank), Some(dir)) = (rank, path.parent()) {
            let best = configs.entry(dir.to_path_buf()).or_insert((rank, path));
            if rank < best.0 {
                *best = (rank, path);
            }
        }
    }
    configs
        .into_iter()
        .filter_map(|(dir, (_, config))| {
            let text = fs::read_to_string(root.join(config)).ok()?;
            let aliases = configured_aliases(&text, config, &dir);
            (!aliases.is_empty()).then_some((dir, aliases))
        })
        .collect()
}

/// The aliases of the config `text`, the file `config` in the directory `dir`, in the order
/// written. Empty when there is no `resolve.alias` or it is of no recognised form.
#[must_use]
pub fn configured_aliases(text: &str, config: &Path, dir: &Path) -> Vec<ViteAlias> {
    let Some(tree) = parse(text, config) else {
        return Vec::new();
    };
    let program = tree.root_node();
    let reader = Reader { source: text, dir };
    let alias = config_object(program, text)
        .and_then(|object| option_value(object, "resolve", text))
        .and_then(|resolve| object_of(resolve, program, text, MAX_ALIASES))
        .and_then(|resolve| option_value(resolve, "alias", text))
        .and_then(|alias| plain(alias, program, text));
    let Some(alias) = alias else {
        return Vec::new();
    };
    let mut cursor = alias.walk();
    let entries = alias.named_children(&mut cursor);
    match alias.kind() {
        "object" => entries
            .filter(|pair| pair.kind() == "pair")
            .filter_map(|pair| {
                let key = pair.child_by_field_name("key")?;
                let key = match key.kind() {
                    "property_identifier" | "identifier" => key.utf8_text(text.as_bytes()).ok()?,
                    _ => literal_text(key, text)?,
                };
                alias_of(key, pair.child_by_field_name("value")?, &reader)
            })
            .collect(),
        "array" => entries
            .filter(|entry| entry.kind() == "object")
            .filter_map(|entry| {
                let find = literal_text(option_value(entry, "find", text)?, text)?;
                alias_of(find, option_value(entry, "replacement", text)?, &reader)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The alias `key` stands for the path `value` names, when `value` is one of the recognised forms.
fn alias_of(key: &str, value: Node<'_>, reader: &Reader<'_>) -> Option<ViteAlias> {
    match reader.value(value)? {
        Value::Text(target) | Value::Dir(target) => Some(ViteAlias {
            key: key.to_owned(),
            target,
        }),
        Value::File | Value::Url(_) => None,
    }
}

/// `node` without its wrappers (parentheses, `as`, `satisfies`), looking up the top level `const`
/// an identifier names.
fn plain<'t>(node: Node<'t>, program: Node<'t>, source: &str) -> Option<Node<'t>> {
    match node.kind() {
        "parenthesized_expression"
        | "as_expression"
        | "satisfies_expression"
        | "non_null_expression" => plain(node.named_child(0)?, program, source),
        "identifier" => {
            let name = node.utf8_text(source.as_bytes()).ok()?;
            plain(declared(program, name, source)?, program, source)
        }
        _ => Some(node),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(text: &str) -> Vec<(String, String)> {
        configured_aliases(text, Path::new("app/vite.config.ts"), Path::new("app"))
            .into_iter()
            .map(|alias| (alias.key, alias.target.display().to_string()))
            .collect()
    }

    fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(key, target)| ((*key).to_owned(), (*target).to_owned()))
            .collect()
    }

    #[test]
    fn the_object_form_reads_literal_and_identifier_keys_in_order() {
        let text = "export default { resolve: { alias: { 'mrc-wasm': './pkg', lib: '../lib', \"q\": 'q' } } };";
        assert_eq!(
            aliases(text),
            pairs(&[("mrc-wasm", "app/pkg"), ("lib", "lib"), ("q", "app/q")])
        );
    }

    #[test]
    fn the_array_form_reads_find_and_replacement() {
        let text = "export default defineConfig({ resolve: { alias: [{ find: 'x', replacement: './x' }, { find: /^y/, replacement: './y' }, { find: 'z', replacement: './z' }] } });";
        assert_eq!(aliases(text), pairs(&[("x", "app/x"), ("z", "app/z")]));
    }

    #[test]
    fn the_path_helpers_of_the_root_reader_are_understood() {
        let text = "import path from 'path';\nexport default defineConfig({ resolve: { alias: {\n a: path.resolve(__dirname, 'engine/wasm/pkg'),\n b: path.join(__dirname, 'src', '..', 'lib'),\n c: fileURLToPath(new URL('./src', import.meta.url)),\n d: __dirname,\n e: new URL('./u', import.meta.url).pathname,\n} } });";
        assert_eq!(
            aliases(text),
            pairs(&[
                ("a", "app/engine/wasm/pkg"),
                ("b", "app/lib"),
                ("c", "app/src"),
                ("d", "app"),
                ("e", "app/u"),
            ])
        );
    }

    #[test]
    fn unrecognised_values_and_other_shapes_give_nothing() {
        for text in [
            "export default { resolve: { alias: { a: path.resolve('/abs'), b: '/abs' } } };",
            "export default { alias: { a: './a' } };",
            "export default { resolve: { alias: makeAliases() } };",
            "export default { resolve: {} };",
            "export default {};",
            "nothing",
        ] {
            assert_eq!(aliases(text), Vec::new(), "{text}");
        }
    }

    #[test]
    fn a_const_object_and_a_function_config_are_followed() {
        let text = "const alias = { a: './a' };\nexport default defineConfig(({ mode }) => ({ resolve: { alias: alias } }));";
        assert_eq!(aliases(text), pairs(&[("a", "app/a")]));
    }

    #[test]
    fn an_alias_matches_its_key_and_what_is_below_it() {
        let alias = ViteAlias {
            key: "mrc-wasm".to_owned(),
            target: PathBuf::from("pkg"),
        };
        assert_eq!(alias.rest("mrc-wasm"), Some(""));
        assert_eq!(alias.rest("mrc-wasm/x"), Some("/x"));
        assert_eq!(alias.rest("mrc-wasm-extra"), None);
        assert_eq!(alias.rest("other"), None);
    }
}
