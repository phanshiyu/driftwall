//! Tests of python that run the whole pipeline through `crate::imports`, which the test edge
//! `languages -->|test| imports` allows.

use crate::imports::scenarios::fixtures;
use crate::imports::*;
use crate::model::Language;

mod python;
mod python_roots;
mod python_symbols;
