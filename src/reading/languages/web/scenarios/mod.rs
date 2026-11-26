//! Tests of web that run the whole pipeline through `crate::reading::imports`, which the test edge
//! `languages -->|test| imports` allows.

use crate::code::Language;
use crate::reading::imports::scenarios::fixtures;
use crate::reading::imports::*;

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
