//! Reads the paths a Rust syntax node names, as typed [`WrittenPath`] values.

use tree_sitter::Node;

use super::module::{Segment, WrittenPath};

/// The paths the node captured by the Rust query stands for.
///
/// A `use` tree expands to one path per leaf. A scoped path expands to itself when it is the
/// outermost one and sits outside a `use` declaration and a visibility modifier. A token inside a
/// macro argument list expands to the `a::b::c` path it starts, because macro arguments are not
/// parsed into expressions.
pub fn paths<'s>(node: Node<'_>, source: &'s str) -> Vec<WrittenPath<'s>> {
    match node.parent().map(|parent| parent.kind()) {
        Some("use_declaration") => use_leaves(node, source)
            .into_iter()
            .map(|leaf| leaf.path)
            .collect(),
        Some("token_tree") => token_path(node, source).into_iter().collect(),
        _ => scoped_path(node, source).into_iter().collect(),
    }
}

/// The path a type alias body names, generic arguments dropped: `a::B`, `a::B<T>` and a bare `B`
/// (a one-segment path). `None` for any other type: a reference, tuple, function or trait object.
pub fn alias_body<'s>(body: Node<'_>, source: &'s str) -> Option<WrittenPath<'s>> {
    let mut segments = Vec::new();
    match body.kind() {
        "type_identifier" | "scoped_type_identifier" | "generic_type" => {
            flatten(body, source, &mut segments)?;
        }
        _ => return None,
    }
    Some(WrittenPath::in_code(segments))
}

/// Names of the inline `mod name { }` blocks around `node`, outermost first.
pub fn enclosing_modules<'s>(node: Node<'_>, source: &'s str) -> Vec<&'s str> {
    let mut names = Vec::new();
    let mut child = node;
    while let Some(parent) = child.parent() {
        if parent.kind() == "mod_item" && parent.child_by_field_name("body") == Some(child) {
            let name = parent
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok());
            names.extend(name);
        }
        child = parent;
    }
    names.reverse();
    names
}

fn scoped_path<'s>(node: Node<'_>, source: &'s str) -> Option<WrittenPath<'s>> {
    let is_scoped = matches!(node.kind(), "scoped_identifier" | "scoped_type_identifier");
    if !is_scoped
        || continues_outward(node)
        || has_ancestor(node, "use_declaration")
        || has_ancestor(node, "visibility_modifier")
    {
        return None;
    }
    let mut segments = Vec::new();
    flatten(node, source, &mut segments)?;
    Some(WrittenPath::in_code(segments))
}

/// Whether `node` is the leading part of a longer path, so only the longer path counts.
fn continues_outward(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "scoped_identifier" | "scoped_type_identifier" => {
            parent.child_by_field_name("path") == Some(node)
        }
        "generic_type" | "generic_type_with_turbofish" => {
            parent.child_by_field_name("type") == Some(node) && continues_outward(parent)
        }
        _ => false,
    }
}

fn has_ancestor(node: Node<'_>, kind: &str) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == kind {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

/// Appends the segments of a path node to `out`. `None` means the path has a part with no module
/// meaning, such as `<T as Trait>::f` or a leading `::`.
fn flatten<'s>(node: Node<'_>, source: &'s str, out: &mut Vec<Segment<'s>>) -> Option<()> {
    match node.kind() {
        "scoped_identifier" | "scoped_type_identifier" => {
            flatten(node.child_by_field_name("path")?, source, out)?;
            flatten(node.child_by_field_name("name")?, source, out)
        }
        "generic_type" | "generic_type_with_turbofish" => {
            flatten(node.child_by_field_name("type")?, source, out)
        }
        _ => {
            out.push(segment(node, source)?);
            Some(())
        }
    }
}

fn segment<'s>(node: Node<'_>, source: &'s str) -> Option<Segment<'s>> {
    match node.kind() {
        "crate" => Some(Segment::Crate),
        "self" => Some(Segment::SelfModule),
        "super" => Some(Segment::Super),
        "identifier" | "type_identifier" => {
            let text = node.utf8_text(source.as_bytes()).ok()?;
            Some(Segment::Name(text.trim_start_matches("r#")))
        }
        _ => None,
    }
}

/// One leaf of a `use` tree: the full path it names and the name it brings into scope, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseLeaf<'s> {
    /// The path as written, with enclosing groups already prefixed.
    pub path: WrittenPath<'s>,
    /// The name the leaf binds in the importing module: its alias, else its last segment. `None`
    /// for a glob, for `as _`, and for a leaf that ends in `crate`, `self` or `super`.
    pub binding: Option<&'s str>,
}

/// The leaves of the `use` argument `node`, in source order.
pub fn use_leaves<'s>(node: Node<'_>, source: &'s str) -> Vec<UseLeaf<'s>> {
    let mut found = Vec::new();
    expand_use(node, source, &[], None, &mut found);
    found
}

fn leaf<'s>(segments: Vec<Segment<'s>>, alias: Option<&'s str>) -> UseLeaf<'s> {
    let path = WrittenPath::declared(segments);
    let binding = match alias {
        Some("_") => None,
        Some(name) => Some(name),
        None => path.last_name(),
    };
    UseLeaf { path, binding }
}

/// Expands one node of a `use` tree below `prefix` into each leaf. `alias` is the `as` name that
/// applies to the leaf this node is, when the node sits inside a `use_as_clause`.
fn expand_use<'s>(
    node: Node<'_>,
    source: &'s str,
    prefix: &[Segment<'s>],
    alias: Option<&'s str>,
    out: &mut Vec<UseLeaf<'s>>,
) {
    match node.kind() {
        "scoped_use_list" => {
            let mut extended = prefix.to_vec();
            if let Some(path) = node.child_by_field_name("path")
                && flatten(path, source, &mut extended).is_none()
            {
                return;
            }
            if let Some(list) = node.child_by_field_name("list") {
                expand_use(list, source, &extended, None, out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                expand_use(child, source, prefix, None, out);
            }
        }
        "use_as_clause" => {
            let alias = node
                .child_by_field_name("alias")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok());
            if let Some(path) = node.child_by_field_name("path") {
                expand_use(path, source, prefix, alias, out);
            }
        }
        "use_wildcard" => {
            let mut inner = Vec::new();
            match node.named_child(0) {
                Some(path) => expand_use(path, source, prefix, None, &mut inner),
                None => inner.push(leaf(prefix.to_vec(), None)),
            }
            // why: a glob binds no name, whatever its path ends in, and uses every item of it.
            out.extend(inner.into_iter().map(|found| UseLeaf {
                path: found.path.into_glob(),
                binding: None,
            }));
        }
        // why: `self` inside a group names the group's own path, as in `use a::b::{self, c}`.
        "self" if !prefix.is_empty() => out.push(leaf(prefix.to_vec(), alias)),
        _ => {
            let mut full = prefix.to_vec();
            if flatten(node, source, &mut full).is_some() {
                out.push(leaf(full, alias));
            }
        }
    }
}

/// The `a::b::c` path that starts at token `node` inside a macro argument list, if any.
fn token_path<'s>(node: Node<'_>, source: &'s str) -> Option<WrittenPath<'s>> {
    if node.prev_sibling().is_some_and(|prev| prev.kind() == "::") {
        return None;
    }
    let mut segments = vec![segment(node, source)?];
    let mut last = node;
    while let Some(separator) = last.next_sibling()
        && separator.kind() == "::"
        && let Some(next) = separator.next_sibling()
        && let Some(found) = segment(next, source)
    {
        segments.push(found);
        last = next;
    }
    (segments.len() > 1).then(|| WrittenPath::in_code(segments))
}
