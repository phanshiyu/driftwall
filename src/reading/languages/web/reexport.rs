//! The re-export table of a TypeScript or JavaScript file: its `export .. from` statements and the
//! names it exports itself, see "Re-exports" in `docs/DESIGN.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Tree};

use super::env::Env;
use super::resolve::ecmascript;
use crate::reading::syntax::literal::literal_text;
use crate::reading::syntax::resolution::{Files, Reference, Resolution};
use crate::reading::syntax::table::Table;

/// The table of `file`, whose parsed text is `source`.
pub fn table(tree: &Tree, source: &str, file: &Path, files: &Files<'_>, env: &Env<'_>) -> Table {
    let mut table = Table::default();
    let root = tree.root_node();
    let imports = ImportedNames::of(root, source);
    let mut aliases = Vec::new();
    let mut cursor = root.walk();
    for statement in root.children(&mut cursor) {
        let line = u32::try_from(statement.start_position().row + 1).unwrap_or(u32::MAX);
        let declaration = match statement.kind() {
            "type_alias_declaration" => Some(statement),
            "export_statement" if statement.child_by_field_name("source").is_none() => statement
                .child_by_field_name("declaration")
                .filter(|node| node.kind() == "type_alias_declaration"),
            _ => None,
        };
        if let Some(found) =
            declaration.and_then(|node| alias(node, source, file, &imports, (files, env)))
        {
            table.lines.insert(line);
            aliases.push(found.0.clone());
            table.names.insert(found.0, found.1);
        } else if statement.kind() == "export_statement" {
            match statement.child_by_field_name("source") {
                Some(from) => reexport(statement, from, source, file, (files, env), &mut table),
                None => local(statement, source, &mut table),
            }
        }
    }
    // why: `type X = Y; export { X };` lists a name the file only forwards, not one it defines.
    for name in aliases {
        table.local.remove(&name);
    }
    table
}

/// The names the top-level `import` statements of a file bind, to the specifier node and the name
/// they have in that module (`default` for a default import). A namespace import binds none.
struct ImportedNames<'t>(BTreeMap<String, (Node<'t>, String)>);

impl<'t> ImportedNames<'t> {
    fn of(root: Node<'t>, source: &str) -> Self {
        let text = |node: Node<'_>| node.utf8_text(source.as_bytes()).ok().map(str::to_owned);
        let mut names = BTreeMap::new();
        let mut cursor = root.walk();
        for statement in root.children(&mut cursor) {
            let mut inside = statement.walk();
            let clause = statement
                .children(&mut inside)
                .find(|child| child.kind() == "import_clause");
            let (Some(from), Some(clause)) = (statement.child_by_field_name("source"), clause)
            else {
                continue;
            };
            let mut inner = clause.walk();
            for part in clause.named_children(&mut inner) {
                match part.kind() {
                    "identifier" => {
                        names.extend(text(part).map(|name| (name, (from, "default".to_owned()))));
                    }
                    "named_imports" => {
                        let mut specs = part.walk();
                        for spec in part.named_children(&mut specs) {
                            let original = spec.child_by_field_name("name").and_then(text);
                            let bound = spec.child_by_field_name("alias").and_then(text);
                            if let Some(original) = original {
                                let bound = bound.unwrap_or_else(|| original.clone());
                                names.insert(bound, (from, original));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Self(names)
    }
}

/// The (alias name, defining file and name) of a `type X = Y;` whose body is exactly a reference to
/// a name imported from another repository file, or `import('./y').Y`. `None` for every other
/// body, which defines a type of its own.
fn alias(
    declaration: Node<'_>,
    source: &str,
    file: &Path,
    imports: &ImportedNames<'_>,
    (files, env): (&Files<'_>, &Env<'_>),
) -> Option<(String, (PathBuf, String))> {
    let text = |node: Node<'_>| node.utf8_text(source.as_bytes()).ok().map(str::to_owned);
    let name = text(declaration.child_by_field_name("name")?)?;
    let body = declaration.child_by_field_name("value")?;
    let (from, original) = match body.kind() {
        "type_identifier" => imports.0.get(&text(body)?)?.clone(),
        "generic_type" => {
            let head = body.child_by_field_name("name")?;
            if head.kind() != "type_identifier" {
                return None;
            }
            imports.0.get(&text(head)?)?.clone()
        }
        // why: the grammar reads `import('./y').Y` in a type position as a member of a call.
        "member_expression" => {
            let call = body.child_by_field_name("object")?;
            if call.child_by_field_name("function")?.kind() != "import" {
                return None;
            }
            let argument = call.child_by_field_name("arguments")?.named_child(0)?;
            (argument, text(body.child_by_field_name("property")?)?)
        }
        _ => return None,
    };
    let specifier = literal_text(from, source)?;
    let reference = Reference {
        specifier,
        names: &[],
        node: from,
        source,
    };
    let target = ecmascript(&reference, file, files, env)
        .into_iter()
        .find_map(|resolution| match resolution {
            Resolution::Resolved { target, .. } => Some(target),
            _ => None,
        })
        .filter(|target| target != file)?;
    Some((name, (target, original)))
}

/// Files the names of `export { .. } from` and `export * from` in `table`.
fn reexport(
    statement: Node<'_>,
    from: Node<'_>,
    source: &str,
    file: &Path,
    (files, env): (&Files<'_>, &Env<'_>),
    table: &mut Table,
) {
    let Some(specifier) = literal_text(from, source) else {
        return;
    };
    let reference = Reference {
        specifier,
        names: &[],
        node: from,
        source,
    };
    let target = ecmascript(&reference, file, files, env)
        .into_iter()
        .find_map(|resolution| match resolution {
            Resolution::Resolved { target, .. } => Some(target),
            _ => None,
        });
    table
        .lines
        .insert(u32::try_from(statement.start_position().row + 1).unwrap_or(u32::MAX));
    let Some(target) = target.filter(|target| target != file) else {
        return;
    };
    let mut cursor = statement.walk();
    for child in statement.children(&mut cursor) {
        match child.kind() {
            "*" => table.stars.push(target.clone()),
            "export_clause" => {
                for (original, exported) in specifiers(child, source) {
                    table.names.insert(exported, (target.clone(), original));
                }
            }
            _ => {}
        }
    }
}

/// Records the names `statement`, an `export` with no source, defines.
fn local(statement: Node<'_>, source: &str, table: &mut Table) {
    if let Some(declaration) = statement.child_by_field_name("declaration") {
        declared(declaration, source, &mut table.local);
    }
    if statement.child_by_field_name("value").is_some() || has_default(statement) {
        table.local.insert("default".to_owned());
    }
    let mut cursor = statement.walk();
    for clause in statement
        .children(&mut cursor)
        .filter(|child| child.kind() == "export_clause")
    {
        table
            .local
            .extend(specifiers(clause, source).into_iter().map(|(_, name)| name));
    }
}

fn has_default(statement: Node<'_>) -> bool {
    let mut cursor = statement.walk();
    statement
        .children(&mut cursor)
        .any(|child| child.kind() == "default")
}

/// The names a declaration brings: its own name, or the identifiers a `const`/`let`/`var` binds.
fn declared(declaration: Node<'_>, source: &str, out: &mut std::collections::BTreeSet<String>) {
    if matches!(
        declaration.kind(),
        "lexical_declaration" | "variable_declaration"
    ) {
        let mut cursor = declaration.walk();
        for declarator in declaration.named_children(&mut cursor) {
            if let Some(name) = declarator
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok())
            {
                out.insert(name.to_owned());
            }
        }
    } else if let Some(name) = declaration
        .child_by_field_name("name")
        .and_then(|name| name.utf8_text(source.as_bytes()).ok())
    {
        out.insert(name.trim_matches(['"', '\'']).to_owned());
    }
}

/// The (original name, exported name) of each specifier of the `{ .. }` list `clause`.
fn specifiers(clause: Node<'_>, source: &str) -> Vec<(String, String)> {
    let text = |node: Node<'_>| node.utf8_text(source.as_bytes()).ok().map(str::to_owned);
    let mut cursor = clause.walk();
    clause
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "export_specifier")
        .filter_map(|specifier| {
            let original = text(specifier.child_by_field_name("name")?)?;
            let exported = specifier
                .child_by_field_name("alias")
                .and_then(text)
                .unwrap_or_else(|| original.clone());
            Some((original, exported))
        })
        .collect()
}
