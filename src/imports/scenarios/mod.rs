//! Tests that run the whole pipeline: the dispatch to every language, the discovery of what each
//! reads from the repository, and the following of re-exports.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::imports::*;
use crate::model::Language;

pub(super) mod fixtures;

mod asset;
mod excluded;
mod file_reference;
mod imports;
mod kind;
mod probe;
mod query;
mod reexport;
mod reexport_facade;
mod reexport_web;
mod references;
mod skipped;
mod test_file;
