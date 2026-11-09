//! gh-board: a GitHub Projects board in the terminal.
//!
//! Functional core, imperative shell. Everything in this crate's library is
//! pure: data in, data out, no I/O. The binary is the shell that runs effects
//! and draws the terminal.

pub mod model;
pub mod search;
pub mod types;
pub mod update;
