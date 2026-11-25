//! The shape of a language entry: grammar, regions, loads and doc imports, and how a statement is
//! located and flagged. What a match resolves to is decided by the language itself.

use std::path::Path;

use super::statement::Flags;
use crate::code::Language;

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

/// How a language treats the files its markup loads, see [`Scan::loads`].
pub struct Loads {
    /// Decides from the importing file whether the import is written in test code.
    pub flags: fn(&Path) -> Flags,
}

/// How a language finds the modules its comments name in types, see "`JSDoc` type imports" in
/// `docs/DESIGN.md`.
pub struct Docs {
    /// The imports written in the comments below the root of a parsed region.
    pub read: for<'a> fn(tree_sitter::Node<'_>, &'a str) -> Vec<DocImport<'a>>,
    /// Decides from the importing file whether the import is written in test code. The import is
    /// type-only whatever the file.
    pub flags: fn(&Path) -> Flags,
}

/// A module named by a type in a `/**` comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocImport<'s> {
    /// The string between the quotes of `import('..')`.
    pub specifier: &'s str,
    /// The identifier after `import('..').`, when there is one.
    pub member: Option<&'s str>,
    /// The line of the `import` word.
    pub line: u32,
}

/// Everything needed to extract the imports of one language. How a match resolves is the language's
/// own resolver, see the module `language` of `imports`.
pub struct LanguageSpec {
    /// Language this entry serves.
    pub language: Language,
    /// Finds what to parse in `source`, the text of `file`: the whole file with a grammar chosen
    /// per file because TSX needs its own, or the script elements of a markup file.
    pub scan: fn(&str, &Path) -> Scan,
    /// The node whose first line is the location of the import that `@specifier` is part of: the
    /// statement, or the call expression, around it.
    pub anchor: for<'a> fn(tree_sitter::Node<'a>) -> tree_sitter::Node<'a>,
    /// Decides from the anchor, the source text and the importing file whether the import is
    /// type-only and whether it is written in test code.
    pub flags: for<'a> fn(tree_sitter::Node<'a>, &str, &Path) -> Flags,
    /// How the files that [`Scan::loads`] lists resolve, for a language whose markup loads files.
    pub loads: Option<Loads>,
    /// `JSDoc` type imports, for a language whose scripts are TypeScript or JavaScript.
    pub docs: Option<&'static Docs>,
}

/// The scan of a language whose files are one piece of code in `grammar`.
#[must_use]
pub fn whole_file(grammar: &'static Grammar) -> Scan {
    Scan {
        regions: vec![Region {
            grammar,
            range: None,
        }],
        loads: Vec::new(),
    }
}
