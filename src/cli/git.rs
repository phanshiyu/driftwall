//! Fetches the contract as it was at a git ref.
//!
//! The only code that runs another program. Arguments are passed one by one, never through a
//! shell, and a ref that could be read as an option is refused.

use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};

/// The text of `contract` (relative to `root`) at `git_ref`, or `None` when that ref has no such
/// file.
///
/// `root` may be any directory inside a work tree: the path is resolved relative to it, as
/// `./<contract>`.
///
/// # Errors
/// Git cannot be run, `root` is not in a repository, the ref does not exist, or the file cannot
/// be read for any reason other than not existing at that ref. The message is git's own.
pub fn contract_at(root: &Path, git_ref: &str, contract: &Path) -> Result<Option<String>> {
    let spec = object_spec(git_ref, contract)?;
    let shown = git(root, &["show", &spec])?;
    if shown.status.success() {
        let text = String::from_utf8(shown.stdout)
            .map_err(|_| anyhow!("{spec} is not valid UTF-8 text"))?;
        return Ok(Some(text));
    }
    // why: git's wording for a missing path varies by version and locale, so ask the tree instead.
    let listing = git(
        root,
        &[
            "ls-tree",
            "--name-only",
            git_ref,
            "--",
            &relative(contract)?,
        ],
    )?;
    if listing.status.success() && listing.stdout.is_empty() {
        return Ok(None);
    }
    bail!("git show {spec}: {}", message(&shown));
}

/// `REF:./<contract>`, the object `git show` reads, relative to the directory git runs in.
fn object_spec(git_ref: &str, contract: &Path) -> Result<String> {
    if git_ref.is_empty() || git_ref.starts_with('-') {
        bail!("`{git_ref}` is not a usable git ref: it is empty or starts with `-`");
    }
    Ok(format!("{git_ref}:{}", relative(contract)?))
}

/// `./<contract>` with `/` separators.
fn relative(contract: &Path) -> Result<String> {
    if contract.is_absolute() {
        bail!(
            "contract path {} must be relative to PATH",
            contract.display()
        );
    }
    let parts: Option<Vec<&str>> = contract
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect();
    let parts =
        parts.ok_or_else(|| anyhow!("contract path {} is not UTF-8", contract.display()))?;
    Ok(format!("./{}", parts.join("/")))
}

fn git(root: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .context("cannot run git")
}

/// What git said on stderr, trimmed, or its exit status when it said nothing.
fn message(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let text = text.trim();
    if text.is_empty() {
        output.status.to_string()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_names_the_ref_and_a_path_relative_to_the_directory() {
        assert_eq!(
            object_spec("main", Path::new("architecture.mmd")).unwrap(),
            "main:./architecture.mmd"
        );
        assert_eq!(
            object_spec("HEAD~1", Path::new("docs/arch.mmd")).unwrap(),
            "HEAD~1:./docs/arch.mmd"
        );
    }

    #[test]
    fn refs_that_look_like_options_are_refused() {
        for bad in ["", "-", "--output=x", "-p"] {
            assert!(object_spec(bad, Path::new("a.mmd")).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn absolute_contract_paths_are_refused() {
        let error = object_spec("main", Path::new("/etc/a.mmd")).unwrap_err();
        assert!(error.to_string().contains("relative"), "{error}");
    }
}
