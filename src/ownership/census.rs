//! The census of walked files that driftwall cannot read: code in a language it has no table
//! entry for.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use super::language_of;

/// The key under which an extensionless script (first two bytes `#!`) is counted.
pub const SHEBANG: &str = "shebang";

/// Common code file extensions (no dot, case-sensitive), including some the language table
/// supports. Those are left out at the point of use, so this list never has to change when a
/// language is added.
const CODE_EXTENSIONS: &[&str] = &[
    "sh", "bash", "zsh", "swift", "c", "h", "cc", "cpp", "hpp", "m", "mm", "go", "java", "kt",
    "kts", "rb", "php", "cs", "lua", "dart", "scala", "ex", "exs", "erl", "hs", "ml", "clj", "vue",
    "astro", "pl", "r", "sql", "ps1", "cmd", "bat", "mc", "zig", "nim", "svelte", "html", "htm",
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "py", "rs",
];

/// The census key of `relative`, a walked file below `root`, or `None` when it is not unread code.
///
/// The key is the extension when it is in the code table and no language supports it, or
/// [`SHEBANG`] for a file without an extension that starts with `#!`. A file with a supported
/// extension is source, so it never has a key.
pub(super) fn unread_key(root: &Path, relative: &Path) -> Option<String> {
    match relative.extension() {
        Some(extension) => {
            let extension = extension.to_str()?;
            let known = CODE_EXTENSIONS.contains(&extension);
            // why: the language table is the only authority on what is source.
            let supported = language_of(&Path::new("x").with_extension(extension)).is_some();
            (known && !supported).then(|| extension.to_owned())
        }
        None => starts_with_shebang(&root.join(relative)).then(|| SHEBANG.to_owned()),
    }
}

/// Whether the file at `path` starts with `#!`. A file that cannot be read does not.
fn starts_with_shebang(path: &Path) -> bool {
    let mut head = [0_u8; 2];
    // why: an unreadable file is simply not counted; the census must never fail the scan.
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok()
        && head == *b"#!"
}

/// The number of lines of the file `relative` below `root`: its newline bytes, plus one when the
/// last line has no newline. A file that cannot be read has 0.
#[must_use]
pub fn line_count(root: &Path, relative: &Path) -> usize {
    // why: lines are informational, so a vanished or unreadable file counts as empty.
    let Ok(bytes) = fs::read(root.join(relative)) else {
        return 0;
    };
    bytes.split_inclusive(|&byte| byte == b'\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    fn key(dir: &tempfile::TempDir, file: &str) -> Option<String> {
        unread_key(dir.path(), Path::new(file))
    }

    #[test]
    fn code_extensions_without_a_language_are_keyed_by_extension() {
        let dir = tree(&[("a.go", ""), ("b.pl", ""), ("c.json", ""), ("d.TS", "")]);
        assert_eq!(key(&dir, "a.go").as_deref(), Some("go"));
        assert_eq!(key(&dir, "b.pl").as_deref(), Some("pl"));
        assert_eq!(key(&dir, "c.json"), None);
        assert_eq!(key(&dir, "d.TS"), None);
    }

    #[test]
    fn supported_extensions_are_never_unread() {
        let dir = tree(&[("a.ts", ""), ("b.py", ""), ("c.rs", ""), ("d.mjs", "")]);
        for file in ["a.ts", "b.py", "c.rs", "d.mjs"] {
            assert_eq!(key(&dir, file), None, "{file}");
        }
    }

    #[test]
    fn extensionless_files_are_unread_only_with_a_shebang() {
        let dir = tree(&[
            ("bin/run", "#!/bin/sh\necho hi\n"),
            ("Makefile", "all:\n"),
            ("short", "#"),
            ("empty", ""),
        ]);
        assert_eq!(key(&dir, "bin/run").as_deref(), Some(SHEBANG));
        for file in ["Makefile", "short", "empty", "missing"] {
            assert_eq!(key(&dir, file), None, "{file}");
        }
    }

    #[test]
    fn lines_are_newlines_plus_an_unterminated_last_line() {
        let dir = tree(&[("a", "x\ny\n"), ("b", "x\ny"), ("c", ""), ("d", "\n")]);
        let lines = |file: &str| line_count(dir.path(), Path::new(file));
        assert_eq!(
            [
                lines("a"),
                lines("b"),
                lines("c"),
                lines("d"),
                lines("gone")
            ],
            [2, 2, 0, 1, 0]
        );
    }
}
