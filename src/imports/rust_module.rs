//! The Rust module-path model: where a file sits in its crate, and how a written path maps to the
//! repository file that holds the item it names. Pure data, no syntax tree involved.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use super::Environment;
use crate::model::Symbol;

/// Files that mark a directory as a crate root.
const ROOT_FILES: [&str; 2] = ["lib.rs", "main.rs"];

/// One segment of a path as written in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment<'s> {
    /// The keyword `crate`.
    Crate,
    /// The keyword `self`.
    SelfModule,
    /// The keyword `super`.
    Super,
    /// A module, item or crate name.
    Name(&'s str),
}

/// A path as written, with generic arguments already dropped, such as `crate::model::Edge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenPath<'s> {
    segments: Vec<Segment<'s>>,
}

/// A module's name chain from its crate root. The crate root itself is the empty path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModulePath {
    names: Vec<String>,
}

/// The directory and file that root one crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateRoot {
    dir: PathBuf,
    file: PathBuf,
}

/// Where a path is written: the crate it belongs to and the module that is current there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    root: CrateRoot,
    module: ModulePath,
}

impl<'s> WrittenPath<'s> {
    /// A path made of `segments`.
    pub const fn new(segments: Vec<Segment<'s>>) -> Self {
        Self { segments }
    }

    /// The name the path ends in, or `None` when it ends in `crate`, `self` or `super` or is empty.
    pub fn last_name(&self) -> Option<&'s str> {
        match self.segments.last()? {
            Segment::Name(name) => Some(name),
            Segment::Crate | Segment::SelfModule | Segment::Super => None,
        }
    }

    /// The name the path starts with, or `None` when it starts with `crate`, `self` or `super`.
    pub fn first_name(&self) -> Option<&'s str> {
        match self.segments.first()? {
            Segment::Name(name) => Some(name),
            Segment::Crate | Segment::SelfModule | Segment::Super => None,
        }
    }

    /// This path with its first segment replaced by all of `prefix`: `gl::Client` with `gl` bound
    /// to `crate::gitlab` is `crate::gitlab::Client`.
    pub fn with_first_replaced(&self, prefix: &Self) -> Self {
        let tail = self.segments.iter().skip(1).copied();
        Self {
            segments: prefix.segments.iter().copied().chain(tail).collect(),
        }
    }

    /// The repository file this path points into, written at `site`, and the symbol it uses from
    /// that file. `None` means the path is not a repository reference: it starts with a third
    /// party or standard library name, climbs above the crate root, or uses `crate`, `self` or
    /// `super` outside any known crate.
    ///
    /// The file is the longest prefix of the path that names a module file. Trailing segments are
    /// items inside that module, and the first of them is the symbol. A path that ends at the
    /// module file itself uses [`Symbol::All`]. When no module file matches, the crate root file
    /// is the target and the first segment after `crate` is the symbol.
    pub fn target(&self, site: Option<&Site>, env: &Environment<'_>) -> Option<(PathBuf, Symbol)> {
        let (first, tail) = self.segments.split_first()?;
        let (root, mut module, mut rest) = match first {
            Segment::Crate => (site?.root.clone(), ModulePath::default(), tail),
            Segment::SelfModule | Segment::Super => {
                let site = site?;
                (
                    site.root.clone(),
                    site.module.clone(),
                    self.segments.as_slice(),
                )
            }
            Segment::Name(name) => {
                let dir = env.crates.get(*name)?;
                (CrateRoot::library(dir), ModulePath::default(), tail)
            }
        };
        while let Some((segment, after)) = rest.split_first() {
            match segment {
                Segment::SelfModule => {}
                Segment::Super => module = module.parent()?,
                Segment::Crate | Segment::Name(_) => break,
            }
            rest = after;
        }
        let items = rest.iter().map_while(|segment| match segment {
            Segment::Name(name) => Some(*name),
            Segment::Crate | Segment::SelfModule | Segment::Super => None,
        });
        let written: Vec<&str> = module
            .names
            .iter()
            .map(String::as_str)
            .chain(items)
            .collect();
        let (file, matched) = root.deepest_file(&written, env.known);
        let symbol = written
            .get(matched)
            .map_or(Symbol::All, |name| Symbol::Name((*name).to_owned()));
        Some((file, symbol))
    }
}

impl ModulePath {
    fn parent(&self) -> Option<Self> {
        let (_, parents) = self.names.split_last()?;
        Some(Self {
            names: parents.to_vec(),
        })
    }

    /// The module a file declares, given its path relative to its crate root directory:
    /// `a/b.rs` and `a/b/mod.rs` are both `a::b`, and the root file is the empty path.
    fn of_file(relative: &Path) -> Option<Self> {
        let mut names = Vec::new();
        for component in relative.with_extension("").components() {
            let Component::Normal(part) = component else {
                return None;
            };
            names.push(part.to_str()?.to_owned());
        }
        if names.last().is_some_and(|name| name == "mod") {
            names.pop();
        }
        Some(Self { names })
    }
}

impl CrateRoot {
    /// The root of a library crate whose `lib.rs` is in `dir`.
    fn library(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            file: dir.join("lib.rs"),
        }
    }

    /// The root of the crate `file` belongs to: the deepest ancestor directory holding a `lib.rs`
    /// or `main.rs`. A file that is itself a root file is its own root. In a directory holding
    /// both, any other file belongs to the library.
    fn of_file(file: &Path, known: &BTreeSet<PathBuf>) -> Option<Self> {
        let dir = file.ancestors().skip(1).find(|dir| {
            ROOT_FILES
                .iter()
                .any(|root| known.contains(&dir.join(root)))
        })?;
        let own = file
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| ROOT_FILES.contains(name));
        let name = own.or_else(|| {
            ROOT_FILES
                .iter()
                .find(|root| known.contains(&dir.join(root)))
                .copied()
        })?;
        Some(Self {
            dir: dir.to_path_buf(),
            file: dir.join(name),
        })
    }

    /// The file of the longest prefix of `written` that is a module file, with the length of that
    /// prefix. Else the root file, with length 0.
    fn deepest_file(&self, written: &[&str], known: &BTreeSet<PathBuf>) -> (PathBuf, usize) {
        (1..=written.len())
            .rev()
            .find_map(|len| {
                let base = written
                    .iter()
                    .take(len)
                    .fold(self.dir.clone(), |dir, name| dir.join(name));
                [base.with_extension("rs"), base.join("mod.rs")]
                    .into_iter()
                    .find(|candidate| known.contains(candidate))
                    .map(|file| (file, len))
            })
            .unwrap_or_else(|| (self.file.clone(), 0))
    }
}

impl Site {
    /// The site of a path written in `file`, inside the inline `mod name { }` blocks `inline`
    /// listed outermost first. `None` when `file` belongs to no crate.
    pub fn of(file: &Path, inline: &[&str], known: &BTreeSet<PathBuf>) -> Option<Self> {
        let root = CrateRoot::of_file(file, known)?;
        let is_root_file = file == root.file;
        let mut module = if is_root_file {
            ModulePath::default()
        } else {
            ModulePath::of_file(file.strip_prefix(&root.dir).ok()?)?
        };
        module
            .names
            .extend(inline.iter().map(|name| (*name).to_owned()));
        Some(Self { root, module })
    }
}
