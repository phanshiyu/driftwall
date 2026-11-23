//! Entry files that declare nothing, which never make a node of their own under `node: *`.
//!
//! See "Folded entry files" in `docs/DESIGN.md`. The scanners here are textual on purpose: they
//! only have to tell "no declaration at all" from "anything else", and any doubt answers "anything
//! else", so a file is folded only when it plainly declares nothing.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

/// Whether `file` is named like an entry file that can be folded: `__init__.py`, `lib.rs` or
/// `main.rs`.
#[must_use]
pub fn is_entry_file(file: &Path) -> bool {
    matches!(
        file.file_name().and_then(OsStr::to_str),
        Some("__init__.py" | "lib.rs" | "main.rs")
    )
}

/// The entry files among `files` (relative to `root`) that declare nothing.
#[must_use]
pub fn trivial_files<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a Path>,
) -> BTreeSet<PathBuf> {
    files
        .into_iter()
        .filter(|file| is_entry_file(file))
        .filter(|file| {
            // why: a file that cannot be read is reported by the import collection, never folded.
            fs::read_to_string(root.join(file)).is_ok_and(|text| declares_nothing(file, &text))
        })
        .map(Path::to_path_buf)
        .collect()
}

/// Whether the entry file `file` with the text `source` declares nothing.
///
/// A Python file declares nothing when its only statements are string literals (a docstring), a
/// Rust file when it only has comments, attributes and `mod` declarations without a body.
#[must_use]
pub fn declares_nothing(file: &Path, source: &str) -> bool {
    match file.extension().and_then(OsStr::to_str) {
        Some("py") => python_is_trivial(source),
        Some("rs") => rust_is_trivial(source),
        _ => false,
    }
}

/// Cursor over the bytes of a source text.
struct Scan<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Scan<'a> {
    const fn new(source: &'a str) -> Self {
        Self {
            text: source.as_bytes(),
            at: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.text
            .get(self.at..)
            .is_some_and(|rest| rest.starts_with(prefix.as_bytes()))
    }

    fn skip_while(&mut self, keep: impl Fn(u8) -> bool) {
        while self.peek().is_some_and(&keep) {
            self.at += 1;
        }
    }

    fn skip_line(&mut self) {
        self.skip_while(|byte| byte != b'\n');
    }

    /// Skips `text` when it is next.
    fn eat(&mut self, text: &str) -> bool {
        let found = self.starts_with(text);
        if found {
            self.at += text.len();
        }
        found
    }

    /// Skips a quoted string literal that starts at the cursor, `quote` being its delimiter
    /// (one byte, or three for a triple quote). Backslash escapes the next byte.
    fn skip_string(&mut self, quote: &str) -> bool {
        self.at += quote.len();
        while self.peek().is_some() {
            if self.eat("\\") {
                self.at += 1;
            } else if self.eat(quote) {
                return true;
            } else if quote.len() == 1 && self.peek() == Some(b'\n') {
                return false;
            } else {
                self.at += 1;
            }
        }
        false
    }
}

fn python_is_trivial(source: &str) -> bool {
    let mut scan = Scan::new(source);
    loop {
        scan.skip_while(|byte| byte.is_ascii_whitespace() || byte == b';');
        match scan.peek() {
            None => return true,
            Some(b'#') => scan.skip_line(),
            Some(_) => {
                // why: a string prefix (`r`, `u`, `b`, `f`) is no code either, and a docstring may carry one.
                scan.skip_while(|byte| matches!(byte, b'r' | b'R' | b'u' | b'U' | b'b' | b'B'));
                let quote = ["\"\"\"", "'''", "\"", "'"]
                    .into_iter()
                    .find(|quote| scan.starts_with(quote));
                let Some(quote) = quote else {
                    return false;
                };
                if !scan.skip_string(quote) {
                    return false;
                }
                // why: anything but a comment after the literal on its line (`"x" + y`) is code.
                scan.skip_while(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b';'));
                if !matches!(scan.peek(), None | Some(b'\n' | b'#')) {
                    return false;
                }
            }
        }
    }
}

fn rust_is_trivial(source: &str) -> bool {
    let mut scan = Scan::new(source);
    loop {
        scan.skip_while(|byte| byte.is_ascii_whitespace());
        if scan.eat("//") {
            scan.skip_line();
        } else if scan.eat("/*") {
            if !skip_block_comment(&mut scan) {
                return false;
            }
        } else if scan.eat("#") {
            scan.eat("!");
            if !skip_attribute(&mut scan) {
                return false;
            }
        } else if scan.peek().is_none() {
            return true;
        } else if !skip_module_declaration(&mut scan) {
            return false;
        }
    }
}

/// Skips the rest of a block comment, which nests. False when it is not closed.
fn skip_block_comment(scan: &mut Scan<'_>) -> bool {
    let mut depth = 1_usize;
    while depth > 0 {
        if scan.eat("/*") {
            depth += 1;
        } else if scan.eat("*/") {
            depth -= 1;
        } else if scan.peek().is_some() {
            scan.at += 1;
        } else {
            return false;
        }
    }
    true
}

/// Skips `[ ... ]` of an attribute, whose brackets nest and whose strings may hold brackets.
fn skip_attribute(scan: &mut Scan<'_>) -> bool {
    scan.skip_while(|byte| byte.is_ascii_whitespace());
    if !scan.eat("[") {
        return false;
    }
    let mut depth = 1_usize;
    while depth > 0 {
        if scan.starts_with("\"") {
            if !scan.skip_string("\"") {
                return false;
            }
        } else if scan.eat("[") {
            depth += 1;
        } else if scan.eat("]") {
            depth -= 1;
        } else if scan.peek().is_some() {
            scan.at += 1;
        } else {
            return false;
        }
    }
    true
}

/// Skips `[pub[(restriction)]] mod name;`. False for anything else.
fn skip_module_declaration(scan: &mut Scan<'_>) -> bool {
    let start = scan.at;
    if word(scan) == Some("pub") {
        scan.skip_while(|byte| byte.is_ascii_whitespace());
        if scan.eat("(") {
            scan.skip_while(|byte| byte != b')');
            scan.eat(")");
        }
    } else {
        scan.at = start;
    }
    scan.skip_while(|byte| byte.is_ascii_whitespace());
    if word(scan) != Some("mod") {
        return false;
    }
    scan.skip_while(|byte| byte.is_ascii_whitespace());
    if word(scan).is_none() {
        return false;
    }
    scan.skip_while(|byte| byte.is_ascii_whitespace());
    scan.eat(";")
}

/// Skips an identifier-like word at the cursor and returns it.
fn word<'a>(scan: &mut Scan<'a>) -> Option<&'a str> {
    let start = scan.at;
    scan.skip_while(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    let found = scan.text.get(start..scan.at)?;
    std::str::from_utf8(found)
        .ok()
        .filter(|word| !word.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn python(source: &str) -> bool {
        declares_nothing(Path::new("pkg/__init__.py"), source)
    }

    fn rust(source: &str) -> bool {
        declares_nothing(Path::new("src/lib.rs"), source)
    }

    #[test]
    fn a_python_file_of_comments_and_docstrings_declares_nothing() {
        assert!(python(""));
        assert!(python("\n\n"));
        assert!(python("# only a comment\n"));
        assert!(python("\"\"\"The package.\n\nMore (not code).\n\"\"\"\n"));
        assert!(python("'''doc'''  # note\n# more\n\"one\"\n"));
        assert!(python("r\"\"\"raw\"\"\"\n"));
    }

    #[test]
    fn any_other_python_statement_is_a_declaration() {
        assert!(!python("import os\n"));
        assert!(!python("from . import a\n"));
        assert!(!python("__all__ = []\n"));
        assert!(!python("\"\"\"doc\"\"\"\nx = 1\n"));
        assert!(!python("\"a\" + b\n"));
        assert!(!python("\"\"\"never closed\n"));
        assert!(!python("pass\n"));
    }

    #[test]
    fn a_rust_file_of_mod_declarations_declares_nothing() {
        assert!(rust(""));
        assert!(rust(
            "//! The crate.\n\n#![deny(missing_docs)]\n\npub mod a;\nmod b;\npub(crate) mod c; // c\n"
        ));
        assert!(rust(
            "/* a /* nested */ comment */\n#[cfg(test)]\nmod tests;\n"
        ));
        assert!(rust("#[path = \"x]y.rs\"]\nmod z;\n"));
    }

    #[test]
    fn any_other_rust_item_is_a_declaration() {
        assert!(!rust("pub mod a;\npub use a::B;\n"));
        assert!(!rust("use a::B;\n"));
        assert!(!rust("pub mod a { pub fn f() {} }\n"));
        assert!(!rust("fn main() {}\n"));
        assert!(!rust("pub const A: u8 = 1;\n"));
        assert!(!rust("/* never closed\n"));
        assert!(!rust("mod ;\n"));
    }

    #[test]
    fn only_entry_files_and_known_languages_can_fold() {
        assert!(is_entry_file(Path::new("a/__init__.py")));
        assert!(is_entry_file(Path::new("lib.rs")));
        assert!(is_entry_file(Path::new("a/main.rs")));
        assert!(!is_entry_file(Path::new("a/mod.rs")));
        assert!(!declares_nothing(Path::new("index.ts"), ""));
    }
}
