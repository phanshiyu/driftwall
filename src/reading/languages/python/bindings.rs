//! What the names of a Python file are bound to, for the dynamic-import calls.

use std::collections::BTreeMap;

use tree_sitter::Node;

/// What a name stands for, as far as the dynamic-import calls care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// `__import__`.
    DunderImport,
    /// `importlib.import_module`.
    ImportModule,
    /// The module `importlib`.
    Importlib,
    /// `runpy.run_module`.
    RunModule,
    /// `runpy.run_path`.
    RunPath,
    /// The module `runpy`.
    Runpy,
    /// Anything else, or a name bound in more than one way.
    Other,
}

/// The names the file binds with an `import` statement, a `def` or a `class`, wherever they are
/// written. The names `importlib`, `runpy` and `__import__` stay bound to themselves unless the
/// file binds them to something else.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    names: BTreeMap<String, Bound>,
}

impl Bindings {
    /// The bindings of the tree rooted at `root`, parsed from `source`.
    #[must_use]
    pub fn of_tree(root: Node<'_>, source: &str) -> Self {
        let mut bindings = Self::default();
        let mut cursor = root.walk();
        'walk: loop {
            bindings.visit(cursor.node(), source);
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    break 'walk;
                }
            }
        }
        for (name, bound) in [
            ("__import__", Bound::DunderImport),
            ("importlib", Bound::Importlib),
            ("runpy", Bound::Runpy),
        ] {
            bindings.names.entry(name.to_owned()).or_insert(bound);
        }
        bindings
    }

    /// What `name` is bound to, `None` when the file never binds it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Bound> {
        self.names.get(name).copied()
    }

    fn bind(&mut self, name: &str, bound: Bound) {
        // why: a name bound two different ways (a fallback `def`, a rebinding) is not trusted.
        let kept = self.names.entry(name.to_owned()).or_insert(bound);
        if *kept != bound {
            *kept = Bound::Other;
        }
    }

    fn visit(&mut self, node: Node<'_>, source: &str) {
        match node.kind() {
            "import_statement" => self.visit_import(node, source),
            "import_from_statement" => self.visit_from(node, source),
            "function_definition" | "class_definition" if !in_class_body(node) => {
                if let Some(name) = field_text(node, "name", source) {
                    self.bind(name, Bound::Other);
                }
            }
            _ => {}
        }
    }

    /// `import a.b` binds `a`; `import a.b as c` binds `c` to `a.b`.
    fn visit_import(&mut self, statement: Node<'_>, source: &str) {
        let mut cursor = statement.walk();
        for item in statement.children_by_field_name("name", &mut cursor) {
            let aliased = item.kind() == "aliased_import";
            let (module, bound_as) = if aliased {
                let module = field_text(item, "name", source).unwrap_or_default();
                (module, field_text(item, "alias", source))
            } else {
                let module = text(item, source);
                (module, module.split('.').next())
            };
            let Some(name) = bound_as else { continue };
            let bound = match module {
                "importlib" => Bound::Importlib,
                "runpy" => Bound::Runpy,
                // why: `import importlib.util` binds the package `importlib`.
                _ if !aliased && name == "importlib" => Bound::Importlib,
                _ => Bound::Other,
            };
            self.bind(name, bound);
        }
    }

    /// `from m import a, b as c` binds `a` and `c` to what `m` holds under those names.
    fn visit_from(&mut self, statement: Node<'_>, source: &str) {
        let module = field_text(statement, "module_name", source).unwrap_or_default();
        let mut cursor = statement.walk();
        for item in statement.children_by_field_name("name", &mut cursor) {
            let (original, bound_as) = if item.kind() == "aliased_import" {
                (
                    field_text(item, "name", source).unwrap_or_default(),
                    field_text(item, "alias", source),
                )
            } else {
                (text(item, source), Some(text(item, source)))
            };
            let Some(name) = bound_as else { continue };
            let bound = match (module, original) {
                ("importlib", "import_module") => Bound::ImportModule,
                ("runpy", "run_module") => Bound::RunModule,
                ("runpy", "run_path") => Bound::RunPath,
                _ => Bound::Other,
            };
            self.bind(name, bound);
        }
    }
}

fn text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    node.utf8_text(source.as_bytes()).unwrap_or_default()
}

fn field_text<'s>(node: Node<'_>, field: &str, source: &'s str) -> Option<&'s str> {
    node.child_by_field_name(field)
        .map(|child| text(child, source))
}

/// Whether the `def` or `class` is written directly in a class body, where its name is an
/// attribute and binds nothing in the file.
fn in_class_body(node: Node<'_>) -> bool {
    let mut parent = node.parent();
    if parent.is_some_and(|p| p.kind() == "decorated_definition") {
        parent = parent.and_then(|p| p.parent());
    }
    parent.is_some_and(|block| {
        block.kind() == "block"
            && block
                .parent()
                .is_some_and(|owner| owner.kind() == "class_definition")
    })
}
