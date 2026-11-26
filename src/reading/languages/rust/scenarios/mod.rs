//! Tests of rust that run the whole pipeline through `crate::reading::imports`, which the test edge
//! `languages -->|test| imports` allows.

use crate::code::Language;
use crate::reading::imports::scenarios::fixtures;
use crate::reading::imports::*;

mod rust;
mod rust_dogfood;
mod rust_path;
mod rust_symbols;
mod rust_test_modules;
