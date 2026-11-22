use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::model::Location;

fn known(paths: &[&str]) -> BTreeSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

fn py_at(source: &str, file: &str, repo: &[&str]) -> FileImports {
    extract(source, Language::Python, Path::new(file), &known(repo)).expect("extract succeeds")
}

fn py(source: &str, repo: &[&str]) -> FileImports {
    py_at(source, "app/main.py", repo)
}

fn targets(imports: &FileImports) -> Vec<String> {
    imports
        .resolved
        .iter()
        .map(|r| r.target.display().to_string())
        .collect()
}

fn lines(imports: &FileImports) -> Vec<u32> {
    imports.resolved.iter().map(|r| r.at.line).collect()
}

fn unresolved_specifiers(imports: &FileImports) -> Vec<&str> {
    imports
        .unresolved
        .iter()
        .map(|u| u.specifier.as_str())
        .collect()
}

#[test]
fn import_dotted_module() {
    let got = py("import a.b.c", &["a/b/c.py"]);
    assert_eq!(targets(&got), ["a/b/c.py"]);
}

#[test]
fn import_aliased_module() {
    let got = py("import a.b as x", &["a/b.py"]);
    assert_eq!(targets(&got), ["a/b.py"]);
}

#[test]
fn import_several_modules() {
    let got = py("import a, b", &["a.py", "b.py"]);
    assert_eq!(targets(&got), ["a.py", "b.py"]);
}

#[test]
fn import_several_modules_with_aliases() {
    let got = py("import a as x, b.c as y", &["a.py", "b/c.py"]);
    assert_eq!(targets(&got), ["a.py", "b/c.py"]);
}

#[test]
fn from_import_name() {
    let got = py("from a.b import c", &["a/b/c.py"]);
    assert_eq!(targets(&got), ["a/b/c.py"]);
}

#[test]
fn from_import_aliased_and_plain_names() {
    let got = py("from a.b import c as d, e", &["a/b/c.py", "a/b/e.py"]);
    assert_eq!(targets(&got), ["a/b/c.py", "a/b/e.py"]);
}

#[test]
fn from_import_parenthesised_names() {
    let source = "from a.b import (\n    c,\n    d as e,\n)\n";
    let got = py(source, &["a/b/c.py", "a/b/d.py"]);
    assert_eq!(targets(&got), ["a/b/c.py", "a/b/d.py"]);
    assert_eq!(lines(&got), [1, 1]);
}

#[test]
fn from_import_star() {
    let got = py("from a.b import *", &["a/b.py", "a/b/c.py"]);
    assert_eq!(targets(&got), ["a/b.py"]);
}

#[test]
fn relative_star_import_resolves_the_package() {
    let got = py_at("from . import *", "pkg/m.py", &["pkg/__init__.py"]);
    assert_eq!(targets(&got), ["pkg/__init__.py"]);
}

#[test]
fn from_dot_import_name() {
    let got = py_at("from . import x", "pkg/m.py", &["pkg/x.py"]);
    assert_eq!(targets(&got), ["pkg/x.py"]);
}

#[test]
fn from_dot_module_import_name() {
    let got = py_at("from .a import b", "pkg/m.py", &["pkg/a/b.py"]);
    assert_eq!(targets(&got), ["pkg/a/b.py"]);
}

#[test]
fn from_two_dots_dotted_module_import_name() {
    let got = py_at("from ..a.b import c", "pkg/sub/m.py", &["pkg/a/b/c.py"]);
    assert_eq!(targets(&got), ["pkg/a/b/c.py"]);
}

#[test]
fn from_two_dots_import_name() {
    let got = py_at("from .. import x", "pkg/sub/m.py", &["pkg/x.py"]);
    assert_eq!(targets(&got), ["pkg/x.py"]);
}

#[test]
fn import_inside_function_body_counts() {
    let source = "def f():\n    import a\n    from b import c\n";
    let got = py(source, &["a.py", "b/c.py"]);
    assert_eq!(targets(&got), ["a.py", "b/c.py"]);
    assert_eq!(lines(&got), [2, 3]);
}

#[test]
fn import_inside_type_checking_block_counts() {
    let source = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from a import b\n";
    let got = py(source, &["a/b.py"]);
    assert_eq!(targets(&got), ["a/b.py"]);
    assert_eq!(lines(&got), [3]);
}

#[test]
fn dynamic_imports_are_not_extracted() {
    let source = "__import__('a')\nimportlib.import_module('a')\nimport_module('a')\n";
    let got = py(source, &["a.py"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn comments_are_ignored() {
    let got = py(
        "# import a\n# from a import b\nx = 1  # import a\n",
        &["a.py"],
    );
    assert_eq!(got, FileImports::default());
}

#[test]
fn string_contents_are_ignored() {
    let source = "s = 'import a'\nd = \"\"\"\nimport a\nfrom a import b\n\"\"\"\n";
    let got = py(source, &["a.py", "a/b.py"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn module_resolves_to_file_or_package() {
    assert_eq!(targets(&py("import a.b", &["a/b.py"])), ["a/b.py"]);
    assert_eq!(
        targets(&py("import a.b", &["a/b/__init__.py"])),
        ["a/b/__init__.py"]
    );
}

#[test]
fn module_file_beats_package() {
    let got = py("import a.b", &["a/b.py", "a/b/__init__.py"]);
    assert_eq!(targets(&got), ["a/b.py"]);
}

#[test]
fn import_resolves_the_full_dotted_path_only() {
    let got = py("import a.b.c", &["a/b.py", "a/__init__.py", "a.py"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn src_root_is_the_fallback() {
    let got = py("import a.b\nfrom c import d", &["src/a/b.py", "src/c/d.py"]);
    assert_eq!(targets(&got), ["src/a/b.py", "src/c/d.py"]);
}

#[test]
fn repository_root_beats_src_root() {
    let got = py("import a", &["src/a.py", "a.py"]);
    assert_eq!(targets(&got), ["a.py"]);
}

#[test]
fn root_order_applies_before_submodule_order() {
    // why: the first root that has the package wins, even if a later root has the submodule.
    let got = py("from a import c", &["a/__init__.py", "src/a/c.py"]);
    assert_eq!(targets(&got), ["a/__init__.py"]);
}

#[test]
fn submodule_beats_the_module_itself() {
    let got = py("from a.b import c", &["a/b/c.py", "a/b.py"]);
    assert_eq!(targets(&got), ["a/b/c.py"]);
}

#[test]
fn submodule_package_beats_the_module_itself() {
    let got = py(
        "from a.b import c",
        &["a/b/c/__init__.py", "a/b/__init__.py"],
    );
    assert_eq!(targets(&got), ["a/b/c/__init__.py"]);
}

#[test]
fn submodule_file_beats_submodule_package() {
    let got = py("from a import b", &["a/b.py", "a/b/__init__.py"]);
    assert_eq!(targets(&got), ["a/b.py"]);
}

#[test]
fn name_inside_module_falls_back_to_the_module_file() {
    let got = py("from a.b import c", &["a/b.py"]);
    assert_eq!(targets(&got), ["a/b.py"]);
}

#[test]
fn name_inside_package_falls_back_to_its_init() {
    let got = py("from a.b import c", &["a/b/__init__.py"]);
    assert_eq!(targets(&got), ["a/b/__init__.py"]);
}

#[test]
fn each_imported_name_resolves_on_its_own() {
    let got = py("from a import c, d", &["a/c.py", "a/__init__.py"]);
    assert_eq!(targets(&got), ["a/__init__.py", "a/c.py"]);
}

#[test]
fn third_party_and_stdlib_are_ignored() {
    let source = "import os\nimport numpy.linalg\nfrom collections import OrderedDict\n\
                  from __future__ import annotations\nfrom a.b import c\n";
    let got = py(source, &["x.py"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn stub_files_are_not_resolved_to() {
    let got = py(
        "import a\nfrom b import c",
        &["a.pyi", "b/c.pyi", "b/__init__.pyi"],
    );
    assert_eq!(got, FileImports::default());
}

#[test]
fn non_python_files_are_not_resolved_to() {
    let got = py("import a", &["a.ts", "a.js"]);
    assert_eq!(got, FileImports::default());
}

#[test]
fn one_dot_means_the_importers_directory() {
    let got = py_at("from .a import b", "pkg/sub/m.py", &["pkg/sub/a.py"]);
    assert_eq!(targets(&got), ["pkg/sub/a.py"]);
}

#[test]
fn two_dots_go_up_one_directory() {
    let got = py_at("from ..a import b", "pkg/sub/m.py", &["pkg/a.py"]);
    assert_eq!(targets(&got), ["pkg/a.py"]);
    let miss = py_at("from ..a import b", "pkg/sub/m.py", &["pkg/sub/a.py"]);
    assert_eq!(unresolved_specifiers(&miss), ["..a"]);
}

#[test]
fn three_dots_go_up_two_directories() {
    let got = py_at("from ...a import b", "pkg/sub/deep/m.py", &["pkg/a.py"]);
    assert_eq!(targets(&got), ["pkg/a.py"]);
}

#[test]
fn relative_name_prefers_submodule_then_module_then_package() {
    let importer = "pkg/m.py";
    let all = py_at(
        "from .a import b",
        importer,
        &["pkg/a/b.py", "pkg/a.py", "pkg/a/__init__.py"],
    );
    assert_eq!(targets(&all), ["pkg/a/b.py"]);
    let no_submodule = py_at(
        "from .a import b",
        importer,
        &["pkg/a.py", "pkg/a/__init__.py"],
    );
    assert_eq!(targets(&no_submodule), ["pkg/a.py"]);
    let package_only = py_at("from .a import b", importer, &["pkg/a/__init__.py"]);
    assert_eq!(targets(&package_only), ["pkg/a/__init__.py"]);
}

#[test]
fn dot_import_prefers_submodule_then_package_init() {
    let importer = "pkg/m.py";
    let sub_file = py_at(
        "from . import x",
        importer,
        &["pkg/x.py", "pkg/x/__init__.py", "pkg/__init__.py"],
    );
    assert_eq!(targets(&sub_file), ["pkg/x.py"]);
    let sub_package = py_at(
        "from . import x",
        importer,
        &["pkg/x/__init__.py", "pkg/__init__.py"],
    );
    assert_eq!(targets(&sub_package), ["pkg/x/__init__.py"]);
    let init = py_at("from . import x", importer, &["pkg/__init__.py"]);
    assert_eq!(targets(&init), ["pkg/__init__.py"]);
}

#[test]
fn relative_import_ignores_the_src_root() {
    let got = py_at("from .a import b", "pkg/m.py", &["src/pkg/a.py"]);
    assert_eq!(unresolved_specifiers(&got), [".a"]);
}

#[test]
fn unresolved_relative_import_carries_the_raw_specifier() {
    let got = py_at("from .a import b", "pkg/m.py", &["pkg/other.py"]);
    assert_eq!(targets(&got), Vec::<String>::new());
    assert_eq!(unresolved_specifiers(&got), [".a"]);
    let nested = py_at("from ..a.b import c", "pkg/sub/m.py", &[]);
    assert_eq!(unresolved_specifiers(&nested), ["..a.b"]);
    let bare = py_at("from . import x", "pkg/m.py", &[]);
    assert_eq!(unresolved_specifiers(&bare), ["."]);
    let bare_two = py_at("from .. import x", "pkg/sub/m.py", &[]);
    assert_eq!(unresolved_specifiers(&bare_two), [".."]);
}

#[test]
fn unresolved_relative_import_reports_the_module_line() {
    let got = py_at("import os\nfrom .a import (\n    b,\n)\n", "pkg/m.py", &[]);
    assert_eq!(got.unresolved.len(), 1);
    assert_eq!(got.unresolved.first().map(|u| u.at.line), Some(2));
}

#[test]
fn climbing_above_the_repository_root_is_unresolved() {
    let top = py_at("from .. import x", "m.py", &["x.py"]);
    assert_eq!(targets(&top), Vec::<String>::new());
    assert_eq!(unresolved_specifiers(&top), [".."]);
    let nested = py_at("from ...a import b", "pkg/m.py", &["a.py", "a/b.py"]);
    assert_eq!(targets(&nested), Vec::<String>::new());
    assert_eq!(unresolved_specifiers(&nested), ["...a"]);
}

#[test]
fn exactly_reaching_the_repository_root_resolves() {
    let got = py_at("from .. import x", "pkg/m.py", &["x.py"]);
    assert_eq!(targets(&got), ["x.py"]);
    let sibling = py_at("from . import x", "m.py", &["x.py"]);
    assert_eq!(targets(&sibling), ["x.py"]);
}

#[test]
fn relative_stub_is_not_resolved_to() {
    let got = py_at("from . import x", "pkg/m.py", &["pkg/x.pyi"]);
    assert_eq!(unresolved_specifiers(&got), ["."]);
}

#[test]
fn one_unresolved_name_does_not_hide_a_resolved_one() {
    let got = py_at("from .a import b, c", "pkg/m.py", &["pkg/a/b.py"]);
    assert_eq!(targets(&got), ["pkg/a/b.py"]);
    assert_eq!(unresolved_specifiers(&got), [".a"]);
}

#[test]
fn importing_the_importer_is_dropped() {
    let in_init = py_at("from . import x", "pkg/__init__.py", &["pkg/__init__.py"]);
    assert_eq!(in_init, FileImports::default());
    let by_name = py_at("import pkg.m", "pkg/m.py", &["pkg/m.py"]);
    assert_eq!(by_name, FileImports::default());
    let relative = py_at("from .m import f", "pkg/m.py", &["pkg/m.py"]);
    assert_eq!(relative, FileImports::default());
}

#[test]
fn line_is_the_line_the_statement_starts_on() {
    let source = "\n\nfrom \\\n    a.b import c\n";
    let got = py(source, &["a/b/c.py"]);
    assert_eq!(lines(&got), [3]);
}

#[test]
fn a_multi_line_from_import_and_a_multi_line_import_start_on_their_first_line() {
    let source = "from a import (\n    b,\n)\nimport \\\n    a.b\n";
    let got = py(source, &["a/__init__.py", "a/b.py"]);
    assert_eq!(lines(&got), [1, 4]);
}

#[test]
fn location_carries_the_importing_file() {
    let got = py("import a", &["a.py"]);
    assert_eq!(
        got.resolved.first().map(|r| r.at.clone()),
        Some(Location {
            file: PathBuf::from("app/main.py"),
            line: 1
        })
    );
}

#[test]
fn duplicates_collapse() {
    let source = "import a\nimport a; import a\nfrom a import *\n";
    let got = py(source, &["a.py"]);
    assert_eq!(lines(&got), [1, 2, 3]);
    let same_line = py("import a; import a", &["a.py"]);
    assert_eq!(lines(&same_line), [1]);
    let missing = py_at("from .x import a; from .x import b", "p/m.py", &[]);
    assert_eq!(missing.unresolved.len(), 1);
}

#[test]
fn output_is_sorted() {
    let source = "import b\nimport a\nfrom .zz import q\nfrom .yy import q\n";
    let got = py_at(source, "m.py", &["a.py", "b.py"]);
    let mut sorted = got.resolved.clone();
    sorted.sort();
    assert_eq!(got.resolved, sorted);
    assert_eq!(targets(&got), ["b.py", "a.py"]);
    let mut sorted = got.unresolved.clone();
    sorted.sort();
    assert_eq!(got.unresolved, sorted);
    assert_eq!(unresolved_specifiers(&got), [".zz", ".yy"]);
}

#[test]
fn syntax_error_still_yields_valid_imports() {
    let source = "import a\ndef broken(:\n    pass\nfrom b import c\n";
    let got = py(source, &["a.py", "b/c.py"]);
    assert_eq!(targets(&got), ["a.py", "b/c.py"]);
}

#[test]
fn empty_source_has_no_imports() {
    assert_eq!(py("", &[]), FileImports::default());
}

#[test]
fn collect_reads_python_and_typescript_together() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = [
        ("pkg/__init__.py", "", Language::Python),
        (
            "pkg/a.py",
            "from . import b\nfrom .missing import x\n",
            Language::Python,
        ),
        ("pkg/b.py", "import os\n", Language::Python),
        ("src/lib/util.py", "", Language::Python),
        (
            "main.py",
            "from lib import util\nimport pkg.a\n",
            Language::Python,
        ),
        (
            "web/app.ts",
            "import './other';\nimport 'pkg/a';\n",
            Language::TypeScript,
        ),
        (
            "web/other.ts",
            "export const x = 1;\n",
            Language::TypeScript,
        ),
    ];
    let mut map = BTreeMap::new();
    for (path, content, language) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(&full, content).expect("write");
        map.insert(PathBuf::from(path), language);
    }

    let first = collect(dir.path(), &map).expect("collect");
    let second = collect(dir.path(), &map).expect("collect again");
    assert_eq!(first, second);

    let a = first.get(Path::new("pkg/a.py")).expect("a.py");
    assert_eq!(targets(a), ["pkg/b.py"]);
    assert_eq!(unresolved_specifiers(a), [".missing"]);
    let main = first.get(Path::new("main.py")).expect("main.py");
    assert_eq!(targets(main), ["src/lib/util.py", "pkg/a.py"]);
    let app = first.get(Path::new("web/app.ts")).expect("app.ts");
    assert_eq!(targets(app), ["web/other.ts"]);
    let b = first.get(Path::new("pkg/b.py")).expect("b.py");
    assert_eq!(*b, FileImports::default());
}
