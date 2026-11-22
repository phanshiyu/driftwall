//! Python import roots, first-party misses and literal dynamic imports, through [`collect`] on
//! repositories written to disk.

use super::fixtures::{Repo, pairs, repo};
use super::*;
use crate::model::{ImportKind, SkipReason};

/// The files every configured-root test shares: a package `mypkg` below `lib/` and a package
/// `tools` that imports it, so only a configured root can reach it.
const LIB: [(&str, &str); 4] = [
    ("lib/mypkg/__init__.py", ""),
    ("lib/mypkg/core.py", ""),
    ("tools/__init__.py", ""),
    ("tools/run.py", "import mypkg.core\n"),
];

fn with_config(config: &[(&str, &str)]) -> Repo {
    let all: Vec<(&str, &str)> = config.iter().chain(LIB.iter()).copied().collect();
    repo(&all)
}

fn reaches_lib(config: &[(&str, &str)]) {
    let repo = with_config(config);
    assert_eq!(
        repo.edges("tools/run.py"),
        pairs(&[(1, "lib/mypkg/core.py")])
    );
    assert_eq!(repo.skipped("tools/run.py"), Vec::new());
}

#[test]
fn a_module_outside_every_root_is_unreachable_without_configuration() {
    let repo = with_config(&[]);
    assert_eq!(repo.edges("tools/run.py"), Vec::new());
    let skipped = repo.imports("tools/run.py").skipped;
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].reason, SkipReason::UnreachableModule);
    assert_eq!(skipped[0].specifier, "mypkg.core");
    assert_eq!(
        skipped[0].message.as_deref(),
        Some("`mypkg` matches `lib/mypkg`, which no import root of `tools/run.py` reaches")
    );
}

#[test]
fn pytest_pythonpath_in_pyproject_is_a_root() {
    reaches_lib(&[(
        "pyproject.toml",
        "[tool.pytest.ini_options]\npythonpath = [\"scripts\", \"lib\"]\n",
    )]);
}

#[test]
fn pyright_and_basedpyright_extra_paths_in_pyproject_are_roots() {
    reaches_lib(&[("pyproject.toml", "[tool.pyright]\nextraPaths = [\"lib\"]\n")]);
    reaches_lib(&[(
        "pyproject.toml",
        "[tool.basedpyright]\nextraPaths = [\"lib\"]\n",
    )]);
}

#[test]
fn setuptools_where_is_a_root() {
    reaches_lib(&[(
        "pyproject.toml",
        "[tool.setuptools.packages.find]\nwhere = [\"lib\"]\n",
    )]);
}

#[test]
fn setuptools_package_dir_for_the_empty_key_is_a_root() {
    reaches_lib(&[(
        "pyproject.toml",
        "[tool.setuptools]\npackage-dir = {\"\" = \"lib\"}\n",
    )]);
    reaches_lib(&[(
        "pyproject.toml",
        "[tool.setuptools.package-dir]\n\"\" = \"lib\"\n",
    )]);
}

#[test]
fn hatch_wheel_packages_give_the_directory_they_sit_in() {
    reaches_lib(&[(
        "pyproject.toml",
        "[tool.hatch.build.targets.wheel]\npackages = [\"lib/mypkg\"]\n",
    )]);
}

#[test]
fn poetry_packages_from_is_a_root() {
    reaches_lib(&[(
        "pyproject.toml",
        "[tool.poetry]\npackages = [{include = \"mypkg\", from = \"lib\"}]\n",
    )]);
}

#[test]
fn setup_cfg_package_dir_and_find_where_are_roots() {
    reaches_lib(&[(
        "setup.cfg",
        "[options]\npackage_dir =\n    =lib\npackages = find:\n",
    )]);
    reaches_lib(&[(
        "setup.cfg",
        "[options]\npackages = find:\n\n[options.packages.find]\nwhere = lib\n",
    )]);
}

#[test]
fn pytest_pythonpath_in_setup_cfg_and_pytest_ini_is_a_root() {
    reaches_lib(&[("setup.cfg", "[tool:pytest]\npythonpath = scripts lib\n")]);
    reaches_lib(&[(
        "pytest.ini",
        "[pytest]\npythonpath =\n    scripts\n    lib\n",
    )]);
}

#[test]
fn pyrightconfig_extra_paths_are_roots() {
    reaches_lib(&[(
        "pyrightconfig.json",
        "{\n  // search path\n  \"extraPaths\": [\"lib\"],\n}\n",
    )]);
}

#[test]
fn a_malformed_config_is_skipped_and_the_others_still_count() {
    reaches_lib(&[
        (
            "pyproject.toml",
            "[tool.pytest.ini_options\npythonpath = [\n",
        ),
        ("pytest.ini", "[pytest]\npythonpath = lib\n"),
        ("pyrightconfig.json", "{ not json"),
    ]);
}

#[test]
fn a_setting_of_the_wrong_type_is_skipped() {
    let repo = with_config(&[(
        "pyproject.toml",
        "[tool.pytest.ini_options]\npythonpath = 3\n[tool.setuptools]\npackage-dir = \"lib\"\n",
    )]);
    assert_eq!(repo.edges("tools/run.py"), Vec::new());
}

#[test]
fn configured_roots_come_before_the_repository_root() {
    let repo = repo(&[
        ("pytest.ini", "[pytest]\npythonpath = lib\n"),
        ("util.py", ""),
        ("lib/util.py", ""),
        ("app/__init__.py", ""),
        ("app/main.py", "import util\n"),
    ]);
    assert_eq!(repo.edges("app/main.py"), pairs(&[(1, "lib/util.py")]));
}

#[test]
fn a_nested_configuration_applies_below_its_directory_only() {
    let repo = repo(&[
        (
            "svc/pyproject.toml",
            "[tool.pytest.ini_options]\npythonpath = [\"lib\"]\n",
        ),
        ("svc/lib/util.py", ""),
        ("svc/app/__init__.py", ""),
        ("svc/app/main.py", "import util\n"),
        ("other/__init__.py", ""),
        ("other/main.py", "import util\n"),
    ]);
    assert_eq!(
        repo.edges("svc/app/main.py"),
        pairs(&[(1, "svc/lib/util.py")])
    );
    assert_eq!(repo.edges("other/main.py"), Vec::new());
    assert_eq!(repo.skipped("other/main.py"), pairs(&[(1, "util")]));
}

#[test]
fn a_config_in_a_directory_without_a_pyproject_is_not_read() {
    let repo = repo(&[
        ("svc/pytest.ini", "[pytest]\npythonpath = lib\n"),
        ("svc/lib/util.py", ""),
        ("svc/app/__init__.py", ""),
        ("svc/app/main.py", "import util\n"),
    ]);
    assert_eq!(repo.edges("svc/app/main.py"), Vec::new());
}

#[test]
fn a_root_that_leaves_the_repository_is_dropped() {
    let repo = repo(&[
        ("pytest.ini", "[pytest]\npythonpath = .. /abs\n"),
        ("app/__init__.py", ""),
        ("app/main.py", "import os\n"),
    ]);
    assert_eq!(repo.imports("app/main.py"), FileImports::default());
}

#[test]
fn sibling_scripts_in_a_directory_without_init_import_each_other() {
    let repo = repo(&[
        ("scripts/tuner.py", ""),
        ("scripts/charts.py", ""),
        (
            "scripts/regimes.py",
            "from tuner import parse\nimport charts\n",
        ),
    ]);
    assert_eq!(
        repo.edges("scripts/regimes.py"),
        pairs(&[(1, "scripts/tuner.py"), (2, "scripts/charts.py")])
    );
    assert_eq!(repo.skipped("scripts/regimes.py"), Vec::new());
}

#[test]
fn pytest_tests_import_their_siblings() {
    let repo = repo(&[
        ("tests/setup_support.py", ""),
        ("tests/test_guardian.py", ""),
        (
            "tests/test_main.py",
            "from setup_support import helper\nimport test_guardian\n",
        ),
    ]);
    assert_eq!(
        repo.edges("tests/test_main.py"),
        pairs(&[(1, "tests/setup_support.py"), (2, "tests/test_guardian.py")])
    );
    assert!(
        repo.imports("tests/test_main.py")
            .resolved
            .iter()
            .all(|i| i.test_only)
    );
}

#[test]
fn siblings_in_flat_script_directories_are_found() {
    let repo = repo(&[
        ("ir/mitsubishi.py", ""),
        ("ir/aircon_ir.py", "import mitsubishi\n"),
        ("tuya/rf_fan_codes.py", ""),
        (
            "tuya/make_ceiling_fan.py",
            "from rf_fan_codes import CODES\n",
        ),
    ]);
    assert_eq!(
        repo.edges("ir/aircon_ir.py"),
        pairs(&[(1, "ir/mitsubishi.py")])
    );
    assert_eq!(
        repo.edges("tuya/make_ceiling_fan.py"),
        pairs(&[(1, "tuya/rf_fan_codes.py")])
    );
}

#[test]
fn own_directory_comes_before_the_repository_root() {
    let repo = repo(&[
        ("util.py", ""),
        ("scripts/util.py", ""),
        ("scripts/run.py", "import util\n"),
    ]);
    assert_eq!(
        repo.edges("scripts/run.py"),
        pairs(&[(1, "scripts/util.py")])
    );
}

#[test]
fn a_file_in_a_package_directory_does_not_use_its_own_directory_as_a_root() {
    let repo = repo(&[
        ("pkg/__init__.py", ""),
        ("pkg/a.py", "import b\nfrom . import b as c\n"),
        ("pkg/b.py", ""),
    ]);
    assert_eq!(repo.edges("pkg/a.py"), pairs(&[(2, "pkg/b.py")]));
    let skipped = repo.imports("pkg/a.py").skipped;
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].at.line, 1);
    assert_eq!(skipped[0].reason, SkipReason::UnreachableModule);
    assert_eq!(
        skipped[0].message.as_deref(),
        Some("`b` matches `pkg/b.py`, which no import root of `pkg/a.py` reaches")
    );
}

#[test]
fn a_script_reachable_only_through_a_makefile_pythonpath_is_unreachable() {
    let repo = repo(&[
        ("Makefile", "export PYTHONPATH=scripts\n"),
        ("scripts/tuner.py", "def parse(): ...\n"),
        ("vendorkit/__init__.py", ""),
        ("vendorkit/bt/__init__.py", ""),
        (
            "vendorkit/bt/parity.py",
            "\n\nfrom tuner import parse\n",
        ),
    ]);
    let imports = repo.imports("vendorkit/bt/parity.py");
    assert_eq!(imports, {
        let mut expected = FileImports::default();
        expected.skipped.push(crate::model::Skipped {
            at: crate::model::Location {
                file: "vendorkit/bt/parity.py".into(),
                line: 3,
            },
            specifier: "tuner".to_owned(),
            reason: SkipReason::UnreachableModule,
            message: Some(
                "`tuner` matches `scripts/tuner.py`, which no import root of `vendorkit/bt/parity.py` reaches"
                    .to_owned(),
            ),
        });
        expected
    });
}

#[test]
fn the_same_import_resolves_once_pythonpath_is_configured() {
    let repo = repo(&[
        (
            "pyproject.toml",
            "[tool.pytest.ini_options]\npythonpath = [\"scripts\", \".\"]\n",
        ),
        ("scripts/tuner.py", "def parse(): ...\n"),
        ("vendorkit/__init__.py", ""),
        ("vendorkit/bt/__init__.py", ""),
        ("vendorkit/bt/parity.py", "from tuner import parse\n"),
    ]);
    let imports = repo.imports("vendorkit/bt/parity.py");
    assert_eq!(imports.skipped, Vec::new());
    assert_eq!(imports.resolved.len(), 1);
    assert_eq!(
        imports.resolved[0].target.to_str(),
        Some("scripts/tuner.py")
    );
    assert_eq!(
        imports.resolved[0]
            .symbols
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["parse"]
    );
}

#[test]
fn a_missing_module_of_a_first_party_package_is_unresolved() {
    let repo = repo(&[
        ("app/__init__.py", ""),
        ("app/core/__init__.py", ""),
        ("app/core/real.py", ""),
        (
            "main.py",
            "from app.core.typo import x\nimport app.nothing\nfrom app.core import real\nfrom app.core import name_in_init\n",
        ),
    ]);
    assert_eq!(
        repo.unresolved("main.py"),
        pairs(&[(1, "app.core.typo"), (2, "app.nothing")])
    );
    assert_eq!(
        repo.edges("main.py"),
        pairs(&[(3, "app/core/real.py"), (4, "app/core/__init__.py")])
    );
    assert_eq!(repo.skipped("main.py"), Vec::new());
}

#[test]
fn a_missing_module_below_a_directory_without_init_is_unresolved() {
    let repo = repo(&[
        ("ns/pkg/real.py", ""),
        (
            "main.py",
            "import ns.pkg.typo\nfrom ns.pkg import typo\nimport ns.pkg\n",
        ),
    ]);
    assert_eq!(repo.unresolved("main.py"), pairs(&[(1, "ns.pkg.typo")]));
    assert_eq!(repo.edges("main.py"), Vec::new());
}

#[test]
fn a_first_party_module_file_with_a_missing_submodule_is_unresolved() {
    let repo = repo(&[("a.py", ""), ("main.py", "import a.b\n")]);
    assert_eq!(repo.unresolved("main.py"), pairs(&[(1, "a.b")]));
}

#[test]
fn first_party_misses_are_found_through_configured_roots_too() {
    let repo = repo(&[
        ("pytest.ini", "[pytest]\npythonpath = lib\n"),
        ("lib/mypkg/__init__.py", ""),
        ("tools/__init__.py", ""),
        ("tools/run.py", "import mypkg.typo\n"),
    ]);
    assert_eq!(repo.unresolved("tools/run.py"), pairs(&[(1, "mypkg.typo")]));
}

#[test]
fn third_party_and_standard_library_stay_ignored() {
    let repo = repo(&[
        ("lib/mypkg/__init__.py", ""),
        (
            "main.py",
            "import os\nimport numpy.linalg\nfrom collections import OrderedDict\nimport importlib\nimportlib.import_module('yaml.nodes')\n",
        ),
    ]);
    assert_eq!(repo.imports("main.py"), FileImports::default());
}

#[test]
fn a_literal_dynamic_import_resolves_like_an_import() {
    let repo = repo(&[
        ("pkg/__init__.py", ""),
        ("pkg/mod.py", ""),
        (
            "main.py",
            "import importlib\nm = importlib.import_module(\"pkg.mod\")\nn = __import__('pkg.mod')\nif x:\n    importlib.import_module('pkg.typo')\n",
        ),
    ]);
    let imports = repo.imports("main.py");
    assert_eq!(imports.resolved.len(), 2);
    for (import, line) in imports.resolved.iter().zip([2, 3]) {
        assert_eq!(import.at.line, line);
        assert_eq!(import.target.to_str(), Some("pkg/mod.py"));
        assert_eq!(import.kind, ImportKind::Runtime);
        assert!(import.symbols.is_empty());
    }
    assert_eq!(
        imports
            .unresolved
            .iter()
            .map(|u| (u.at.line, u.specifier.as_str()))
            .collect::<Vec<_>>(),
        [(5, "pkg.typo")]
    );
    assert_eq!(imports.skipped, Vec::new());
}

#[test]
fn a_literal_dynamic_import_follows_the_roots_and_reports_unreachable_modules() {
    let repo = repo(&[
        ("scripts/tuner.py", ""),
        ("app/__init__.py", ""),
        (
            "app/main.py",
            "import importlib\nimportlib.import_module('tuner')\n",
        ),
    ]);
    let skipped = repo.imports("app/main.py").skipped;
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].at.line, 2);
    assert_eq!(skipped[0].specifier, "tuner");
    assert_eq!(skipped[0].reason, SkipReason::UnreachableModule);
}

#[test]
fn a_relative_literal_resolves_against_a_literal_package() {
    let repo = repo(&[
        ("pkg/__init__.py", ""),
        ("pkg/x.py", ""),
        ("pkg/sub/__init__.py", ""),
        ("pkg/sub/y.py", ""),
        (
            "main.py",
            "import importlib\nimportlib.import_module('.x', package='pkg')\nimportlib.import_module('.y', 'pkg.sub')\nimportlib.import_module('..x', package='pkg.sub')\nimportlib.import_module('.', package='pkg')\nimportlib.import_module('.missing', 'pkg')\n",
        ),
    ]);
    let imports = repo.imports("main.py");
    let targets: Vec<(u32, &str)> = imports
        .resolved
        .iter()
        .map(|i| (i.at.line, i.target.to_str().unwrap_or_default()))
        .collect();
    assert_eq!(
        targets,
        [
            (2, "pkg/x.py"),
            (3, "pkg/sub/y.py"),
            (4, "pkg/x.py"),
            (5, "pkg/__init__.py"),
        ]
    );
    assert_eq!(
        imports
            .unresolved
            .iter()
            .map(|u| (u.at.line, u.specifier.as_str()))
            .collect::<Vec<_>>(),
        [(6, ".missing")]
    );
    assert_eq!(imports.skipped, Vec::new());
}

#[test]
fn every_other_dynamic_import_stays_dynamic() {
    let source = "import importlib\n\
        importlib.import_module(name)\n\
        importlib.import_module(f'pkg.{name}')\n\
        importlib.import_module('a' 'b')\n\
        importlib.import_module('a.' + name)\n\
        importlib.import_module(b'a')\n\
        importlib.import_module('a\\tb')\n\
        importlib.import_module('.x')\n\
        importlib.import_module('.x', package=name)\n\
        importlib.import_module('.x', package=__package__)\n\
        importlib.import_module('..x', package='pkg')\n\
        importlib.import_module('a b')\n\
        importlib.import_module()\n\
        importlib.import_module(name='pkg.mod')\n";
    let repo = repo(&[
        ("pkg/__init__.py", ""),
        ("pkg/mod.py", ""),
        ("pkg/x.py", ""),
        ("main.py", source),
    ]);
    let imports = repo.imports("main.py");
    assert_eq!(imports.resolved, Vec::new());
    assert_eq!(imports.unresolved, Vec::new());
    let got: Vec<(u32, &str)> = imports
        .skipped
        .iter()
        .inspect(|skip| assert_eq!(skip.reason, SkipReason::DynamicSpecifier))
        .map(|skip| (skip.at.line, skip.specifier.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (2, "name"),
            (3, "f'pkg.{name}'"),
            (4, "'a' 'b'"),
            (5, "'a.' + name"),
            (6, "b'a'"),
            (7, "'a\\tb'"),
            (8, ".x"),
            (9, ".x"),
            (10, ".x"),
            (11, "..x"),
            (12, "a b"),
            (14, "name='pkg.mod'"),
        ]
    );
}
