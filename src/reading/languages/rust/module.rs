//! The Rust module-path model: where a file sits in its crate, and how a written path maps to the
//! repository file that holds the item it names. Pure data, no syntax tree involved.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use super::env::Env;
use super::path_modules::PathModules;
use crate::code::Symbol;
use crate::reading::syntax::resolution::Files;

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
    /// A leading `::`, which makes the name after it an extern crate and never a module of the
    /// current one.
    Extern,
    /// A module, item or crate name.
    Name(&'s str),
}

/// A path as written, with generic arguments already dropped, such as `crate::contract::Edge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenPath<'s> {
    segments: Vec<Segment<'s>>,
    /// Whether the path may name a module as a whole: true for a `use` leaf and a `mod`
    /// declaration, false for a path in an expression, a type or a macro argument.
    names_module: bool,
    /// Whether the path is the prefix of a glob, `use a::b::*`.
    glob: bool,
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
    /// A path made of `segments` written in an expression, a type or a macro argument, where it
    /// cannot name a module as a whole.
    pub const fn in_code(segments: Vec<Segment<'s>>) -> Self {
        Self {
            segments,
            names_module: false,
            glob: false,
        }
    }

    /// A path made of `segments` written in a `use` or `mod` declaration, where it may name a
    /// module as a whole.
    pub const fn declared(segments: Vec<Segment<'s>>) -> Self {
        Self {
            segments,
            names_module: true,
            glob: false,
        }
    }

    /// This path as the prefix of a glob: `use a::b::*` names every item of `a::b`, not the module.
    pub fn into_glob(self) -> Self {
        Self { glob: true, ..self }
    }

    /// Whether the path is the prefix of a glob, `use a::b::*`.
    #[must_use]
    pub const fn is_glob(&self) -> bool {
        self.glob
    }

    /// Whether the path, when it ends at a module file, only binds that module's name: written in
    /// a `use` or `mod` declaration and not a glob. Such a path uses no item of the module.
    pub const fn binds_module(&self) -> bool {
        self.names_module && !self.glob
    }

    /// Whether the path is exactly `use super::*`: the glob of the parent module.
    #[must_use]
    pub fn is_super_glob(&self) -> bool {
        self.glob && self.segments == [Segment::Super]
    }

    /// The name `n` when the path is exactly `super::n` and not a glob.
    #[must_use]
    pub const fn super_item(&self) -> Option<&'s str> {
        match (self.segments.as_slice(), self.glob) {
            ([Segment::Super, Segment::Name(name)], false) => Some(name),
            _ => None,
        }
    }

    /// This path, written relative to a module, as it reads from an inline module directly
    /// inside it: `self::a` becomes `super::a`, `super::a` becomes `super::super::a`, and a path
    /// that starts with `crate` or a name is unchanged.
    #[must_use]
    pub fn seen_from_child(&self) -> Self {
        let mut segments = self.segments.clone();
        // why: `self::a` read from the child is `super::a`, and `super::a` is one level further up.
        match segments.first() {
            Some(Segment::SelfModule) => {
                segments.remove(0);
                segments.insert(0, Segment::Super);
            }
            Some(Segment::Super) => segments.insert(0, Segment::Super),
            _ => {}
        }
        Self {
            segments,
            ..self.clone()
        }
    }

    /// The name the path ends in, or `None` when it ends in `crate`, `self` or `super` or is empty.
    pub fn last_name(&self) -> Option<&'s str> {
        match self.segments.last()? {
            Segment::Name(name) => Some(name),
            Segment::Extern | Segment::Crate | Segment::SelfModule | Segment::Super => None,
        }
    }

    /// The name of the crate the path starts with: its first name, also after a leading `::`.
    pub fn crate_name(&self) -> Option<&'s str> {
        match self.segments.as_slice() {
            [Segment::Extern, Segment::Name(name), ..] => Some(name),
            _ => self.first_name(),
        }
    }

    /// The name the path starts with, or `None` when it starts with `crate`, `self` or `super`.
    pub fn first_name(&self) -> Option<&'s str> {
        match self.segments.first()? {
            Segment::Name(name) => Some(name),
            Segment::Extern | Segment::Crate | Segment::SelfModule | Segment::Super => None,
        }
    }

    /// The name `n` when the path starts `self::n` and goes on, as `self::z::A` does.
    pub const fn self_name(&self) -> Option<&'s str> {
        match self.segments.as_slice() {
            [Segment::SelfModule, Segment::Name(name), _, ..] => Some(name),
            _ => None,
        }
    }

    /// This path with its first segment replaced by all of `prefix`: `gl::Client` with `gl` bound
    /// to `crate::gitlab` is `crate::gitlab::Client`.
    pub fn with_first_replaced(&self, prefix: &Self) -> Self {
        self.with_leading_replaced(1, prefix)
    }

    /// This path with its first `count` segments replaced by all of `prefix`: `self::z::A` with
    /// `z` bound to `crate::a` is `crate::a::A`.
    pub fn with_leading_replaced(&self, count: usize, prefix: &Self) -> Self {
        let tail = self.segments.iter().skip(count).copied();
        Self {
            segments: prefix.segments.iter().copied().chain(tail).collect(),
            names_module: self.names_module,
            glob: self.glob,
        }
    }

    /// The repository file this path points into, written in `importer` at `site`, and the symbol it uses from
    /// that file. `None` means the path is not a repository reference: it starts with a third
    /// party or standard library name, climbs above the crate root, or uses `crate`, `self` or
    /// `super` outside any known crate.
    ///
    /// The file is the longest prefix of the path that names a module file. Trailing segments are
    /// items inside that module, and the first of them is the symbol. A path that ends at the
    /// module file itself uses [`Symbol::All`], except that a path in code (not a `use` or `mod`
    /// declaration) that ends at a module file names an item of the parent module instead, see
    /// [`WrittenPath::in_code`]. When no module file matches, the crate root file is the target and
    /// the first segment after `crate` is the symbol.
    pub fn target(
        &self,
        importer: &Path,
        site: Option<&Site>,
        files: &Files<'_>,
        env: &Env<'_>,
    ) -> Option<(PathBuf, Symbol)> {
        let (first, tail) = self.segments.split_first()?;
        // why: a leading `::` can only name an extern crate, so `::name` skips every module of the file.
        let (first, tail) = match first {
            Segment::Extern => match tail.split_first()? {
                (name @ Segment::Name(_), tail) => (name, tail),
                _ => return None,
            },
            _ => (first, tail),
        };
        let (root, mut module, mut rest) = match first {
            Segment::Extern => return None,
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
                let dir = env.crate_dir(name, importer)?;
                (CrateRoot::library(dir), ModulePath::default(), tail)
            }
        };
        while let Some((segment, after)) = rest.split_first() {
            match segment {
                Segment::SelfModule => {}
                Segment::Super => module = module.parent()?,
                Segment::Extern | Segment::Crate | Segment::Name(_) => break,
            }
            rest = after;
        }
        let items = rest.iter().map_while(|segment| match segment {
            Segment::Name(name) => Some(*name),
            Segment::Extern | Segment::Crate | Segment::SelfModule | Segment::Super => None,
        });
        let module_len = module.names.len();
        let written: Vec<&str> = module
            .names
            .iter()
            .map(String::as_str)
            .chain(items)
            .collect();
        // why: a module is no value, so `diagram::parse(..)` with a `parse.rs` names an item of `diagram`.
        if !self.names_module
            && written.len() > module_len
            && let Some((item, parents)) = written.split_last()
            && root.deepest_file(&written, files.known, env.paths).1 == written.len()
        {
            let (file, _) = root.deepest_file(parents, files.known, env.paths);
            return Some((file, Symbol::Name((*item).to_owned())));
        }
        let (file, matched) = root.deepest_file(&written, files.known, env.paths);
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
    fn deepest_file(
        &self,
        written: &[&str],
        known: &BTreeSet<PathBuf>,
        paths: &PathModules,
    ) -> (PathBuf, usize) {
        (1..=written.len())
            .rev()
            .find_map(|len| {
                let names = written.get(..len)?;
                // why: the longest `#[path]` module on the way decides where the rest is looked up.
                let placed = (1..=len).rev().find_map(|at| {
                    let file = paths.file_of(&self.file, names.get(..at)?)?;
                    Some((at, file))
                });
                let (base, rest) = match placed {
                    Some((at, file)) if at == len => return Some((file.clone(), len)),
                    // why: a file placed by `#[path]` owns its directory, as a mod-rs file does.
                    Some((at, file)) => (file.parent()?.to_path_buf(), names.get(at..)?),
                    None => (self.dir.clone(), names),
                };
                let base = rest.iter().fold(base, |dir, name| dir.join(name));
                [base.with_extension("rs"), base.join("mod.rs")]
                    .into_iter()
                    .find(|candidate| known.contains(candidate))
                    .map(|file| (file, len))
            })
            .unwrap_or_else(|| (self.file.clone(), 0))
    }
}

impl Site {
    /// The crate root file and module names of this site, the key `#[path]` modules are stored under.
    #[must_use]
    pub fn placement(&self) -> (PathBuf, Vec<String>) {
        (self.root.file.clone(), self.module.names.clone())
    }

    /// The file of the module that `mod name;`, written at this site, declares: `<name>.rs` or
    /// `<name>/mod.rs` in the directory of the current module. `None` when no known file is it.
    pub fn declared_file(
        &self,
        name: &str,
        known: &BTreeSet<PathBuf>,
        paths: &PathModules,
    ) -> Option<PathBuf> {
        let written: Vec<&str> = self
            .module
            .names
            .iter()
            .map(String::as_str)
            .chain([name])
            .collect();
        let (file, matched) = self.root.deepest_file(&written, known, paths);
        (matched == written.len()).then_some(file)
    }

    /// The site of a path written in `file`, inside the inline `mod name { }` blocks `inline`
    /// listed outermost first. `None` when `file` belongs to no crate.
    pub fn of(
        file: &Path,
        inline: &[&str],
        known: &BTreeSet<PathBuf>,
        paths: &PathModules,
    ) -> Option<Self> {
        // why: a file `#[path]` placed is the module its declaration names, not the one its location suggests.
        if let Some((root_file, names)) = paths.module_of(file) {
            let mut module = ModulePath {
                names: names.clone(),
            };
            module
                .names
                .extend(inline.iter().map(|name| (*name).to_owned()));
            let dir = root_file.parent()?.to_path_buf();
            let root = CrateRoot {
                dir,
                file: root_file.clone(),
            };
            return Some(Self { root, module });
        }
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
