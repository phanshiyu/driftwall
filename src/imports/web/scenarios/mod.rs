//! Tests of web that run the whole pipeline through `crate::imports`, which the test edge
//! `languages -->|test| imports` allows.

use crate::code::Language;
use crate::imports::scenarios::fixtures;
use crate::imports::*;

mod alias;
mod alias_resolve;
mod ecmascript_symbols;
mod html;
mod jsdoc;
mod markup;
mod package;
mod tsconfig;
mod vite;
mod vite_alias;
mod workspace;
