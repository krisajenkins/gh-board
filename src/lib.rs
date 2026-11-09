//! gh-board: a GitHub Projects board in the terminal.
//!
//! Functional core, imperative shell. Everything in the library except
//! [`shell`] is pure: data in, data out, no I/O. `shell` talks to GitHub and
//! runs the app loop, which owns the terminal; [`ui`] holds the UI's pure
//! parts (UI state, keys, layout, drawing).

pub mod cli;
pub mod decode;
pub mod model;
pub mod queries;
pub mod search;
pub mod shell;
pub mod types;
pub mod ui;
pub mod update;
