//! The Swift targets a repository declares, read from its `Package.swift` manifests.
//!
//! The IO shell is [`swift_targets`]. The manifest is parsed with the Swift grammar, so comments and
//! strings never produce a target. See "Swift resolution" in `docs/DESIGN.md`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use super::paths::join_normalised;
use crate::model::Language;

/// The manifest file name.
const MANIFEST_FILE: &str = "Package.swift";

/// The calls that declare a target, the directory below the package that holds its sources when no
/// `path:` is given, and whether it is test code.
const TARGET_CALLS: &[(&str, &str, bool)] = &[
    ("target", "Sources", false),
    ("executableTarget", "Sources", false),
    ("testTarget", "Tests", true),
    ("macro", "Sources", false),
    ("plugin", "Sources", false),
];

/// The call that is also the plugin product of `products:` unless it has this argument.
const PLUGIN_CALL: &str = "plugin";
const PLUGIN_CAPABILITY: &str = "capability";

/// One target declared by a `Package.swift`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SwiftTarget {
    /// The `name:` of the target.
    pub name: String,
    /// The name `import` writes: the target name with every character other than an ASCII letter,
    /// digit or `_` replaced by `_`.
    pub module: String,
    /// Repository-relative directory that holds the sources of the target.
    pub dir: PathBuf,
    /// Repository-relative directory of the `Package.swift` that declares the target.
    pub package: PathBuf,
    /// Whether the target is a `.testTarget(`.
    pub test: bool,
}

/// The module name `SwiftPM` gives a target called `name`.
fn module_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// The targets of every `Package.swift` among `files`, read from under `root`, sorted. A manifest
/// that cannot be read declares none.
#[must_use]
pub fn swift_targets(root: &Path, files: &BTreeMap<PathBuf, Language>) -> Vec<SwiftTarget> {
    let mut targets: Vec<SwiftTarget> = files
        .iter()
        .filter(|(path, language)| {
            **language == Language::Swift && path.file_name().is_some_and(|n| n == MANIFEST_FILE)
        })
        .filter_map(|(path, _)| {
            let text = fs::read_to_string(root.join(path)).ok()?;
            Some(manifest_targets(
                &text,
                path.parent().unwrap_or_else(|| Path::new("")),
            ))
        })
        .flatten()
        .collect();
    targets.sort();
    targets
}

/// The targets declared by the manifest `text` of the package in `package`.
fn manifest_targets(text: &str, package: &Path) -> Vec<SwiftTarget> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        match call(node, text, package) {
            Call::Target(target) => found.extend(target),
            Call::Other => {
                let mut cursor = node.walk();
                pending.extend(node.children(&mut cursor));
            }
        }
    }
    found
}

/// What a syntax node is, as far as the manifest is concerned.
enum Call {
    /// Not a call of [`TARGET_CALLS`]: the walk goes on into it.
    Other,
    /// A call that declares a target, or would if its arguments allowed it. The walk never enters
    /// such a call, so a `.target(name:)` inside `dependencies:` declares nothing.
    Target(Option<SwiftTarget>),
}

/// What an argument of a call holds.
enum Written {
    /// The argument is not written.
    Absent,
    /// A plain string literal, and its contents.
    Plain(String),
    /// Anything else: an interpolated or escaped string, a name, a call.
    Computed,
}

/// Classifies the syntax node `node`, the manifest being `text` and the package in `package`.
fn call(node: Node<'_>, text: &str, package: &Path) -> Call {
    if node.kind() != "call_expression" {
        return Call::Other;
    }
    let Some(callee) = method(node, text) else {
        return Call::Other;
    };
    let Some(&(_, default, is_test)) = TARGET_CALLS.iter().find(|(known, _, _)| *known == callee)
    else {
        return Call::Other;
    };
    let arguments = arguments(node, text);
    let argument = |label: &str| {
        arguments
            .iter()
            .find(|(found, _)| *found == label)
            .map(|(_, value)| *value)
    };
    if callee == PLUGIN_CALL && argument(PLUGIN_CAPABILITY).is_none() {
        return Call::Target(None);
    }
    let written = |label: &str| {
        argument(label).map_or(Written::Absent, |value| {
            literal(value, text).map_or(Written::Computed, Written::Plain)
        })
    };
    Call::Target(declared(
        &written("name"),
        &written("path"),
        (package, default, is_test),
    ))
}

/// The target a call names, `None` when its name or its directory cannot be known.
fn declared(
    name: &Written,
    path: &Written,
    (package, default, is_test): (&Path, &str, bool),
) -> Option<SwiftTarget> {
    let Written::Plain(name) = name else {
        return None;
    };
    let below = match path {
        Written::Absent => format!("{default}/{name}"),
        Written::Plain(path) => path.clone(),
        Written::Computed => return None,
    };
    Some(SwiftTarget {
        module: module_name(name),
        dir: join_normalised(Some(package), &below)?,
        package: package.to_path_buf(),
        name: name.clone(),
        test: is_test,
    })
}

/// The name in `.name(` of a call expression, `None` for any other callee.
fn method<'t>(call: Node<'_>, text: &'t str) -> Option<&'t str> {
    let prefix = call
        .named_child(0)
        .filter(|n| n.kind() == "prefix_expression")?;
    let dot = prefix.child_by_field_name("operation")?;
    if dot.utf8_text(text.as_bytes()).ok()? != "." {
        return None;
    }
    prefix
        .child_by_field_name("target")?
        .utf8_text(text.as_bytes())
        .ok()
}

/// The labelled arguments of a call, as (label, value node), in source order.
fn arguments<'t, 'a>(call: Node<'a>, text: &'t str) -> Vec<(&'t str, Node<'a>)> {
    let mut cursor = call.walk();
    let Some(suffix) = call
        .children(&mut cursor)
        .find(|n| n.kind() == "call_suffix")
    else {
        return Vec::new();
    };
    let mut inner = suffix.walk();
    let Some(list) = suffix
        .children(&mut inner)
        .find(|n| n.kind() == "value_arguments")
    else {
        return Vec::new();
    };
    let mut each = list.walk();
    list.children(&mut each)
        .filter(|n| n.kind() == "value_argument")
        .filter_map(|argument| {
            let label = argument
                .child_by_field_name("name")?
                .utf8_text(text.as_bytes())
                .ok()?;
            Some((label, argument.child_by_field_name("value")?))
        })
        .collect()
}

/// The contents of a plain string literal: no interpolation and no escape. `None` for anything
/// else.
fn literal(node: Node<'_>, text: &str) -> Option<String> {
    if node.kind() != "line_string_literal" {
        return None;
    }
    let mut cursor = node.walk();
    let mut contents = String::new();
    for part in node.children(&mut cursor) {
        match part.kind() {
            "\"" => {}
            "line_str_text" => contents.push_str(part.utf8_text(text.as_bytes()).ok()?),
            _ => return None,
        }
    }
    Some(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(text: &str) -> Vec<(String, String, bool)> {
        let mut found: Vec<_> = manifest_targets(text, Path::new("pkg"))
            .into_iter()
            .map(|t| (t.name, t.dir.display().to_string(), t.test))
            .collect();
        found.sort();
        found
    }

    fn t(name: &str, dir: &str, test: bool) -> (String, String, bool) {
        (name.to_owned(), dir.to_owned(), test)
    }

    #[test]
    fn targets_have_default_and_explicit_directories() {
        let found = targets(
            r#"// swift-tools-version:5.9
import PackageDescription
let package = Package(
    name: "PM",
    targets: [
        .target(name: "AppCore"),
        .executableTarget(name: "DeskApp", dependencies: ["AppCore"], path: "App/PM"),
        .testTarget(name: "AppCoreTests", dependencies: ["AppCore"]),
        .macro(name: "Macros"),
        .plugin(name: "Gen", capability: .buildTool()),
        .target(name: "Dotted", path: "./a/../b/"),
    ]
)
"#,
        );
        assert_eq!(
            found,
            [
                t("AppCore", "pkg/Sources/AppCore", false),
                t("AppCoreTests", "pkg/Tests/AppCoreTests", true),
                t("DeskApp", "pkg/App/PM", false),
                t("Dotted", "pkg/b", false),
                t("Gen", "pkg/Sources/Gen", false),
                t("Macros", "pkg/Sources/Macros", false),
            ]
        );
    }

    #[test]
    fn only_the_arguments_of_the_call_itself_name_the_target() {
        let found = targets(
            r#"let package = Package(
    name: "PM",
    products: [
        .library(name: "Lib", targets: ["Core"]),
        .plugin(name: "Tool", targets: ["Gen"]),
    ],
    targets: [
        .target(name: "Core", dependencies: [
            .target(name: "Inner"),
            .product(name: "Algo", package: "swift-algorithms"),
        ]),
        .plugin(name: "Gen", capability: .command(intent: .custom(verb: "x", description: "y"))),
    ]
)
"#,
        );
        assert_eq!(
            found,
            [
                t("Core", "pkg/Sources/Core", false),
                t("Gen", "pkg/Sources/Gen", false),
            ]
        );
    }

    #[test]
    fn comments_and_strings_declare_nothing() {
        let found = targets(
            r#"let package = Package(
    name: "PM", // .target(name: "Nope")
    /* .target(name: "AlsoNope") */
    targets: [ .target(name: "Real") ],
    description: ".target(name: \"Text\")"
)
"#,
        );
        assert_eq!(found, [t("Real", "pkg/Sources/Real", false)]);
    }

    #[test]
    fn a_target_whose_name_or_path_is_not_a_plain_literal_is_skipped() {
        let found = targets(
            r##"let n = "x"
let package = Package(name: "PM", targets: [
    .target(name: n),
    .target(name: "Interp\(n)"),
    .target(name: "Escaped\n"),
    .target(name: "DynamicPath", path: base),
    .target(name: "Raw", path: #"Sources/Raw"#),
    .target(name: "Outside", path: "../../x"),
    .target(),
    .target(name: "Ok"),
])
"##,
        );
        assert_eq!(found, [t("Ok", "pkg/Sources/Ok", false)]);
    }

    #[test]
    fn the_module_name_replaces_every_other_character() {
        let found = manifest_targets(
            r#"let p = Package(name: "x", targets: [.executableTarget(name: "app-cli")])"#,
            Path::new(""),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].module, "app_cli");
        assert_eq!(found[0].name, "app-cli");
        assert_eq!(found[0].dir, Path::new("Sources/app-cli"));
    }

    #[test]
    fn text_that_is_not_a_manifest_declares_nothing() {
        assert_eq!(targets(""), []);
        assert_eq!(targets("let x = )))((("), []);
        assert_eq!(targets("let p = Package(name: \"x\", targets: [])\n"), []);
    }
}
