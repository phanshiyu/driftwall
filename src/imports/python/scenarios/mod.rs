//! Tests of python that run the whole pipeline through `crate::imports`, which the test edge
//! `languages -->|test| imports` allows.

use crate::code::Language;
use crate::imports::scenarios::fixtures;
use crate::imports::*;

mod python;
mod python_roots;
mod python_symbols;
