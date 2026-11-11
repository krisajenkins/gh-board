//! Key handling, as data: key in -> UI changes, messages and actions out.

mod common;

use std::time::{Duration, Instant};

use common::*;
use gh_board::model::State;
use gh_board::ui::editor::{Edit, EditKind};
use gh_board::ui::{Action, Mode, Outcome, UiState, handle_key};
use gh_board::update::{Direction, Effect, Level, Msg, Towards, update};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ch(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn msgs(msgs: Vec<Msg>) -> Outcome {
    Outcome {
        msgs,
        actions: vec![],
    }
}

fn actions(actions: Vec<Action>) -> Outcome {
    Outcome {
        msgs: vec![],
        actions,
    }
}

/// Press a key on the board in its plain state; the outcome only.
fn on_board(k: KeyEvent) -> Outcome {
    handle_key(&UiState::default(), &loaded(), k).1
}

/// Press keys in turn, running each message through the reducer as the app
/// loop would. Returns the final UI, state, and every message sent.
fn press(ui: UiState, s: State, keys: &[KeyEvent]) -> (UiState, State, Vec<Msg>) {
    keys.iter().fold((ui, s, vec![]), |(ui, s, mut sent), &k| {
        let (ui, out) = handle_key(&ui, &s, k);
        let s = out.msgs.iter().cloned().fold(s, |s, m| update(s, m).0);
        sent.extend(out.msgs);
        (ui, s, sent)
    })
}

// Board keys ------------------------------------------------------------------

#[test]
fn hjkl_and_arrows_move_focus() {
    for (k, dir) in [
        (ch('h'), Direction::Left),
        (ch('j'), Direction::Down),
        (ch('k'), Direction::Up),
        (ch('l'), Direction::Right),
        (key(KeyCode::Left), Direction::Left),
        (key(KeyCode::Down), Direction::Down),
        (key(KeyCode::Up), Direction::Up),
        (key(KeyCode::Right), Direction::Right),
        (ch('G'), Direction::Bottom),
    ] {
        assert_eq!(on_board(k), msgs(vec![Msg::Focus(dir)]), "{k:?}");
    }
}

#[test]
fn g_waits_for_a_second_g() {
    let (ui, out) = handle_key(&UiState::default(), &loaded(), ch('g'));
    assert!(ui.pending_g);
    assert_eq!(out, Outcome::default());
    let (ui, out) = handle_key(&ui, &loaded(), ch('g'));
    assert!(!ui.pending_g);
    assert_eq!(out, msgs(vec![Msg::Focus(Direction::Top)]));
}

#[test]
fn another_key_after_g_cancels_it_and_acts_normally() {
    let (ui, _) = handle_key(&UiState::default(), &loaded(), ch('g'));
    let (ui, out) = handle_key(&ui, &loaded(), ch('j'));
    assert!(!ui.pending_g);
    assert_eq!(out, msgs(vec![Msg::Focus(Direction::Down)]));
}

#[test]
fn shift_g_arrives_with_the_shift_modifier_too() {
    let k = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
    assert_eq!(on_board(k), msgs(vec![Msg::Focus(Direction::Bottom)]));
}

#[test]
fn digits_pick_the_priority_filter() {
    for n in 0..=9 {
        let c = char::from_digit(n, 10).unwrap();
        assert_eq!(on_board(ch(c)), msgs(vec![Msg::FilterKey(n as usize)]));
    }
}

#[test]
fn board_operations_are_one_message_each() {
    assert_eq!(on_board(ch('<')), msgs(vec![Msg::MoveCard(Towards::Prev)]));
    assert_eq!(on_board(ch('>')), msgs(vec![Msg::MoveCard(Towards::Next)]));
    assert_eq!(on_board(ch('D')), msgs(vec![Msg::ToggleHidden]));
    assert_eq!(on_board(ch('R')), msgs(vec![Msg::Refresh]));
    assert_eq!(on_board(ch('!')), msgs(vec![Msg::ReviewFailures]));
}

#[test]
fn o_opens_the_focused_cards_link() {
    assert_eq!(
        on_board(ch('o')),
        actions(vec![Action::OpenUrl(
            "https://github.com/acme/plans/issues/1".into()
        )])
    );
}

#[test]
fn o_does_nothing_without_a_board() {
    let s = step(State::new(config()), Msg::Refresh);
    assert_eq!(
        handle_key(&UiState::default(), &s, ch('o')).1,
        Outcome::default()
    );
}

#[test]
fn enter_opens_the_detail() {
    assert_eq!(
        on_board(key(KeyCode::Enter)),
        msgs(vec![Msg::OpenDetail { item_id: None }])
    );
}

#[test]
fn q_and_ctrl_c_quit() {
    assert_eq!(on_board(ch('q')), actions(vec![Action::Quit]));
    assert_eq!(on_board(ctrl('c')), actions(vec![Action::Quit]));
}

#[test]
fn unbound_keys_do_nothing() {
    assert_eq!(on_board(ch('z')), Outcome::default());
    assert_eq!(on_board(ctrl('j')), Outcome::default());
}

// Clearing the search ---------------------------------------------------------

#[test]
fn esc_clears_an_active_search() {
    let s = step(loaded(), Msg::Search("blog".into()));
    let (_, out) = handle_key(&UiState::default(), &s, key(KeyCode::Esc));
    assert_eq!(out, msgs(vec![Msg::Search(String::new())]));
}

#[test]
fn esc_with_no_search_does_nothing() {
    assert_eq!(on_board(key(KeyCode::Esc)), Outcome::default());
}

#[test]
fn ctrl_l_clears_the_search_and_redraws() {
    let s = step(loaded(), Msg::Search("blog".into()));
    let (_, out) = handle_key(&UiState::default(), &s, ctrl('l'));
    assert_eq!(
        out,
        Outcome {
            msgs: vec![Msg::Search(String::new())],
            actions: vec![Action::Redraw],
        }
    );
    assert_eq!(on_board(ctrl('l')), actions(vec![Action::Redraw]));
}

// The search prompt -----------------------------------------------------------

#[test]
fn slash_opens_the_search_prompt_on_the_current_query() {
    let s = step(loaded(), Msg::Search("old".into()));
    let (ui, out) = handle_key(&UiState::default(), &s, ch('/'));
    assert_eq!(
        ui.mode,
        Mode::Search {
            before: "old".into(),
            input: "old".into(),
        }
    );
    assert_eq!(out, Outcome::default());
}

#[test]
fn each_keystroke_searches_live() {
    let (ui, s, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('/'), ch('b'), ch('l'), ch('o')],
    );
    assert_eq!(
        sent,
        vec![
            Msg::Search("b".into()),
            Msg::Search("bl".into()),
            Msg::Search("blo".into()),
        ]
    );
    assert_eq!(s.search, "blo");
    assert!(matches!(ui.mode, Mode::Search { ref input, .. } if input == "blo"));
}

#[test]
fn backspace_and_ctrl_u_edit_live_too() {
    let (_, s, sent) = press(
        UiState::default(),
        loaded(),
        &[
            ch('/'),
            ch('a'),
            ch('b'),
            key(KeyCode::Backspace),
            ctrl('u'),
        ],
    );
    assert_eq!(
        &sent[2..],
        [Msg::Search("a".into()), Msg::Search("".into())]
    );
    assert_eq!(s.search, "");
}

#[test]
fn ctrl_w_deletes_a_word() {
    let (ui, _, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('/'), ch('a'), ch(' '), ch('b'), ctrl('w')],
    );
    assert_eq!(sent.last(), Some(&Msg::Search("a ".into())));
    assert!(matches!(ui.mode, Mode::Search { ref input, .. } if input == "a "));
}

#[test]
fn backspace_on_an_empty_query_sends_nothing() {
    let (_, _, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('/'), key(KeyCode::Backspace)],
    );
    assert!(sent.is_empty());
}

#[test]
fn enter_keeps_the_query() {
    let (ui, s, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('/'), ch('x'), key(KeyCode::Enter)],
    );
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(s.search, "x");
    assert_eq!(sent, vec![Msg::Search("x".into())]);
}

#[test]
fn esc_restores_the_previous_query() {
    let s = step(loaded(), Msg::Search("old".into()));
    let (ui, s, sent) = press(
        UiState::default(),
        s,
        &[ch('/'), ch('x'), key(KeyCode::Esc)],
    );
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(s.search, "old");
    assert_eq!(sent.last(), Some(&Msg::Search("old".into())));
}

#[test]
fn board_keys_are_text_while_searching() {
    let (ui, _, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('/'), ch('q'), ch('j'), ch('1')],
    );
    assert_eq!(sent.last(), Some(&Msg::Search("qj1".into())));
    assert!(matches!(ui.mode, Mode::Search { .. }));
}

// The new-issue prompt --------------------------------------------------------

#[test]
fn n_prompts_for_a_title_then_creates_on_enter() {
    let (ui, _, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('n'), ch('H'), ch('i'), ch(' ')],
    );
    assert_eq!(
        ui.mode,
        Mode::NewIssue {
            input: "Hi ".into()
        }
    );
    assert!(sent.is_empty(), "typing a title sends nothing");
    let (ui, out) = handle_key(&ui, &loaded(), key(KeyCode::Enter));
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(out, msgs(vec![Msg::Create { title: "Hi".into() }]));
}

#[test]
fn an_empty_title_creates_nothing() {
    let (ui, _, _) = press(UiState::default(), loaded(), &[ch('n'), ch(' ')]);
    let (ui, out) = handle_key(&ui, &loaded(), key(KeyCode::Enter));
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(out, Outcome::default());
}

#[test]
fn esc_cancels_a_new_issue() {
    let (ui, _, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('n'), ch('x'), key(KeyCode::Esc)],
    );
    assert_eq!(ui.mode, Mode::Board);
    assert!(sent.is_empty());
}

#[test]
fn creating_puts_a_new_card_on_the_board() {
    let (_, s, _) = press(
        UiState::default(),
        loaded(),
        &[ch('n'), ch('X'), key(KeyCode::Enter)],
    );
    assert!(
        gh_board::model::view(&s)
            .cards
            .values()
            .any(|c| c.temp && c.title == "X")
    );
}

// The feedback prompt ---------------------------------------------------------

#[test]
fn shift_f_prompts_for_a_title_then_edits_the_description() {
    let (ui, _, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('F'), ch(' '), ch('O'), ch('k'), ch(' ')],
    );
    assert_eq!(
        ui.mode,
        Mode::Feedback {
            input: " Ok ".into()
        }
    );
    assert!(sent.is_empty(), "typing feedback sends nothing");
    let (ui, out) = handle_key(&ui, &loaded(), key(KeyCode::Enter));
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(
        out,
        actions(vec![Action::Edit(Edit {
            kind: EditKind::Feedback { title: "Ok".into() },
            initial: String::new(),
        })]),
        "Enter sends nothing yet: the description comes next"
    );
}

#[test]
fn blank_feedback_sends_nothing_and_esc_cancels() {
    let (ui, _, _) = press(UiState::default(), loaded(), &[ch('F'), ch(' ')]);
    let (ui, out) = handle_key(&ui, &loaded(), key(KeyCode::Enter));
    assert_eq!((ui.mode, out), (Mode::Board, Outcome::default()));

    let (ui, _, sent) = press(
        UiState::default(),
        loaded(),
        &[ch('F'), ch('x'), key(KeyCode::Esc)],
    );
    assert_eq!(ui.mode, Mode::Board);
    assert!(sent.is_empty());
}

#[test]
fn feedback_leaves_the_board_alone() {
    let before = loaded();
    let (_, s, _) = press(
        UiState::default(),
        before.clone(),
        &[ch('F'), ch('X'), key(KeyCode::Enter)],
    );
    let (s, _) = update(
        s,
        Msg::SendFeedback {
            title: "X".into(),
            body: "Y".into(),
        },
    );
    assert_eq!(gh_board::model::view(&s), gh_board::model::view(&before));
    assert!(s.ops.is_empty(), "feedback is not an op");
}

// Help ------------------------------------------------------------------------

#[test]
fn question_mark_shows_help_and_any_key_closes_it() {
    let (ui, _) = handle_key(&UiState::default(), &loaded(), ch('?'));
    assert_eq!(ui.mode, Mode::Help);
    let (ui, out) = handle_key(&ui, &loaded(), ch('j'));
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(out, Outcome::default(), "the closing key does nothing else");
}

// The failure prompt ----------------------------------------------------------

fn prompting() -> (UiState, State, u64) {
    let (s, op) = with_failed_op();
    let id = op.id;
    let ui = UiState::default().apply_effect(Effect::PromptFailure { op }, Instant::now());
    (ui, s, id)
}

#[test]
fn the_failure_prompt_answers_retry_abandon_or_later() {
    let (ui, s, op_id) = prompting();
    for (k, msg) in [
        (ch('r'), Msg::OpRetry { op_id }),
        (ch('a'), Msg::OpAbandon { op_id }),
        (key(KeyCode::Esc), Msg::PromptDismissed { op_id }),
    ] {
        let (after, out) = handle_key(&ui, &s, k);
        assert_eq!(after.prompt, None);
        assert_eq!(out, msgs(vec![msg]));
    }
}

#[test]
fn the_failure_prompt_swallows_other_keys() {
    let (ui, s, _) = prompting();
    let (after, out) = handle_key(&ui, &s, ch('j'));
    assert!(after.prompt.is_some());
    assert_eq!(out, Outcome::default());
}

#[test]
fn a_failure_does_not_interrupt_typing() {
    let (ui, s, op_id) = prompting();
    let ui = UiState {
        mode: Mode::Search {
            before: String::new(),
            input: String::new(),
        },
        ..ui
    };
    let (ui, out) = handle_key(&ui, &s, ch('r'));
    assert_eq!(out, msgs(vec![Msg::Search("r".into())]));
    // Once the search is done, the prompt is there.
    let (ui, _) = handle_key(&ui, &s, key(KeyCode::Enter));
    let (_, out) = handle_key(&ui, &s, ch('a'));
    assert_eq!(out, msgs(vec![Msg::OpAbandon { op_id }]));
}

#[test]
fn retrying_through_the_reducer_clears_the_failure() {
    let (ui, s, _) = prompting();
    let (_, s, _) = press(ui, s, &[ch('r')]);
    assert_eq!(gh_board::model::op_counts(&s), (1, 0));
}

// Notices ---------------------------------------------------------------------

#[test]
fn notices_expire_and_any_key_dismisses_them() {
    let now = Instant::now();
    let ui = UiState::default().apply_effect(
        Effect::Notify {
            message: "hello".into(),
            level: Level::Info,
        },
        now,
    );
    assert_eq!(
        ui.notice.as_ref().map(|n| n.message.as_str()),
        Some("hello")
    );
    assert_eq!(ui.expire(now + Duration::from_secs(1)), None, "not yet");
    assert_eq!(
        ui.expire(now + Duration::from_secs(60)).map(|u| u.notice),
        Some(None)
    );
    let (after, _) = handle_key(&ui, &loaded(), ch('z'));
    assert_eq!(after.notice, None);
}

#[test]
fn errors_stay_up_longer_than_info() {
    let now = Instant::now();
    let at = |level| {
        UiState::default()
            .notify("x".into(), level, now)
            .notice
            .map(|n| n.until)
    };
    assert!(at(Level::Error) > at(Level::Info));
}

// Scrolling -------------------------------------------------------------------

#[test]
fn going_to_the_bottom_of_a_long_column_scrolls_it() {
    let s = loaded_with(config(), cards((1..=56).map(card)));
    let area = Rect::new(0, 0, 100, 30);
    let ui = UiState::default().settle(&s, area);
    let todo = gh_board::model::board(&s).columns[0].key.clone();
    assert_eq!(ui.scroll.first_card.get(&todo), Some(&0));
    let (ui, s, _) = press(ui, s, &[ch('G')]);
    let ui = ui.settle(&s, area);
    // 27 board lines hold six 4-line cards: 50..=55.
    assert_eq!(ui.scroll.first_card.get(&todo), Some(&50));
    let (ui, s, _) = press(ui, s, &[ch('g'), ch('g')]);
    assert_eq!(ui.settle(&s, area).scroll.first_card.get(&todo), Some(&0));
}

#[test]
fn moving_right_past_the_edge_scrolls_columns() {
    let s = loaded();
    // 40 cells hold two 18-wide columns; there are four.
    let area = Rect::new(0, 0, 40, 20);
    let (ui, s, _) = press(UiState::default(), s, &[ch('l'), ch('l')]);
    assert_eq!(ui.settle(&s, area).scroll.first_col, 1);
}
