//! Fetches the contract as it was at a git ref.
//!
//! The only code that runs another program. Arguments are passed one by one, never through a
//! shell, and a ref that could be read as an option is refused.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

/// Writes the tree of `git_ref` (the directory `root`, as git sees it) into the empty directory
/// `dest`.
///
/// # Errors
/// Git or `tar` cannot be run, or the ref does not exist.
pub fn export_tree(root: &Path, git_ref: &str, dest: &Path) -> Result<()> {
    object_spec(git_ref, Path::new("."))?;
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["archive", "--format=tar", git_ref])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot run git")?;
    let tar = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dest)
        .stdin(archive.stdout.take().context("git archive has no output")?)
        .output()
        .context("cannot run tar")?;
    let archived = archive.wait_with_output().context("cannot run git")?;
    if !archived.status.success() {
        bail!("git archive {git_ref}: {}", message(&archived));
    }
    if !tar.status.success() {
        bail!("tar: {}", message(&tar));
    }
    Ok(())
}

/// The `(old path, new path)` pairs git detects as renames between `git_ref` and the working tree
/// of `root`, untracked files included and ignored ones not, paths relative to `root`.
///
/// `scratch` is an empty directory for a throwaway index: the working tree is added to that index,
/// never the real one.
///
/// # Errors
/// Git cannot be run or fails.
pub fn renames(root: &Path, git_ref: &str, scratch: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    object_spec(git_ref, Path::new("."))?;
    let index = scratch.join("index");
    // why: `git add` writes a blob per file; they go to the scratch dir so the repository is
    // why: untouched, and the real objects are still readable as an alternate.
    let objects = scratch.join("objects");
    std::fs::create_dir_all(&objects).context("cannot create a temporary object directory")?;
    let real = git(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "objects",
        ],
    )?;
    if !real.status.success() {
        bail!("git rev-parse --git-path objects: {}", message(&real));
    }
    let real = String::from_utf8_lossy(&real.stdout).trim().to_owned();
    let run = |args: &[&str]| -> Result<Output> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .env("GIT_INDEX_FILE", &index)
            .env("GIT_OBJECT_DIRECTORY", &objects)
            .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &real)
            .args(args)
            .output()
            .context("cannot run git")?;
        if output.status.success() {
            Ok(output)
        } else {
            bail!("git {}: {}", args.join(" "), message(&output))
        }
    };
    run(&["add", "-A", "--", "."])?;
    let listing = run(&[
        "diff",
        "--cached",
        "--name-status",
        "-z",
        "-M",
        "--relative",
        git_ref,
        "--",
        ".",
    ])?;
    Ok(parse_renames(&String::from_utf8_lossy(&listing.stdout)))
}

/// The rename entries of `git diff --name-status -z`: `R<score>`, old path, new path. Other
/// statuses carry one path (a copy carries two and is skipped like the rest).
fn parse_renames(listing: &str) -> Vec<(PathBuf, PathBuf)> {
    let mut fields = listing.split('\0').filter(|field| !field.is_empty());
    let mut found = Vec::new();
    while let Some(status) = fields.next() {
        let paths = if status.starts_with(['R', 'C']) { 2 } else { 1 };
        let mut names = fields.by_ref().take(paths);
        if let (true, Some(old), Some(new)) = (status.starts_with('R'), names.next(), names.next())
        {
            found.push((PathBuf::from(old), PathBuf::from(new)));
        }
    }
    found
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
    fn rename_entries_are_paired_and_other_statuses_skipped() {
        let listing = "M\0a.rs\0R100\0b/old.rs\0c/new.rs\0A\0d.rs\0C75\0e.rs\0f.rs\0D\0g.rs\0";
        assert_eq!(
            parse_renames(listing),
            vec![(PathBuf::from("b/old.rs"), PathBuf::from("c/new.rs"))]
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
