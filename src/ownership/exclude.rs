//! Which source files are not the project's own: vendored and minified code.

use std::ffi::OsStr;
use std::path::{Component, Path};

use crate::model::Exclusion;

/// Directory names whose whole subtree is vendored code.
const VENDOR_DIRECTORIES: [&str; 2] = ["vendor", "third_party"];
/// The dot-separated name part that marks a minified file (`app.min.js`).
const MINIFIED_PART: &str = "min";

/// The reason `file` is excluded from the scan, or `None` when it is the project's own source.
///
/// A file below a directory named `vendor` or `third_party` is [`Exclusion::Vendored`]. A file
/// whose second to last dot-separated name part is exactly `min` (`htmx.min.js`, `site.min.css`)
/// is [`Exclusion::Minified`]. Vendored wins when both apply. The path is repository-relative.
#[must_use]
pub fn exclusion_of(file: &Path) -> Option<Exclusion> {
    let in_vendor_directory = file
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .any(|part| match part {
            Component::Normal(name) => VENDOR_DIRECTORIES.iter().any(|vendor| name == *vendor),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => false,
        });
    if in_vendor_directory {
        return Some(Exclusion::Vendored);
    }
    let name = file.file_name().and_then(OsStr::to_str)?;
    let mut parts = name.rsplit('.');
    let _extension = parts.next()?;
    let marker = parts.next()?;
    // why: `.min.js` alone is a hidden file called `.min`, not a minified `<name>.min.js`.
    (marker == MINIFIED_PART && parts.next().is_some_and(|stem| !stem.is_empty()))
        .then_some(Exclusion::Minified)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(path: &str) -> Option<Exclusion> {
        exclusion_of(Path::new(path))
    }

    #[test]
    fn minified_names_have_min_as_the_part_before_the_extension() {
        for name in [
            "htmx.min.js",
            "static/htmx.min.js",
            "a.min.mjs",
            "a.b.min.js",
            "site.min.css",
        ] {
            assert_eq!(reason(name), Some(Exclusion::Minified), "{name}");
        }
    }

    #[test]
    fn near_misses_are_source() {
        for name in [
            "min.js",
            "admin.js",
            "a.minx.js",
            "a.mini.js",
            "a.min",
            ".min.js",
            "minimal/a.js",
            "a.js",
        ] {
            assert_eq!(reason(name), None, "{name}");
        }
    }

    #[test]
    fn anything_below_a_vendor_directory_is_vendored() {
        for path in [
            "vendor/a.js",
            "ui/vendor/basecoat/basecoat.min.js",
            "a/third_party/b/c/d.rs",
            "vendor/x/y.py",
        ] {
            assert_eq!(reason(path), Some(Exclusion::Vendored), "{path}");
        }
    }

    #[test]
    fn only_whole_directory_names_count() {
        for path in [
            "vendors/a.js",
            "my_vendor/a.js",
            "third-party/a.js",
            "vendor.js",
        ] {
            assert_eq!(reason(path), None, "{path}");
        }
    }

    #[test]
    fn the_exclusion_reasons_have_stable_words() {
        assert_eq!(Exclusion::Vendored.as_str(), "vendored");
        assert_eq!(Exclusion::Minified.as_str(), "minified");
    }
}
