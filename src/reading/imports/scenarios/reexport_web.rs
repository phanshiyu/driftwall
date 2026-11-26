//! TypeScript, JavaScript and Python re-exports followed to the defining file.

use super::fixtures::repo;
use super::reexport::{site, sites};
use crate::code::ImportKind;

const BARREL: &[(&str, &str)] = &[
    (
        "src/ui/index.ts",
        "export { Tone } from './tone';\nexport { Mode as Style, Theme } from './other';\nexport type { Shape } from './shape';\nexport { default as Icon } from './icon';\nexport * as shapes from './shape';\n",
    ),
    ("src/ui/tone.ts", "export const Tone = 1;\n"),
    (
        "src/ui/other.ts",
        "export const Mode = 1;\nexport const Theme = 2;\n",
    ),
    ("src/ui/shape.ts", "export interface Shape {}\n"),
    ("src/ui/icon.ts", "export default function icon() {}\n"),
];

fn with_app(app: &str) -> Vec<(&str, &str)> {
    let mut files = BARREL.to_vec();
    files.push(("src/app.ts", app));
    files
}

#[test]
fn ts_named_reexport_is_followed() {
    let got = sites(&with_app("import { Tone } from './ui';\n"), "src/app.ts");
    assert_eq!(got, [site(1, "src/ui/tone.ts", "Tone")]);
}

#[test]
fn ts_rename_type_only_and_default_are_followed() {
    let got = sites(
        &with_app(
            "import { Style, Theme, type Shape } from './ui';\nimport { Icon } from './ui';\n",
        ),
        "src/app.ts",
    );
    assert_eq!(
        got,
        [
            site(1, "src/ui/other.ts", "Mode,Theme"),
            site(1, "src/ui/shape.ts", "Shape"),
            site(2, "src/ui/icon.ts", "default"),
        ]
    );
}

#[test]
fn ts_type_only_kind_survives_the_move() {
    let files = with_app("import type { Tone } from './ui';\n");
    let imports = repo(&files).imports("src/app.ts");
    let [import] = imports.resolved.as_slice() else {
        panic!("one import expected");
    };
    assert_eq!(import.target.display().to_string(), "src/ui/tone.ts");
    assert_eq!(import.kind, ImportKind::TypeOnly);
}

#[test]
fn ts_namespace_import_stays_on_the_barrel_and_a_namespace_reexport_is_the_whole_module() {
    let got = sites(
        &with_app("import * as ui from './ui';\nimport { shapes } from './ui';\nimport './ui';\n"),
        "src/app.ts",
    );
    assert_eq!(
        got,
        [
            site(1, "src/ui/index.ts", "*"),
            site(2, "src/ui/shape.ts", "*"),
            site(3, "src/ui/index.ts", ""),
        ]
    );
}

#[test]
fn ts_barrel_keeps_its_own_edges() {
    let got = sites(&with_app(""), "src/ui/index.ts");
    assert_eq!(got[0], site(1, "src/ui/tone.ts", "Tone"));
}

#[test]
fn ts_chain_of_barrels_is_followed() {
    let files = [
        ("src/index.ts", "export { Tone } from './ui';\n"),
        ("src/ui/index.ts", "export { Tone } from './tone';\n"),
        ("src/ui/tone.ts", "export class Tone {}\n"),
        ("src/app.ts", "import { Tone } from './index';\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.ts"),
        [site(1, "src/ui/tone.ts", "Tone")]
    );
    assert_eq!(
        sites(&files, "src/index.ts"),
        [site(1, "src/ui/index.ts", "Tone")]
    );
}

#[test]
fn ts_cycle_stays_on_the_barrel() {
    let files = [
        ("src/a.ts", "export { X } from './b';\n"),
        ("src/b.ts", "export { X } from './a';\n"),
        ("src/app.ts", "import { X } from './a';\n"),
    ];
    assert_eq!(sites(&files, "src/app.ts"), [site(1, "src/a.ts", "X")]);
}

#[test]
fn ts_name_the_barrel_defines_stays_local() {
    let files = [
        (
            "src/ui/index.ts",
            "export { Tone } from './tone';\nexport const Tone = 2;\n",
        ),
        ("src/ui/tone.ts", "export const Tone = 1;\n"),
        ("src/app.ts", "import { Tone } from './ui';\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.ts"),
        [site(1, "src/ui/index.ts", "Tone")]
    );
}

#[test]
fn ts_star_is_followed_only_when_one_source_can_define_the_name() {
    let files = [
        (
            "src/ui/index.ts",
            "export * from './tone';\nexport * from './size';\n",
        ),
        ("src/ui/tone.ts", "export const Tone = 1;\n"),
        (
            "src/ui/size.ts",
            "export function Size() {}\nexport const Both = 1;\n",
        ),
        ("src/ui/more.ts", "export const Both = 2;\n"),
        (
            "src/app.ts",
            "import { Tone, Size, Both, Nope } from './ui';\nimport D from './ui';\n",
        ),
    ];
    assert_eq!(
        sites(&files, "src/app.ts"),
        [
            site(1, "src/ui/index.ts", "Nope"),
            site(1, "src/ui/size.ts", "Both,Size"),
            site(1, "src/ui/tone.ts", "Tone"),
            site(2, "src/ui/index.ts", "default"),
        ]
    );
}

#[test]
fn ts_star_with_two_sources_defining_the_name_stays() {
    let files = [
        (
            "src/ui/index.ts",
            "export * from './a';\nexport * from './b';\n",
        ),
        ("src/ui/a.ts", "export const Same = 1;\n"),
        ("src/ui/b.ts", "export const Same = 2;\n"),
        ("src/app.ts", "import { Same } from './ui';\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.ts"),
        [site(1, "src/ui/index.ts", "Same")]
    );
}

#[test]
fn ts_star_through_a_named_barrel_follows_the_chain() {
    let files = [
        ("src/ui/index.ts", "export * from './inner';\n"),
        ("src/ui/inner/index.ts", "export { Leaf } from './leaf';\n"),
        ("src/ui/inner/leaf.ts", "export class Leaf {}\n"),
        ("src/app.ts", "import { Leaf } from './ui';\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.ts"),
        [site(1, "src/ui/inner/leaf.ts", "Leaf")]
    );
}

#[test]
fn js_barrel_with_a_default_reexport_is_followed() {
    let files = [
        ("lib/index.js", "export { default } from './main.js';\n"),
        ("lib/main.js", "export default function main() {}\n"),
        ("app.js", "import main from './lib/index.js';\n"),
    ];
    assert_eq!(sites(&files, "app.js"), [site(1, "lib/main.js", "default")]);
}

const PACKAGE: &[(&str, &str)] = &[
    (
        "pkg/__init__.py",
        "from .tone import Tone\nfrom .other import Mode as Style, Theme\nfrom . import helpers\nfrom .wide import *\n",
    ),
    ("pkg/tone.py", "class Tone: pass\n"),
    ("pkg/other.py", "Mode = 1\nTheme = 2\n"),
    ("pkg/helpers.py", "def h(): pass\n"),
    ("pkg/wide.py", "def draw(): pass\n_hidden = 1\n"),
];

fn with_main(main: &str) -> Vec<(&str, &str)> {
    let mut files = PACKAGE.to_vec();
    files.push(("main.py", main));
    files
}

#[test]
fn python_init_reexports_are_followed() {
    let got = sites(
        &with_main("from pkg import Tone, Style, Theme, draw\n"),
        "main.py",
    );
    assert_eq!(
        got,
        [
            site(1, "pkg/other.py", "Mode,Theme"),
            site(1, "pkg/tone.py", "Tone"),
            site(1, "pkg/wide.py", "draw"),
        ]
    );
}

#[test]
fn python_module_bindings_and_private_star_names_stay() {
    let got = sites(
        &with_main("from pkg import helpers, _hidden\nimport pkg\n"),
        "main.py",
    );
    assert_eq!(
        got,
        [
            site(1, "pkg/__init__.py", "_hidden"),
            site(1, "pkg/helpers.py", ""),
            site(2, "pkg/__init__.py", ""),
        ]
    );
}

#[test]
fn python_init_keeps_its_own_edges() {
    let got = sites(&with_main(""), "pkg/__init__.py");
    assert_eq!(got[0], site(1, "pkg/tone.py", "Tone"));
}

#[test]
fn python_local_definition_wins_over_a_reexport() {
    let files = [
        (
            "pkg/__init__.py",
            "from .tone import Tone\nclass Tone: pass\n",
        ),
        ("pkg/tone.py", "class Tone: pass\n"),
        ("main.py", "from pkg import Tone\n"),
    ];
    assert_eq!(
        sites(&files, "main.py"),
        [site(1, "pkg/__init__.py", "Tone")]
    );
}

#[test]
fn python_all_limits_what_a_star_exports() {
    let files = [
        ("pkg/__init__.py", "from .a import *\nfrom .b import *\n"),
        (
            "pkg/a.py",
            "__all__ = ['one']\ndef one(): pass\ndef two(): pass\n",
        ),
        ("pkg/b.py", "def two(): pass\n"),
        ("main.py", "from pkg import one, two\n"),
    ];
    assert_eq!(
        sites(&files, "main.py"),
        [site(1, "pkg/a.py", "one"), site(1, "pkg/b.py", "two")]
    );
}

#[test]
fn python_chain_and_cycle() {
    let files = [
        ("a/__init__.py", "from b import Deep\n"),
        ("b/__init__.py", "from .c import Deep\n"),
        ("b/c.py", "class Deep: pass\n"),
        ("x/__init__.py", "from y import Loop\n"),
        ("y/__init__.py", "from x import Loop\n"),
        ("main.py", "from a import Deep\nfrom x import Loop\n"),
    ];
    assert_eq!(
        sites(&files, "main.py"),
        [site(1, "b/c.py", "Deep"), site(2, "x/__init__.py", "Loop")]
    );
}

#[test]
fn python_other_modules_are_not_facades() {
    let files = [
        ("pkg/__init__.py", ""),
        ("pkg/mod.py", "from .tone import Tone\n"),
        ("pkg/tone.py", "class Tone: pass\n"),
        ("main.py", "from pkg.mod import Tone\n"),
    ];
    assert_eq!(sites(&files, "main.py"), [site(1, "pkg/mod.py", "Tone")]);
}

#[test]
fn python_a_submodule_reexported_by_init_is_followed_into_the_module() {
    let files = [
        ("app/__init__.py", ""),
        ("app/a/__init__.py", ""),
        ("app/a/impl.py", "class A: pass\n"),
        ("app/c/__init__.py", "from app.a import impl\n"),
        (
            "app/b/x.py",
            "from app.c import impl\nfrom app.c import other\n",
        ),
    ];
    assert_eq!(
        sites(&files, "app/b/x.py"),
        [
            site(1, "app/a/impl.py", "*"),
            site(2, "app/c/__init__.py", "other")
        ]
    );
}
