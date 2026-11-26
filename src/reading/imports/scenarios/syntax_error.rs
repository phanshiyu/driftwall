//! A file whose syntax tree has an error is an `unparsed` skip, once per file, and keeps the
//! imports that were still recovered.

use super::fixtures::repo;
use super::*;
use crate::code::SkipReason;

fn skip_lines(imports: &FileImports) -> Vec<(u32, &str)> {
    imports
        .skipped
        .iter()
        .filter(|skip| skip.reason == SkipReason::Unparsed)
        .map(|skip| (skip.at.line, skip.specifier.as_str()))
        .collect()
}

#[test]
fn conflict_markers_are_a_blind_spot_and_earlier_imports_stay() {
    let r = repo(&[
        (
            "src/a.ts",
            "import './b';\n<<<<<<< HEAD\nconst x = 1;\n=======\nconst x = 2;\n>>>>>>> branch\nimport './c';\n",
        ),
        ("src/b.ts", ""),
        ("src/c.ts", ""),
    ]);
    let got = r.imports("src/a.ts");
    assert_eq!(skip_lines(&got), [(2, "src/a.ts")]);
    assert!(
        got.resolved
            .iter()
            .any(|import| import.target == Path::new("src/b.ts"))
    );
}

#[test]
fn an_unterminated_template_before_an_import_is_a_blind_spot() {
    let r = repo(&[
        ("src/a.ts", "const t = `open;\nimport './b';\n"),
        ("src/b.ts", ""),
    ]);
    assert_eq!(skip_lines(&r.imports("src/a.ts")).len(), 1);
}

#[test]
fn a_file_with_several_errors_counts_once() {
    let r = repo(&[
        ("src/a.ts", "const = ;\nlet = ;\nimport './b';\n"),
        ("src/b.ts", ""),
    ]);
    assert_eq!(skip_lines(&r.imports("src/a.ts")).len(), 1);
}

#[test]
fn other_languages_are_counted_too() {
    let r = repo(&[("a.py", "import b\ndef (:\n"), ("b.py", "")]);
    assert_eq!(skip_lines(&r.imports("a.py")).len(), 1);
}

#[test]
fn a_clean_file_has_no_syntax_error_skip() {
    let r = repo(&[
        ("src/a.ts", "import './b';\nexport const x = 1;\n"),
        ("src/b.ts", ""),
    ]);
    assert_eq!(skip_lines(&r.imports("src/a.ts")), []);
}

#[test]
fn a_case_pattern_of_several_quoted_parts_is_valid_shell_and_not_unparsed() {
    // why: tree-sitter-bash reads `*"a"*"b"*)` as an error node, though `sh -n` accepts it.
    for script in [
        "case \"$x\" in *\"foo\"*\"--port \"*) echo hit;; esac\n. ./lib/b.sh\n",
        "case \"$x\" in a|*\"b\"*\"c\"*) echo hit;; esac\n. ./lib/b.sh\n",
        "case \"$x\" in\n  *\"a\"*\"b\"*) . ./lib/b.sh ;;\n  *) echo no ;;\nesac\n",
    ] {
        let r = repo(&[("scripts/a.sh", script), ("lib/b.sh", "echo b\n")]);
        let got = r.imports("scripts/a.sh");
        assert_eq!(skip_lines(&got), [], "{script}");
        assert!(
            got.resolved
                .iter()
                .any(|import| import.target == Path::new("lib/b.sh")),
            "{script}"
        );
    }
}

#[test]
fn a_shell_error_outside_a_case_pattern_is_still_unparsed() {
    let r = repo(&[
        ("scripts/a.sh", "echo $(( 1 +\n. ./lib/b.sh\n"),
        ("lib/b.sh", "echo b\n"),
    ]);
    assert_eq!(skip_lines(&r.imports("scripts/a.sh")).len(), 1);
}
