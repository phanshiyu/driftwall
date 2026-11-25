//! The re-export table of a Rust file: its top-level `pub use` declarations and the items it
//! defines, see "Re-exports" in `docs/DESIGN.md`.

use std::path::Path;

use tree_sitter::{Node, Tree};

use super::env::Env;
use super::module::{Segment, Site, WrittenPath};
use super::resolve::through_bindings;
use super::scope::Bindings;
use super::tree::{self, use_leaves};
use crate::code::Symbol;
use crate::imports::syntax::resolution::Files;
use crate::imports::syntax::table::Table;

/// The item kinds whose `name` is a name of the module an import can use as a symbol. A `mod` is
/// not one: a module is looked up as a file, and `mod parse; pub use parse::parse;` re-exports a
/// function that shares its name.
const ITEMS: [&str; 8] = [
    "function_item",
    "struct_item",
    "enum_item",
    "union_item",
    "type_item",
    "const_item",
    "static_item",
    "trait_item",
];

/// The table of `file`, whose parsed text is `source`.
pub fn table(tree: &Tree, source: &str, file: &Path, files: &Files<'_>, env: &Env<'_>) -> Table {
    let mut table = Table::default();
    let root = tree.root_node();
    let bindings = Bindings::of_tree(root, source);
    let site = Site::of(file, &[], files.known, env.paths);
    let mut cursor = root.walk();
    for item in root.children(&mut cursor) {
        if item.kind() == "type_item"
            && let Some((name, target)) =
                alias(item, source, file, &bindings, site.as_ref(), files, env)
        {
            table
                .lines
                .insert(u32::try_from(item.start_position().row + 1).unwrap_or(u32::MAX));
            table.names.insert(name, target);
        } else if ITEMS.contains(&item.kind()) {
            let name = item
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok());
            table.local.extend(name.map(str::to_owned));
        } else if item.kind() == "use_declaration" && is_public(item) {
            let Some(argument) = item.child_by_field_name("argument") else {
                continue;
            };
            table
                .lines
                .insert(u32::try_from(item.start_position().row + 1).unwrap_or(u32::MAX));
            for leaf in use_leaves(argument, source) {
                let path = through_bindings(&leaf.path, &bindings, &[], site.as_ref(), files, env);
                let path = through_mod(path, site.as_ref(), files, env);
                let Some((target, symbol)) = path.target(site.as_ref(), files, env) else {
                    continue;
                };
                if target == file {
                    continue;
                }
                match symbol {
                    Symbol::All if path.is_glob() => table.stars.push(target),
                    // why: a module re-exported as a whole is no item, so it is not followed.
                    Symbol::All => {}
                    Symbol::Name(original) => {
                        // why: only a path of module then item names the item; deeper ones name an inline module's item.
                        if path.last_name() == Some(original.as_str())
                            && let Some(exported) = leaf.binding
                        {
                            table.names.insert(exported.to_owned(), (target, original));
                        }
                    }
                }
            }
        }
    }
    table
}

/// The (alias name, defining file and name) of a top-level `type X = P;` whose body is exactly a
/// path to an item of another repository file. Any visibility counts, and so do generic parameters
/// (`type X<T> = P<T>;`). `None` when the body is anything else, or names std, a third-party crate
/// or an item of `file` itself: the alias then defines a type of its own.
fn alias(
    item: Node<'_>,
    source: &str,
    file: &Path,
    bindings: &Bindings<'_>,
    site: Option<&Site>,
    files: &Files<'_>,
    env: &Env<'_>,
) -> Option<(String, (std::path::PathBuf, String))> {
    let text = |node: Node<'_>| node.utf8_text(source.as_bytes()).ok();
    let name = text(item.child_by_field_name("name")?)?;
    let written = tree::alias_body(item.child_by_field_name("type")?, source)?;
    // why: a bare name is an item the file imported, so it stands for the path its `use` wrote.
    let imported = match written.first_name() {
        Some(first) if written.last_name() == Some(first) => bindings.get(&[], first),
        _ => None,
    };
    let path = through_bindings(
        &imported.unwrap_or(written),
        bindings,
        &[],
        site,
        files,
        env,
    );
    let (target, symbol) = path.target(site, files, env)?;
    let Symbol::Name(original) = symbol else {
        return None;
    };
    // why: only a path of module then item names the item; deeper ones name an inline module's item.
    (target != file && path.last_name() == Some(original.as_str()))
        .then(|| (name.to_owned(), (target, original)))
}

/// `path` read from the module the file declares: a leading name that a `mod name;` of the file
/// declares is `self::name`, also when the file binds the name a second time (`mod parse;` with
/// `pub use parse::parse;`), which makes the binding ambiguous.
fn through_mod<'s>(
    path: WrittenPath<'s>,
    site: Option<&Site>,
    files: &Files<'_>,
    env: &Env<'_>,
) -> WrittenPath<'s> {
    let declared = path.first_name().filter(|name| {
        site.is_some_and(|site| site.declared_file(name, files.known, env.paths).is_some())
    });
    match declared {
        Some(name) => path.with_first_replaced(&WrittenPath::declared(vec![
            Segment::SelfModule,
            Segment::Name(name),
        ])),
        None => path,
    }
}

/// Whether the `use` declaration carries a visibility (`pub`, `pub(crate)`, `pub(super)`, ..).
fn is_public(declaration: Node<'_>) -> bool {
    let mut cursor = declaration.walk();
    declaration
        .children(&mut cursor)
        .any(|child| child.kind() == "visibility_modifier")
}
