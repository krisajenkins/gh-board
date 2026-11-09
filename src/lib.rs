//! gh-board: a GitHub Projects board in the terminal.
//!
//! Functional core, imperative shell. Everything in the library except
//! [`shell`] is pure: data in, data out, no I/O. `shell` talks to GitHub; the
//! binary draws the terminal.

pub mod decode;
pub mod model;
pub mod queries;
pub mod search;
pub mod shell;
pub mod types;
pub mod update;
