//! Tests of python that run the whole pipeline through `crate::reading::imports`, which the test edge
//! `languages -->|test| imports` allows.

use crate::code::Language;
use crate::reading::imports::scenarios::fixtures;
use crate::reading::imports::*;

mod python;
mod python_roots;
mod python_symbols;
