//! Tests that run the whole pipeline: the dispatch to every language, the discovery of what each
//! reads from the repository, and the following of re-exports.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::imports::*;
use crate::model::Language;

mod fixtures;

mod alias;
mod alias_resolve;
mod asset;
mod ecmascript_symbols;
mod excluded;
mod file_reference;
mod html;
mod imports;
mod jsdoc;
mod kind;
mod markup;
mod package;
mod probe;
mod python;
mod python_roots;
mod python_symbols;
mod query;
mod reexport;
mod reexport_facade;
mod reexport_web;
mod references;
mod rust;
mod rust_dogfood;
mod rust_path;
mod rust_symbols;
mod rust_test_modules;
mod shell;
mod skipped;
mod swift;
mod test_file;
mod tsconfig;
mod vite;
mod vite_alias;
mod workspace;
