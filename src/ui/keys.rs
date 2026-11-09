//! Keys -> UI changes, messages for the reducer, and requests for the shell.
//! Pure: `handle_key` is data in, data out.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{State, board};
use crate::ui::state::{Mode, UiState};
use crate::update::{Direction, Level, Msg, Towards};

/// Something only the shell can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Open a link in the browser.
    OpenUrl(String),
    /// Repaint the whole screen (`Ctrl-l`).
    Redraw,
    /// Show a notice (the shell stamps the time).
    Notify(String, Level),
}

/// What a key did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub msgs: Vec<Msg>,
    pub actions: Vec<Action>,
}

impl Outcome {
    fn msg(msg: Msg) -> Self {
        Outcome {
            msgs: vec![msg],
            actions: vec![],
        }
    }

    fn action(action: Action) -> Self {
        Outcome {
            msgs: vec![],
            actions: vec![action],
        }
    }
}

/// The board's keys, for the help overlay and the README.
pub const HELP: &[(&str, &str)] = &[
    ("h j k l", "move focus (arrows too)"),
    ("gg G", "first / last card in the column"),
    ("0", "show all priorities"),
    ("1..n", "show one priority, in field order"),
    ("n+1", "show cards with no priority"),
    ("/", "fuzzy search, live (Enter keeps, Esc undoes)"),
    ("Esc C-l", "clear the search (C-l also redraws)"),
    ("< >", "move the card to the previous / next status"),
    ("n", "new issue here, at the filtered priority"),
    ("D", "show / hide hidden statuses"),
    ("R", "reload everything from GitHub"),
    ("!", "review failed changes"),
    ("o", "open the card in the browser"),
    ("Enter", "card detail (not yet)"),
    ("?", "this help"),
    ("q", "quit"),
];

fn ctrl(key: &KeyEvent, c: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(c)
}

/// Handle one key press. Any key dismisses a notice.
pub fn handle_key(ui: &UiState, state: &State, key: KeyEvent) -> (UiState, Outcome) {
    let ui = UiState {
        notice: None,
        ..ui.clone()
    };
    match &ui.mode {
        Mode::Search { before, input } => {
            let (before, input) = (before.clone(), input.clone());
            search_key(ui, before, input, key)
        }
        Mode::NewIssue { input } => {
            let input = input.clone();
            new_issue_key(ui, input, key)
        }
        _ if ui.prompt.is_some() => prompt_key(ui, key),
        Mode::Help => (
            UiState {
                mode: Mode::Board,
                ..ui
            },
            Outcome::default(),
        ),
        Mode::Board => board_key(ui, state, key),
    }
}

/// Edit a one-line input. `None` when the key is not an edit.
fn edit(input: &str, key: &KeyEvent) -> Option<String> {
    if ctrl(key, 'u') {
        return Some(String::new());
    }
    if ctrl(key, 'w') {
        let trimmed = input.trim_end();
        let cut = trimmed.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        return Some(trimmed[..cut].to_string());
    }
    match key.code {
        KeyCode::Backspace => {
            let mut s = input.to_string();
            s.pop();
            Some(s)
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(format!("{input}{c}"))
        }
        _ => None,
    }
}

fn search_key(ui: UiState, before: String, input: String, key: KeyEvent) -> (UiState, Outcome) {
    let board = UiState {
        mode: Mode::Board,
        ..ui.clone()
    };
    if key.code == KeyCode::Esc || ctrl(&key, 'c') {
        return (board, Outcome::msg(Msg::Search(before)));
    }
    if key.code == KeyCode::Enter {
        return (board, Outcome::default());
    }
    match edit(&input, &key) {
        Some(edited) if edited != input => (
            UiState {
                mode: Mode::Search {
                    before,
                    input: edited.clone(),
                },
                ..ui
            },
            Outcome::msg(Msg::Search(edited)),
        ),
        _ => (ui, Outcome::default()),
    }
}

fn new_issue_key(ui: UiState, input: String, key: KeyEvent) -> (UiState, Outcome) {
    let board = UiState {
        mode: Mode::Board,
        ..ui.clone()
    };
    if key.code == KeyCode::Esc || ctrl(&key, 'c') {
        return (board, Outcome::default());
    }
    if key.code == KeyCode::Enter {
        let title = input.trim();
        let out = if title.is_empty() {
            Outcome::default()
        } else {
            Outcome::msg(Msg::Create {
                title: title.into(),
            })
        };
        return (board, out);
    }
    match edit(&input, &key) {
        Some(input) => (
            UiState {
                mode: Mode::NewIssue { input },
                ..ui
            },
            Outcome::default(),
        ),
        None => (ui, Outcome::default()),
    }
}

fn prompt_key(ui: UiState, key: KeyEvent) -> (UiState, Outcome) {
    let Some(op) = &ui.prompt else {
        return (ui, Outcome::default());
    };
    let op_id = op.id;
    let msg = match key.code {
        KeyCode::Char('r' | 'R') => Msg::OpRetry { op_id },
        KeyCode::Char('a' | 'A') => Msg::OpAbandon { op_id },
        KeyCode::Esc | KeyCode::Char('q') => Msg::PromptDismissed { op_id },
        _ => return (ui, Outcome::default()),
    };
    (UiState { prompt: None, ..ui }, Outcome::msg(msg))
}

fn board_key(ui: UiState, state: &State, key: KeyEvent) -> (UiState, Outcome) {
    let g_was_pending = ui.pending_g;
    let ui = UiState {
        pending_g: false,
        ..ui
    };
    if ctrl(&key, 'l') {
        let mut out = Outcome::action(Action::Redraw);
        if !state.search.is_empty() {
            out.msgs.push(Msg::Search(String::new()));
        }
        return (ui, out);
    }
    if ctrl(&key, 'c') {
        return (ui, Outcome::action(Action::Quit));
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return (ui, Outcome::default());
    }
    let focus = |d| Outcome::msg(Msg::Focus(d));
    let out = match key.code {
        KeyCode::Char('h') | KeyCode::Left => focus(Direction::Left),
        KeyCode::Char('j') | KeyCode::Down => focus(Direction::Down),
        KeyCode::Char('k') | KeyCode::Up => focus(Direction::Up),
        KeyCode::Char('l') | KeyCode::Right => focus(Direction::Right),
        KeyCode::Char('g') if g_was_pending => focus(Direction::Top),
        KeyCode::Char('g') => {
            return (
                UiState {
                    pending_g: true,
                    ..ui
                },
                Outcome::default(),
            );
        }
        KeyCode::Home => focus(Direction::Top),
        KeyCode::Char('G') | KeyCode::End => focus(Direction::Bottom),
        KeyCode::Char(c @ '0'..='9') => {
            Outcome::msg(Msg::FilterKey(c.to_digit(10).unwrap_or(0) as usize))
        }
        KeyCode::Char('/') => {
            return (
                UiState {
                    mode: Mode::Search {
                        before: state.search.clone(),
                        input: state.search.clone(),
                    },
                    ..ui
                },
                Outcome::default(),
            );
        }
        KeyCode::Esc if !state.search.is_empty() => Outcome::msg(Msg::Search(String::new())),
        KeyCode::Char('<') => Outcome::msg(Msg::MoveCard(Towards::Prev)),
        KeyCode::Char('>') => Outcome::msg(Msg::MoveCard(Towards::Next)),
        KeyCode::Char('n') => {
            return (
                UiState {
                    mode: Mode::NewIssue {
                        input: String::new(),
                    },
                    ..ui
                },
                Outcome::default(),
            );
        }
        KeyCode::Char('D') => Outcome::msg(Msg::ToggleHidden),
        KeyCode::Char('R') => Outcome::msg(Msg::Refresh),
        KeyCode::Char('!') => Outcome::msg(Msg::ReviewFailures),
        KeyCode::Char('o') => match board(state).focused().and_then(|c| c.url.clone()) {
            Some(url) => Outcome::action(Action::OpenUrl(url)),
            None => Outcome::default(),
        },
        KeyCode::Enter => Outcome::action(Action::Notify(
            "The detail view is not ported yet".into(),
            Level::Info,
        )),
        KeyCode::Char('?') => {
            return (
                UiState {
                    mode: Mode::Help,
                    ..ui
                },
                Outcome::default(),
            );
        }
        KeyCode::Char('q') => Outcome::action(Action::Quit),
        _ => Outcome::default(),
    };
    (ui, out)
}
