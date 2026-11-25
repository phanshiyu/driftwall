//! Discovery of the Python import roots a project configures, read from `pyproject.toml`,
//! `setup.cfg`, `pytest.ini` and `pyrightconfig.json`.
//!
//! The IO shell next to [`super::layout`]: it finds the configuration directories, reads
//! the files and hands back the roots each directory configures. See "Python resolution" in
//! `docs/DESIGN.md` for which settings are read. A file that cannot be read or parsed is skipped
//! silently, like a malformed `tsconfig.json`: the other files still count.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jsonc_parser::ParseOptions;
use serde_json::Value as Json;

use crate::imports::syntax::paths::directories;
use crate::imports::syntax::paths::join_normalised;
use crate::model::Language;

const PYPROJECT: &str = "pyproject.toml";
const SETUP_CFG: &str = "setup.cfg";
const PYTEST_INI: &str = "pytest.ini";
const PYRIGHT_CONFIG: &str = "pyrightconfig.json";

/// Maps each configuration directory to the import roots its files configure.
///
/// Reads from under `root`. A configuration directory is the repository root and each directory with a
/// `pyproject.toml`, among the directories at or above a Python file in `files`. The roots are
/// repository-relative, in the order they apply, and a directory that configures none is absent.
#[must_use]
pub fn python_roots(
    root: &Path,
    files: &BTreeMap<PathBuf, Language>,
) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let sources = files
        .iter()
        .filter(|(_, language)| matches!(language, Language::Python))
        .map(|(path, _)| path.as_path());
    directories(sources)
        .into_iter()
        .filter(|dir| dir.as_os_str().is_empty() || root.join(dir).join(PYPROJECT).is_file())
        .filter_map(|dir| {
            let read = |name: &str| fs::read_to_string(root.join(&dir).join(name)).ok();
            let roots = configured(&dir, &read);
            (!roots.is_empty()).then_some((dir, roots))
        })
        .collect()
}

/// The roots configured in `dir`, reading its files through `read`, without repeats.
fn configured(dir: &Path, read: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let from = |name: &str, parse: fn(&str) -> Vec<String>| {
        read(name).map(|text| parse(&text)).unwrap_or_default()
    };
    let entries = from(PYPROJECT, pyproject)
        .into_iter()
        .chain(from(SETUP_CFG, setup_cfg))
        .chain(from(PYTEST_INI, pytest_ini))
        .chain(from(PYRIGHT_CONFIG, pyright_config));
    let mut roots = Vec::new();
    for entry in entries {
        if let Some(root) = join_normalised(Some(dir), &entry)
            && !roots.contains(&root)
        {
            roots.push(root);
        }
    }
    roots
}

/// The root entries of a `pyproject.toml`, relative to its directory. None when it is not TOML.
fn pyproject(text: &str) -> Vec<String> {
    let Ok(table) = toml::from_str::<toml::Value>(text) else {
        return Vec::new();
    };
    let at = |path: &[&str]| path.iter().try_fold(&table, |value, key| value.get(key));
    let mut entries = Vec::new();
    entries.extend(toml_strings(at(&[
        "tool",
        "pytest",
        "ini_options",
        "pythonpath",
    ])));
    entries.extend(toml_strings(at(&["tool", "pyright", "extraPaths"])));
    entries.extend(toml_strings(at(&["tool", "basedpyright", "extraPaths"])));
    entries.extend(toml_strings(
        at(&["tool", "setuptools", "package-dir"]).and_then(|dirs| dirs.get("")),
    ));
    entries.extend(toml_strings(at(&[
        "tool",
        "setuptools",
        "packages",
        "find",
        "where",
    ])));
    let wheel = at(&["tool", "hatch", "build", "targets", "wheel", "packages"]);
    entries.extend(toml_strings(wheel).iter().map(|package| parent_of(package)));
    let poetry = at(&["tool", "poetry", "packages"]).and_then(toml::Value::as_array);
    for package in poetry.into_iter().flatten() {
        if package
            .get("include")
            .and_then(toml::Value::as_str)
            .is_some()
        {
            let from = package.get("from").and_then(toml::Value::as_str);
            entries.push(from.unwrap_or(".").to_owned());
        }
    }
    entries
}

/// The strings of a TOML value: the elements of an array of strings (other elements are dropped),
/// or a lone string.
fn toml_strings(value: Option<&toml::Value>) -> Vec<String> {
    match value {
        Some(toml::Value::String(text)) => vec![text.clone()],
        Some(toml::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// The directory a package path sits in: `src/x` is `src`, `x` is `.`.
fn parent_of(package: &str) -> String {
    let trimmed = package.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((parent, _)) => parent.to_owned(),
        None => ".".to_owned(),
    }
}

/// The root entries of a `setup.cfg`.
fn setup_cfg(text: &str) -> Vec<String> {
    let ini = Ini::parse(text);
    let mut entries: Vec<String> = ini
        .get("options", "package_dir")
        .into_iter()
        .flat_map(str::lines)
        .filter_map(|line| line.trim().strip_prefix('='))
        .map(|target| target.trim().to_owned())
        .collect();
    entries.extend(list(ini.get("options.packages.find", "where")));
    entries.extend(list(ini.get("tool:pytest", "pythonpath")));
    entries
}

/// The root entries of a `pytest.ini`.
fn pytest_ini(text: &str) -> Vec<String> {
    list(Ini::parse(text).get("pytest", "pythonpath"))
}

/// The root entries of a `pyrightconfig.json`. None when it is not JSON with comments.
fn pyright_config(text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value = jsonc_parser::parse_to_serde_value::<Json>(text, &ParseOptions::default());
    let paths = value
        .ok()
        .and_then(|value| value.get("extraPaths").cloned());
    paths
        .as_ref()
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|path| path.as_str().map(str::to_owned))
        .collect()
}

/// The items of the value of an INI list, separated by commas or whitespace.
fn list(value: Option<&str>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|value| value.split(|c: char| c == ',' || c.is_whitespace()))
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

/// An INI file read as `configparser` does, as far as the settings above need: `[section]`
/// headers, `key = value` or `key: value`, indented lines continuing the value above, `#` and `;`
/// comment lines.
struct Ini {
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl Ini {
    fn parse(text: &str) -> Self {
        let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut section = String::new();
        let mut key: Option<String> = None;
        for line in text.strip_prefix('\u{feff}').unwrap_or(text).lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            let indented = line.starts_with(char::is_whitespace);
            if let (true, Some(key)) = (indented, key.as_ref()) {
                if let Some(value) = sections
                    .get_mut(&section)
                    .and_then(|entries| entries.get_mut(key))
                {
                    value.push('\n');
                    value.push_str(trimmed);
                }
            } else if let Some(name) = trimmed
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                name.trim().clone_into(&mut section);
                sections.entry(section.clone()).or_default();
                key = None;
            } else if let Some((name, value)) = trimmed.split_once(['=', ':']) {
                let name = name.trim().to_lowercase();
                sections
                    .entry(section.clone())
                    .or_default()
                    .insert(name.clone(), value.trim().to_owned());
                key = Some(name);
            }
        }
        Self { sections }
    }

    fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(section)
            .and_then(|entries| entries.get(key))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyproject_settings_are_read_in_the_documented_order() {
        let text = r#"
[tool.poetry]
packages = [{ include = "p", from = "poetry_lib" }, { include = "q" }, { from = "no_include" }]
[tool.hatch.build.targets.wheel]
packages = ["src/x", "y", "a/b/"]
[tool.setuptools.packages.find]
where = ["find_lib"]
[tool.setuptools.package-dir]
"" = "dir_lib"
x = "other"
[tool.basedpyright]
extraPaths = ["based"]
[tool.pyright]
extraPaths = ["right"]
[tool.pytest.ini_options]
pythonpath = ["scripts", "."]
"#;
        assert_eq!(
            pyproject(text),
            [
                "scripts",
                ".",
                "right",
                "based",
                "dir_lib",
                "find_lib",
                "src",
                ".",
                "a",
                "poetry_lib",
                "."
            ]
        );
    }

    #[test]
    fn pyproject_settings_of_the_wrong_type_give_nothing() {
        let text = r#"
[tool.pytest.ini_options]
pythonpath = 3
[tool.pyright]
extraPaths = [1, "ok", { a = 1 }]
[tool.setuptools]
package-dir = "x"
[tool.poetry]
packages = "x"
"#;
        assert_eq!(pyproject(text), ["ok"]);
        assert_eq!(pyproject("not = [toml"), Vec::<String>::new());
    }

    #[test]
    fn a_lone_string_is_a_list_of_one() {
        let text = "[tool.pytest.ini_options]\npythonpath = \"src\"\n";
        assert_eq!(pyproject(text), ["src"]);
    }

    #[test]
    fn setup_cfg_package_dir_and_find_and_pytest_are_read() {
        let text = "[metadata]\nname = x\n\n[options]\npackages = find:\npackage_dir =\n    =lib\n    other = elsewhere\n\n[options.packages.find]\nwhere = src, extra\n\n[tool:pytest]\npythonpath = scripts\n    .\n";
        assert_eq!(setup_cfg(text), ["lib", "src", "extra", "scripts", "."]);
    }

    #[test]
    fn setup_cfg_package_dir_on_one_line_is_read() {
        assert_eq!(setup_cfg("[options]\npackage_dir = = src\n"), ["src"]);
        assert_eq!(
            setup_cfg("[options]\n# comment\npackage_dir: =lib\n"),
            ["lib"]
        );
    }

    #[test]
    fn pytest_ini_reads_only_the_pytest_section() {
        let text = "[tool:pytest]\npythonpath = no\n[pytest]\n; note\npythonpath = a b\n";
        assert_eq!(pytest_ini(text), ["a", "b"]);
    }

    #[test]
    fn pyright_config_is_jsonc_with_extra_paths() {
        let text = "{\n // c\n \"extraPaths\": [\"scripts\", 3, \"lib\",],\n}";
        assert_eq!(pyright_config(text), ["scripts", "lib"]);
        assert_eq!(pyright_config("{ nope"), Vec::<String>::new());
        assert_eq!(
            pyright_config("{\"extraPaths\": \"x\"}"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn roots_are_normalised_against_their_directory_and_the_bad_ones_dropped() {
        let read = |name: &str| {
            (name == PYPROJECT).then(|| {
                "[tool.pytest.ini_options]\npythonpath = [\".\", \"./lib\", \"../shared\", \"/abs\", \"../../out\", \"lib\"]\n".to_owned()
            })
        };
        let roots = |dir: &str| -> Vec<String> {
            configured(Path::new(dir), &read)
                .iter()
                .map(|root| root.display().to_string())
                .collect()
        };
        assert_eq!(
            roots("svc/api"),
            ["svc/api", "svc/api/lib", "svc/shared", "out"]
        );
        assert_eq!(roots("svc"), ["svc", "svc/lib", "shared"]);
    }
}
