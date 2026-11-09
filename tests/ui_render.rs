//! Rendering, through ratatui's `TestBackend`. Snapshots catch the text;
//! colours (focus, pending, failed borders) are asserted on buffer cells.

mod common;

use std::time::Instant;

use common::*;
use gh_board::model::State;
use gh_board::types::{Card, IssueState};
use gh_board::ui::theme;
use gh_board::ui::{Mode, UiState, render};
use gh_board::update::{Direction, Effect, Level, Msg};
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

const W: u16 = 100;
const H: u16 = 30;

/// Settle scrolling, then draw, as the app loop does.
fn draw_sized(s: &State, ui: &UiState, w: u16, h: u16) -> Terminal<TestBackend> {
    let ui = ui.settle(s, Rect::new(0, 0, w, h));
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
    t.draw(|f| render(f, s, &ui)).expect("draw");
    t
}

fn draw(s: &State, ui: &UiState) -> Terminal<TestBackend> {
    draw_sized(s, ui, W, H)
}

/// The style a cell was drawn with, colours and modifiers only.
fn style_at(buf: &Buffer, x: u16, y: u16) -> Style {
    let cell = &buf[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

/// Assert a card's top-left corner is at (x, y) with this border style.
fn assert_border(buf: &Buffer, x: u16, y: u16, want: Style) {
    assert_eq!(buf[(x, y)].symbol(), "╭", "a card's corner at ({x}, {y})");
    let got = style_at(buf, x, y);
    assert_eq!(got.fg, want.fg, "border colour at ({x}, {y})");
    // The bottom-right corner shares it.
    let mut bx = x;
    while buf[(bx, y)].symbol() != "╮" {
        bx += 1;
    }
    let mut by = y + 1;
    while buf[(bx, by)].symbol() != "╯" {
        by += 1;
    }
    assert_eq!(buf[(bx, by)].fg, want.fg.unwrap_or_default());
}

// Before the board ------------------------------------------------------------

#[test]
fn loading() {
    let s = step(State::new(config()), Msg::Refresh);
    assert_snapshot!(draw(&s, &UiState::default()).backend());
}

#[test]
fn load_failed() {
    let s = step(State::new(config()), Msg::Refresh);
    let seq = s.seq;
    let s = step(
        s,
        Msg::BoardFailed {
            seq,
            err: "gh: HTTP 401: Bad credentials".into(),
        },
    );
    assert_snapshot!(draw(&s, &UiState::default()).backend());
}

// The board -------------------------------------------------------------------

#[test]
fn loaded_board() {
    let t = draw(&loaded(), &UiState::default());
    assert_snapshot!(t.backend());
    // Todo is focused, on its first card; the others are plain.
    let buf = t.backend().buffer();
    assert_border(buf, 0, 1, theme::CARD_FOCUS);
    assert_border(buf, 0, 5, theme::CARD);
    assert_eq!(buf[(1, 0)].bg, theme::HEADER_FOCUS.bg.unwrap());
}

#[test]
fn focus_moves_the_highlight() {
    let s = step(loaded(), Msg::Focus(Direction::Down));
    let t = draw(&s, &UiState::default());
    let buf = t.backend().buffer();
    assert_border(buf, 0, 1, theme::CARD);
    assert_border(buf, 0, 5, theme::CARD_FOCUS);
    // Over to In Progress: (100 - 3) / 4 = 24 wide, so column 2 starts at 50.
    let s = step(s, Msg::Focus(Direction::Right));
    let s = step(s, Msg::Focus(Direction::Right));
    let t = draw(&s, &UiState::default());
    let buf = t.backend().buffer();
    assert_border(buf, 0, 5, theme::CARD);
    assert_border(buf, 50, 1, theme::CARD_FOCUS);
    assert_eq!(buf[(51, 0)].bg, theme::HEADER_FOCUS.bg.unwrap());
}

#[test]
fn pending_and_failed_cards() {
    // #2 (Todo, third) has a failed change; #3 (second) one in flight.
    let (s, _) = with_failed_op();
    let s = step(
        s,
        Msg::SetPriority {
            item_id: "item3".into(),
            priority_id: Some("p-urgent".into()),
        },
    );
    let t = draw(&s, &UiState::default());
    assert_snapshot!(t.backend());
    let buf = t.backend().buffer();
    assert_border(buf, 0, 5, theme::PENDING);
    assert_border(buf, 0, 9, theme::FAILED);
    assert_eq!(buf[(2, 6)].symbol(), "~");
    assert_eq!(buf[(2, 6)].fg, theme::PENDING.fg.unwrap());
    assert_eq!(buf[(2, 10)].symbol(), "!");
    assert_eq!(buf[(2, 10)].fg, theme::FAILED.fg.unwrap());
}

#[test]
fn a_focused_pending_card_keeps_its_marker() {
    let s = step(
        loaded(),
        Msg::SetPriority {
            item_id: "item1".into(),
            priority_id: Some("p-low".into()),
        },
    );
    let t = draw(&s, &UiState::default());
    let buf = t.backend().buffer();
    assert_border(buf, 0, 1, theme::CARD_FOCUS);
    assert_eq!(buf[(2, 2)].symbol(), "~");
}

#[test]
fn priorities_are_coloured_by_rank() {
    // #2 is High (rank 1); its meta line is the last inside its box.
    let t = draw(&loaded(), &UiState::default());
    let buf = t.backend().buffer();
    let (x, y) = (2, 11);
    let text: String = (x..x + 4).map(|x| buf[(x, y)].symbol()).collect();
    assert_eq!(text, "High");
    assert_eq!(buf[(x, y)].fg, theme::priority(1).fg.unwrap());
}

#[test]
fn long_and_wide_titles_wrap_then_truncate() {
    let s = loaded_with(
        config(),
        cards([
            Card {
                title: "Write the launch blog post for the conference talk and the release".into(),
                labels: vec!["blog".into(), "marketing".into(), "q3".into()],
                assignees: vec![user("u-amy", "amy"), user("u-zed", "zed"), me()],
                priority_id: Some("p-medium".into()),
                ..card(41)
            },
            Card {
                title: "日本語のタイトルはとても長いので折り返す必要があります".into(),
                ..card(42)
            },
            Card {
                title: "Supercalifragilisticexpialidocious".into(),
                state: Some(IssueState::Closed),
                ..card(43)
            },
        ]),
    );
    assert_snapshot!(draw(&s, &UiState::default()).backend());
}

#[test]
fn a_long_column_scrolls_to_keep_focus_visible() {
    let s = loaded_with(config(), cards((1..=56).map(card)));
    let s = (0..30).fold(s, |s, _| step(s, Msg::Focus(Direction::Down)));
    let t = draw(&s, &UiState::default());
    assert_snapshot!(t.backend());
    let buf = t.backend().buffer();
    // Focus (#31) is the last whole card: rows 25..=28 (six cards from y=1).
    assert_border(buf, 0, 21, theme::CARD_FOCUS);
}

#[test]
fn narrow_terminals_scroll_columns_sideways() {
    let s = step(loaded(), Msg::Focus(Direction::Right));
    let s = step(s, Msg::Focus(Direction::Right));
    assert_snapshot!(draw_sized(&s, &UiState::default(), 40, 16).backend());
}

#[test]
fn search_active() {
    let s = step(loaded(), Msg::Search("blog".into()));
    let ui = UiState {
        mode: Mode::Search {
            before: String::new(),
            input: "blog".into(),
        },
        ..UiState::default()
    };
    let mut t = draw(&s, &ui);
    assert_snapshot!(t.backend());
    t.backend_mut()
        .assert_cursor_position(ratatui::layout::Position::new(5, H - 2));
}

#[test]
fn filtered_with_hidden_columns_shown_and_ops_counted() {
    let (s, _) = with_failed_op();
    let s = step(s, Msg::FilterKey(2));
    let s = step(s, Msg::ToggleHidden);
    let s = step(s, Msg::Refresh);
    assert_snapshot!(draw(&s, &UiState::default()).backend());
}

#[test]
fn new_issue_prompt() {
    let ui = UiState {
        mode: Mode::NewIssue {
            input: "Fix the flaky test".into(),
        },
        ..UiState::default()
    };
    assert_snapshot!(draw(&loaded(), &ui).backend());
}

#[test]
fn notice() {
    let ui = UiState::default().notify(
        "Cannot create an issue: no repository".into(),
        Level::Error,
        Instant::now(),
    );
    let t = draw(&loaded(), &ui);
    let buf = t.backend().buffer();
    let line: String = (0..W).map(|x| buf[(x, H - 2)].symbol()).collect();
    assert_eq!(line.trim_end(), "Cannot create an issue: no repository");
    assert_eq!(buf[(0, H - 2)].fg, theme::ERROR.fg.unwrap());
}

#[test]
fn help_overlay() {
    let ui = UiState {
        mode: Mode::Help,
        ..UiState::default()
    };
    assert_snapshot!(draw(&loaded(), &ui).backend());
}

#[test]
fn failure_prompt() {
    let (s, op) = with_failed_op();
    let ui = UiState::default().apply_effect(Effect::PromptFailure { op }, Instant::now());
    assert_snapshot!(draw(&s, &ui).backend());
}
