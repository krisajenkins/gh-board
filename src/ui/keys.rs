//! Keys -> UI changes, messages for the reducer, and requests for the shell.
//! Pure: `handle_key` is data in, data out.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{State, board, op_counts};
use crate::types::IssueState;
use crate::ui::detail;
use crate::ui::editor::{Edit, EditKind};
use crate::ui::picker::{Picker, PickerKind, initial_cursor, menu};
use crate::ui::state::{Mode, UiState};
use crate::update::{Direction, Level, Msg, Towards};

/// Something only the shell can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Open a link in the browser.
    OpenUrl(String),
    /// Copy text to the clipboard; the shell reports how it went, naming it
    /// `what`.
    Copy {
        text: String,
        what: String,
    },
    /// Repaint the whole screen (`Ctrl-l`).
    Redraw,
    /// Show a notice (the shell stamps the time).
    Notify(String, Level),
    /// Suspend the UI and edit text in `$EDITOR`.
    Edit(Edit),
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
    ("F", "feedback on gh-board (title, then $EDITOR)"),
    ("D", "show / hide hidden statuses"),
    ("R", "reload everything from GitHub"),
    ("!", "review failed changes"),
    ("o", "open the card in the browser"),
    ("y", "copy the card's URL to the clipboard"),
    ("Enter", "card detail"),
    ("?", "this help"),
    ("q", "quit"),
];

/// Copy a card's link, naming it by the link itself in the notice.
fn copy_url(url: String) -> Action {
    Action::Copy {
        what: url.clone(),
        text: url,
    }
}

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
            title_key(
                ui,
                input,
                key,
                |input| Mode::NewIssue { input },
                |title| Outcome::msg(Msg::Create { title }),
            )
        }
        Mode::Feedback { input } => {
            let input = input.clone();
            title_key(
                ui,
                input,
                key,
                |input| Mode::Feedback { input },
                // The title first, then the description in $EDITOR.
                |title| {
                    Outcome::action(Action::Edit(Edit {
                        kind: EditKind::Feedback { title },
                        initial: String::new(),
                    }))
                },
            )
        }
        Mode::ConfirmQuit { .. } => confirm_quit_key(ui, key),
        _ if ui.prompt.is_some() => prompt_key(ui, key),
        Mode::Help => (
            UiState {
                mode: Mode::Board,
                ..ui
            },
            Outcome::default(),
        ),
        Mode::Picker(picker) => {
            let picker = picker.clone();
            picker_key(ui, state, picker, key)
        }
        Mode::Board if state.detail.is_some() => detail_key(ui, state, key),
        Mode::Board => board_key(ui, state, key),
    }
}

/// Quit, unless changes are still on their way to GitHub: then ask first.
/// Failed changes do not count; nothing more will happen to them.
fn quit(ui: UiState, state: &State) -> (UiState, Outcome) {
    match op_counts(state).0 {
        0 => (ui, Outcome::action(Action::Quit)),
        pending => (
            UiState {
                mode: Mode::ConfirmQuit { pending },
                ..ui
            },
            Outcome::default(),
        ),
    }
}

fn confirm_quit_key(ui: UiState, key: KeyEvent) -> (UiState, Outcome) {
    let board = UiState {
        mode: Mode::Board,
        ..ui
    };
    match key.code {
        KeyCode::Char('y' | 'Y') => (board, Outcome::action(Action::Quit)),
        _ => (board, Outcome::default()),
    }
}

fn with_mode(ui: UiState, mode: Mode) -> UiState {
    UiState { mode, ..ui }
}

/// `F`, from the board or the detail view: prompt for feedback.
fn feedback(ui: UiState) -> UiState {
    with_mode(
        ui,
        Mode::Feedback {
            input: String::new(),
        },
    )
}

fn picker_key(ui: UiState, state: &State, picker: Picker, key: KeyEvent) -> (UiState, Outcome) {
    let Some(m) = menu(state, &picker) else {
        return (with_mode(ui, Mode::Board), Outcome::default());
    };
    let n = m.choices.len();
    let last = n.saturating_sub(1);
    let cursor = picker.cursor.min(last);
    let moved = |cursor| {
        (
            with_mode(
                ui.clone(),
                Mode::Picker(Picker {
                    cursor,
                    ..picker.clone()
                }),
            ),
            Outcome::default(),
        )
    };
    let close = || (with_mode(ui.clone(), Mode::Board), Outcome::default());
    if key.code == KeyCode::Esc || ctrl(&key, 'c') {
        return close();
    }
    if key.code == KeyCode::Enter {
        return match m.choices.get(cursor) {
            Some(choice) if m.stays_open => (
                with_mode(ui, Mode::Picker(Picker { cursor, ..picker })),
                Outcome::msg(choice.msg.clone()),
            ),
            Some(choice) => (with_mode(ui, Mode::Board), Outcome::msg(choice.msg.clone())),
            None => (ui, Outcome::default()),
        };
    }
    match key.code {
        KeyCode::Down => return moved((cursor + 1).min(last)),
        KeyCode::Up => return moved(cursor.saturating_sub(1)),
        KeyCode::Home => return moved(0),
        KeyCode::End => return moved(last),
        _ if ctrl(&key, 'n') => return moved((cursor + 1).min(last)),
        _ if ctrl(&key, 'p') => return moved(cursor.saturating_sub(1)),
        _ => {}
    }
    if m.filter.is_some() {
        // Everything else edits the filter, which starts the cursor afresh.
        return match edit(&picker.filter, &key) {
            Some(filter) if filter != picker.filter => (
                with_mode(
                    ui,
                    Mode::Picker(Picker {
                        cursor: 0,
                        filter,
                        ..picker
                    }),
                ),
                Outcome::default(),
            ),
            _ => (ui, Outcome::default()),
        };
    }
    match key.code {
        KeyCode::Char('q') => close(),
        KeyCode::Char('j') => moved((cursor + 1).min(last)),
        KeyCode::Char('k') => moved(cursor.saturating_sub(1)),
        KeyCode::Char('g') => moved(0),
        KeyCode::Char('G') => moved(last),
        _ => (ui, Outcome::default()),
    }
}

fn detail_key(ui: UiState, state: &State, key: KeyEvent) -> (UiState, Outcome) {
    let g_was_pending = ui.pending_g;
    let ui = UiState {
        pending_g: false,
        ..ui
    };
    let scroll = |ui: UiState, to: usize| {
        (
            UiState {
                detail_scroll: to,
                ..ui
            },
            Outcome::default(),
        )
    };
    let page = ui.detail_height.max(1);
    let half = (page / 2).max(1);
    let at = ui.detail_scroll;
    if ctrl(&key, 'd') {
        return scroll(ui, at.saturating_add(half));
    }
    if ctrl(&key, 'u') {
        return scroll(ui, at.saturating_sub(half));
    }
    if ctrl(&key, 'c') {
        return quit(ui, state);
    }
    if ctrl(&key, 'l') {
        return (ui, Outcome::action(Action::Redraw));
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return (ui, Outcome::default());
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => return scroll(ui, at.saturating_add(1)),
        KeyCode::Char('k') | KeyCode::Up => return scroll(ui, at.saturating_sub(1)),
        KeyCode::PageDown => return scroll(ui, at.saturating_add(page)),
        KeyCode::PageUp => return scroll(ui, at.saturating_sub(page)),
        KeyCode::Char('g') if g_was_pending => return scroll(ui, 0),
        KeyCode::Home => return scroll(ui, 0),
        // Clamped to the end by the next settle.
        KeyCode::Char('G') | KeyCode::End => return scroll(ui, usize::MAX),
        KeyCode::Char('g') => {
            return (
                UiState {
                    pending_g: true,
                    ..ui
                },
                Outcome::default(),
            );
        }
        KeyCode::Char('q') | KeyCode::Esc => return (ui, Outcome::msg(Msg::CloseDetail)),
        KeyCode::Char('r') => return (ui, Outcome::msg(Msg::ReloadDetail)),
        KeyCode::Char('<') => return (ui, Outcome::msg(Msg::MoveCard(Towards::Prev))),
        KeyCode::Char('>') => return (ui, Outcome::msg(Msg::MoveCard(Towards::Next))),
        KeyCode::Char('!') => return (ui, Outcome::msg(Msg::ReviewFailures)),
        KeyCode::Char('?') => return (with_mode(ui, Mode::Help), Outcome::default()),
        KeyCode::Char('F') => return (feedback(ui), Outcome::default()),
        _ => {}
    }
    let Some(card) = detail::card(state) else {
        return (ui, Outcome::default());
    };
    let item_id = card.item_id.clone();
    let open = |ui: UiState, kind: PickerKind| {
        let cursor = initial_cursor(state, kind, &item_id);
        with_mode(
            ui,
            Mode::Picker(Picker {
                kind,
                item_id: item_id.clone(),
                cursor,
                filter: String::new(),
            }),
        )
    };
    let edit = |kind, initial: String| Outcome::action(Action::Edit(Edit { kind, initial }));
    match key.code {
        KeyCode::Char('o') => match card.url {
            Some(url) => (ui, Outcome::action(Action::OpenUrl(url))),
            None => (ui, Outcome::default()),
        },
        KeyCode::Char('y') => match card.url {
            Some(url) => (ui, Outcome::action(copy_url(url))),
            None => (ui, Outcome::default()),
        },
        KeyCode::Char('m') => (ui, Outcome::msg(Msg::AssignMe { item_id })),
        KeyCode::Char('u') => (ui, Outcome::msg(Msg::UnassignAll { item_id })),
        KeyCode::Char('s') => (open(ui, PickerKind::Status), Outcome::default()),
        KeyCode::Char('p') => {
            let has_field = state
                .meta
                .as_ref()
                .is_some_and(|m| m.priority_field.is_some());
            if has_field {
                (open(ui, PickerKind::Priority), Outcome::default())
            } else {
                (
                    ui,
                    Outcome::action(Action::Notify(
                        "This board has no priority field".into(),
                        Level::Warn,
                    )),
                )
            }
        }
        KeyCode::Char('x') if card.state == Some(IssueState::Closed) => (
            ui,
            Outcome::msg(Msg::SetState {
                item_id,
                issue_state: IssueState::Open,
                reason: None,
            }),
        ),
        KeyCode::Char('x') => (open(ui, PickerKind::Close), Outcome::default()),
        KeyCode::Char('a') => {
            let load = match (&card.repo_id, &card.repo) {
                (Some(repo_id), Some(repo)) if !state.assignable.contains_key(repo_id) => {
                    vec![Msg::LoadAssignable {
                        repo_id: repo_id.clone(),
                        repo: repo.clone(),
                    }]
                }
                _ => vec![],
            };
            (
                open(ui, PickerKind::Assignee),
                Outcome {
                    msgs: load,
                    actions: vec![],
                },
            )
        }
        KeyCode::Char('c') => (
            ui,
            edit(
                EditKind::Comment {
                    item_id: item_id.clone(),
                },
                String::new(),
            ),
        ),
        KeyCode::Char('e') => {
            match crate::model::description(state.detail.as_ref(), &state.ops, &item_id) {
                Some((body, _)) => {
                    let body = body.to_string();
                    (
                        ui,
                        edit(
                            EditKind::Description {
                                item_id: item_id.clone(),
                            },
                            body,
                        ),
                    )
                }
                None => (
                    ui,
                    Outcome::action(Action::Notify(
                        "The description is still loading; try again in a moment".into(),
                        Level::Info,
                    )),
                ),
            }
        }
        _ => (ui, Outcome::default()),
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

/// A one-line title prompt: Enter does `submit(title)` unless it is blank,
/// Esc or `Ctrl-c` cancel, and anything else edits the input.
fn title_key(
    ui: UiState,
    input: String,
    key: KeyEvent,
    mode: impl Fn(String) -> Mode,
    submit: impl Fn(String) -> Outcome,
) -> (UiState, Outcome) {
    let board = with_mode(ui.clone(), Mode::Board);
    if key.code == KeyCode::Esc || ctrl(&key, 'c') {
        return (board, Outcome::default());
    }
    if key.code == KeyCode::Enter {
        let title = input.trim();
        let out = if title.is_empty() {
            Outcome::default()
        } else {
            submit(title.into())
        };
        return (board, out);
    }
    match edit(&input, &key) {
        Some(input) => (with_mode(ui, mode(input)), Outcome::default()),
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
        return quit(ui, state);
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
        KeyCode::Char('F') => return (feedback(ui), Outcome::default()),
        KeyCode::Char('D') => Outcome::msg(Msg::ToggleHidden),
        KeyCode::Char('R') => Outcome::msg(Msg::Refresh),
        KeyCode::Char('!') => Outcome::msg(Msg::ReviewFailures),
        KeyCode::Char('o') => match board(state).focused().and_then(|c| c.url.clone()) {
            Some(url) => Outcome::action(Action::OpenUrl(url)),
            None => Outcome::default(),
        },
        KeyCode::Char('y') => match board(state).focused().and_then(|c| c.url.clone()) {
            Some(url) => Outcome::action(copy_url(url)),
            None => Outcome::default(),
        },
        KeyCode::Enter => {
            return (
                UiState {
                    detail_scroll: 0,
                    ..ui
                },
                Outcome::msg(Msg::OpenDetail { item_id: None }),
            );
        }
        KeyCode::Char('?') => {
            return (
                UiState {
                    mode: Mode::Help,
                    ..ui
                },
                Outcome::default(),
            );
        }
        KeyCode::Char('q') => return quit(ui, state),
        _ => Outcome::default(),
    };
    (ui, out)
}
