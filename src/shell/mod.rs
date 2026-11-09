//! The imperative shell: the only part of the library that does I/O.
//!
//! - [`gh`]: one GraphQL request -> `gh api graphql` -> the response's `data`.
//! - [`effects`]: the reducer's network effects -> requests -> decoded `Msg`s.
//! - [`app`]: the app loop: terminal, keys, dispatch, browser and clipboard.

pub mod app;
pub mod effects;
pub mod gh;
