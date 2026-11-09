//! The terminal UI's pure parts: UI state, key handling, layout and drawing.
//!
//! The shell's app loop owns the terminal; everything here is data in, data
//! out. [`keys::handle_key`] turns a key into UI changes, reducer messages and
//! [`keys::Action`]s for the shell; [`render::render`] draws `(&State,
//! &UiState)` onto a frame.

pub mod card;
pub mod detail;
pub mod editor;
pub mod keys;
pub mod layout;
pub mod picker;
pub mod render;
pub mod state;
pub mod text;
pub mod theme;

pub use keys::{Action, Outcome, handle_key};
pub use render::render;
pub use state::{Mode, UiState};
