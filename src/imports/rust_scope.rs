//! The names a Rust file brings into scope with `use` and `mod name;`, kept per module block.
//!
//! A name bound in a module is visible to paths written in that same module and nowhere else:
//! an inline `mod tests { }` does not inherit the `use` items of the file around it, exactly as
//! in Rust. Blocks that are not modules (function bodies, `impl` blocks) are not scopes here:
//! a `use` inside a function binds its name for the whole enclosing module. A name bound to two
//! different paths in one module is ambiguous and is not followed.
//!
//! The one inheritance is `use super::*;` in an inline module: the module then sees the names its
//! parent module block binds, see "Rust resolution" in `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Node;

use super::rust_module::{Segment, WrittenPath};
use super::rust_syntax::{enclosing_modules, use_leaves};

/// Names brought into scope by one file, per enclosing inline module chain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings<'s> {
    /// Inline module names, outermost first (empty for the file's own module), to each bound name
    /// and the path it stands for. `None` marks a name bound to more than one path.
    by_module: BTreeMap<Vec<&'s str>, BTreeMap<&'s str, Option<WrittenPath<'s>>>>,
    /// The inline module chains that hold a `use super::*;`.
    inherits: BTreeSet<Vec<&'s str>>,
}

impl<'s> Bindings<'s> {
    /// Collects every `use` leaf and every `mod name;` declaration below `root`.
    #[must_use]
    pub fn of_tree(root: Node<'_>, source: &'s str) -> Self {
        let mut bindings = Self::default();
        let mut cursor = root.walk();
        'walk: loop {
            let node = cursor.node();
            let is_use = node.kind() == "use_declaration";
            if is_use {
                bindings.add_use(node, source);
            } else if node.kind() == "mod_item" {
                bindings.add_mod(node, source);
            }
            // why: a `use` has nothing inside it that binds, so its subtree is skipped.
            if !is_use && cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    break 'walk;
                }
            }
        }
        bindings
    }

    /// The path `name` stands for in the module block `inline`, if exactly one path was bound,
    /// written as it reads inside `inline`. A name the module does not bind is looked up in its
    /// parent when the module has a `use super::*;`, and `use super::name;` stands for whatever
    /// the parent binds `name` to.
    #[must_use]
    pub fn get(&self, inline: &[&'s str], name: &str) -> Option<WrittenPath<'s>> {
        let parent = inline.split_last().map(|(_, parent)| parent);
        let inherited = |name: &str| {
            parent
                .and_then(|parent| self.get(parent, name))
                .map(|path| path.seen_from_child())
        };
        match self.by_module.get(inline).and_then(|names| names.get(name)) {
            Some(bound) => {
                let path = bound.as_ref()?;
                // why: `use super::depgraph;` re-imports the parent's binding, which a plain path cannot name.
                Some(
                    path.super_item()
                        .and_then(inherited)
                        .unwrap_or_else(|| path.clone()),
                )
            }
            None if self.inherits.contains(inline) => inherited(name),
            None => None,
        }
    }

    fn add_use(&mut self, node: Node<'_>, source: &'s str) {
        let Some(argument) = node.child_by_field_name("argument") else {
            return;
        };
        let inline = enclosing_modules(node, source);
        for leaf in use_leaves(argument, source) {
            if leaf.path.is_super_glob() && !inline.is_empty() {
                self.inherits.insert(inline.clone());
            } else if let Some(name) = leaf.binding {
                self.bind(&inline, name, leaf.path);
            }
        }
    }

    /// `mod name;` binds `name` to `self::name`, the file or directory module next to this one.
    fn add_mod(&mut self, node: Node<'_>, source: &'s str) {
        if node.child_by_field_name("body").is_some() {
            return;
        }
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| name.utf8_text(source.as_bytes()).ok())
        else {
            return;
        };
        let inline = enclosing_modules(node, source);
        let path = WrittenPath::declared(vec![Segment::SelfModule, Segment::Name(name)]);
        self.bind(&inline, name, path);
    }

    fn bind(&mut self, inline: &[&'s str], name: &'s str, path: WrittenPath<'s>) {
        let names = self.by_module.entry(inline.to_vec()).or_default();
        match names.get(name) {
            Some(Some(existing)) if *existing == path => {}
            Some(_) => {
                names.insert(name, None);
            }
            None => {
                names.insert(name, Some(path));
            }
        }
    }
}
