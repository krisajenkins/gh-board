//! Colours. Named after the Lua plugin's highlight groups, with the colours its
//! default links usually resolve to.

use ratatui::style::{Color, Modifier, Style};

pub const CARD: Style = Style::new().fg(Color::DarkGray);
pub const CARD_FOCUS: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
pub const PENDING: Style = Style::new().fg(Color::Blue);
pub const FAILED: Style = Style::new().fg(Color::Red);
pub const NUMBER: Style = Style::new().fg(Color::Yellow);
pub const TITLE: Style = Style::new();
pub const DIM: Style = Style::new().fg(Color::DarkGray);
pub const HEADER: Style = Style::new().add_modifier(Modifier::BOLD);
pub const HEADER_FOCUS: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Cyan)
    .add_modifier(Modifier::BOLD);
pub const KEY: Style = Style::new().fg(Color::Cyan);
pub const ASSIGNEE: Style = Style::new().fg(Color::Cyan);
pub const ASSIGNEE_ME: Style = Style::new().fg(Color::Green);
pub const LABEL: Style = Style::new().fg(Color::Magenta);
pub const STATUS_BAR: Style = Style::new().bg(Color::Black);
pub const INFO: Style = Style::new();
pub const WARN: Style = Style::new().fg(Color::Yellow);
pub const ERROR: Style = Style::new().fg(Color::Red);

/// A priority by its 0-based rank in the field: Urgent, High, Medium, Low.
pub fn priority(rank: usize) -> Style {
    match rank {
        0 => Style::new().fg(Color::Red),
        1 => Style::new().fg(Color::Yellow),
        2 => Style::new().fg(Color::Blue),
        3 => Style::new().fg(Color::Cyan),
        _ => Style::new().fg(Color::Magenta),
    }
}
