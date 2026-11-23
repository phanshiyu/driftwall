//! The per-language table: grammar, import query and resolver for each supported language.

use std::path::Path;

use super::{
    Bindings, Environment, Reference, Resolution, jsdoc, markup, python, queries, resolve, rust,
    shell, statement, statement::Flags, swift,
};
use crate::model::Language;

/// One tree-sitter grammar with a stable name that keys the compiled-query cache, and the query
/// that captures the imports written in it.
pub struct Grammar {
    /// Unique across all grammars.
    pub name: &'static str,
    /// Builds the tree-sitter language.
    pub language: fn() -> tree_sitter::Language,
    /// Tree-sitter query capturing each import's module reference as `@specifier` and, when one
    /// statement imports several things from it, each imported name as `@name`.
    pub query: &'static str,
}

/// A stretch of a file that one grammar parses.
pub struct Region {
    /// The grammar of the code in the region.
    pub grammar: &'static Grammar,
    /// The part of the file the grammar parses, or `None` for the whole file. Positions of nodes
    /// are positions in the file either way.
    pub range: Option<tree_sitter::Range>,
}

/// What [`LanguageSpec::scan`] finds in one file.
pub struct Scan {
    /// The regions to parse and query, in source order.
    pub regions: Vec<Region>,
    /// The files the markup loads without an import statement, in source order. Empty unless the
    /// language has [`LanguageSpec::loads`].
    pub loads: Vec<Load>,
}

/// A reference to a file that is written as an attribute of a tag, not as an import statement.
pub struct Load {
    /// The path as written, trimmed.
    pub specifier: String,
    /// The line the tag starts on.
    pub line: u32,
    /// Whether the file is a resource the page references (a stylesheet, a font, an icon) rather
    /// than code it runs: a source file it finds is then an asset, not a runtime import.
    pub asset: bool,
}

/// How a language resolves the files its markup loads, see [`Scan::loads`].
pub struct Loads {
    /// Resolves one load written in `importer`. The import it makes uses no symbol.
    pub resolve: fn(&Load, &Path, &Environment<'_>) -> Resolution,
    /// Decides from the importing file whether the import is written in test code.
    pub flags: fn(&Path) -> Flags,
}

/// How a language finds and resolves the modules its comments name in types, see "`JSDoc` type imports" in `docs/DESIGN.md`.
pub struct Docs {
    /// The imports written in the comments below the root of a parsed region.
    pub read: for<'a> fn(tree_sitter::Node<'_>, &'a str) -> Vec<jsdoc::DocImport<'a>>,
    /// Resolves one specifier written in `importer`. Its import uses at most the member read.
    pub resolve: fn(&str, &Path, &Environment<'_>) -> Resolution,
    /// Decides from the importing file whether the import is written in test code. The import is
    /// type-only whatever the file.
    pub flags: fn(&Path) -> Flags,
}

pub(super) static TYPESCRIPT_GRAMMAR: Grammar = Grammar {
    name: "typescript",
    language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    query: queries::TYPESCRIPT,
};

static TSX_GRAMMAR: Grammar = Grammar {
    name: "tsx",
    language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
    query: queries::TYPESCRIPT,
};

pub(super) static JAVASCRIPT_GRAMMAR: Grammar = Grammar {
    name: "javascript",
    language: || tree_sitter_javascript::LANGUAGE.into(),
    query: queries::JAVASCRIPT,
};

static PYTHON_GRAMMAR: Grammar = Grammar {
    name: "python",
    language: || tree_sitter_python::LANGUAGE.into(),
    query: queries::PYTHON,
};

static SWIFT_GRAMMAR: Grammar = Grammar {
    name: "swift",
    language: || tree_sitter_swift::LANGUAGE.into(),
    query: queries::SWIFT,
};

static SHELL_GRAMMAR: Grammar = Grammar {
    name: "bash",
    language: || tree_sitter_bash::LANGUAGE.into(),
    query: queries::SHELL,
};

pub(super) static RUST_GRAMMAR: Grammar = Grammar {
    name: "rust",
    language: || tree_sitter_rust::LANGUAGE.into(),
    query: queries::RUST,
};

/// Everything needed to extract and resolve imports for one language. Adding a language is adding
/// one entry to [`spec`].
pub struct LanguageSpec {
    /// Language this entry serves.
    pub language: Language,
    /// Finds what to parse in `source`, the text of `file`: the whole file with a grammar chosen
    /// per file because TSX needs its own, or the script elements of a markup file.
    pub scan: fn(&str, &Path) -> Scan,
    /// Collects, once per file, the names the file brings into scope. Languages whose resolver
    /// does not follow such names use a function that returns no bindings.
    pub bindings: for<'a> fn(tree_sitter::Node<'a>, &'a str) -> Bindings<'a>,
    /// The node whose first line is the location of the import that `@specifier` is part of: the
    /// statement, or the call expression, around it.
    pub anchor: for<'a> fn(tree_sitter::Node<'a>) -> tree_sitter::Node<'a>,
    /// Decides from the anchor, the source text and the importing file whether the import is
    /// type-only and whether it is written in test code.
    pub flags: for<'a> fn(tree_sitter::Node<'a>, &str, &Path) -> Flags,
    /// Resolves one match, written in `importer`, against the [`Environment`]. Returns one
    /// [`Resolution`] per target the match refers to, so a statement naming several imports can
    /// resolve each independently.
    pub resolve: fn(&Reference<'_>, &Path, &Environment<'_>) -> Vec<Resolution>,
    /// How the files that [`Scan::loads`] lists resolve, for a language whose markup loads files.
    pub loads: Option<Loads>,
    /// `JSDoc` type imports, for a language whose scripts are TypeScript or JavaScript.
    pub docs: Option<&'static Docs>,
}

/// The [`LanguageSpec::docs`] of TypeScript, JavaScript and Svelte.
static DOCS: Docs = Docs {
    read: jsdoc::read,
    resolve: resolve::specifier,
    flags: statement::doc_flags,
};

static TYPESCRIPT: LanguageSpec = LanguageSpec {
    language: Language::TypeScript,
    scan: typescript_scan,
    bindings: no_bindings,
    anchor: statement::ecmascript_anchor,
    flags: statement::ecmascript_flags,
    resolve: resolve::ecmascript,
    loads: None,
    docs: Some(&DOCS),
};

static PYTHON: LanguageSpec = LanguageSpec {
    language: Language::Python,
    scan: python_scan,
    bindings: no_bindings,
    anchor: statement::python_anchor,
    flags: statement::python_flags,
    resolve: python::resolve,
    loads: None,
    docs: None,
};

static SVELTE: LanguageSpec = LanguageSpec {
    language: Language::Svelte,
    scan: markup::svelte,
    bindings: no_bindings,
    anchor: statement::ecmascript_anchor,
    flags: statement::ecmascript_flags,
    resolve: resolve::ecmascript,
    loads: None,
    docs: Some(&DOCS),
};

static HTML: LanguageSpec = LanguageSpec {
    language: Language::Html,
    scan: markup::html,
    bindings: no_bindings,
    anchor: statement::ecmascript_anchor,
    flags: statement::ecmascript_flags,
    resolve: markup::resolve_inline,
    loads: Some(Loads {
        resolve: markup::resolve_load,
        flags: statement::load_flags,
    }),
    docs: None,
};

static JAVASCRIPT: LanguageSpec = LanguageSpec {
    language: Language::JavaScript,
    scan: javascript_scan,
    bindings: no_bindings,
    anchor: statement::ecmascript_anchor,
    flags: statement::ecmascript_flags,
    resolve: resolve::ecmascript,
    loads: None,
    docs: Some(&DOCS),
};

static RUST: LanguageSpec = LanguageSpec {
    language: Language::Rust,
    scan: rust_scan,
    bindings: rust_bindings,
    anchor: statement::rust_anchor,
    flags: statement::rust_flags,
    resolve: rust::resolve,
    loads: None,
    docs: None,
};

static SWIFT: LanguageSpec = LanguageSpec {
    language: Language::Swift,
    scan: swift_scan,
    bindings: no_bindings,
    anchor: statement::swift_anchor,
    flags: statement::swift_flags,
    resolve: swift::resolve,
    loads: None,
    docs: None,
};

static SHELL: LanguageSpec = LanguageSpec {
    language: Language::Shell,
    scan: shell_scan,
    bindings: no_bindings,
    anchor: statement::shell_anchor,
    flags: statement::shell_flags,
    resolve: shell::resolve,
    loads: None,
    docs: None,
};

/// The table entry for `language`, or `None` when extraction is not implemented for it yet.
#[must_use]
pub fn spec(language: Language) -> Option<&'static LanguageSpec> {
    match language {
        Language::TypeScript => Some(&TYPESCRIPT),
        Language::JavaScript => Some(&JAVASCRIPT),
        Language::Python => Some(&PYTHON),
        Language::Rust => Some(&RUST),
        Language::Svelte => Some(&SVELTE),
        Language::Html => Some(&HTML),
        Language::Swift => Some(&SWIFT),
        Language::Shell => Some(&SHELL),
    }
}

/// The [`LanguageSpec::bindings`] of a language that does not follow names brought into scope.
fn no_bindings<'a>(_root: tree_sitter::Node<'a>, _source: &'a str) -> Bindings<'a> {
    Bindings::default()
}

/// The [`LanguageSpec::bindings`] of Rust: its `use` leaves and `mod name;` declarations.
fn rust_bindings<'a>(root: tree_sitter::Node<'a>, source: &'a str) -> Bindings<'a> {
    Bindings::of_tree(root, source)
}

/// The scan of a language whose files are one piece of code in `grammar`.
fn whole_file(grammar: &'static Grammar) -> Scan {
    Scan {
        regions: vec![Region {
            grammar,
            range: None,
        }],
        loads: Vec::new(),
    }
}

fn typescript_scan(_source: &str, file: &Path) -> Scan {
    whole_file(
        if file.extension().is_some_and(|extension| extension == "tsx") {
            &TSX_GRAMMAR
        } else {
            &TYPESCRIPT_GRAMMAR
        },
    )
}

fn javascript_scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&JAVASCRIPT_GRAMMAR)
}

fn python_scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&PYTHON_GRAMMAR)
}

fn swift_scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&SWIFT_GRAMMAR)
}

fn shell_scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&SHELL_GRAMMAR)
}

fn rust_scan(_source: &str, _file: &Path) -> Scan {
    whole_file(&RUST_GRAMMAR)
}
