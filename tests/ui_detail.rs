//! The detail view, pickers and quit guard: keys as data, and rendering.

mod common;

use common::*;
use gh_board::model::{OpKind, State, view};
use gh_board::types::{IssueState, StateReason};
use gh_board::ui::editor::{Edit, EditKind};
use gh_board::ui::picker::{Menu, Picker, PickerKind, menu};
use gh_board::ui::theme;
use gh_board::ui::{Action, Mode, Outcome, UiState, handle_key, render};
use gh_board::update::{Direction, Level, Msg, Towards, update};
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

const W: u16 = 100;
const H: u16 = 30;

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

fn area() -> Rect {
    Rect::new(0, 0, W, H)
}

/// Press keys in turn as the app loop does: messages through the reducer,
/// and a settle (scroll clamping) between keys. Returns every message sent
/// and every action asked for.
fn press(ui: UiState, s: State, keys: &[KeyEvent]) -> (UiState, State, Vec<Msg>, Vec<Action>) {
    let ui = ui.settle(&s, area());
    keys.iter().fold(
        (ui, s, vec![], vec![]),
        |(ui, s, mut sent, mut acts), &k| {
            let (ui, out) = handle_key(&ui, &s, k);
            let s = out.msgs.iter().cloned().fold(s, |s, m| update(s, m).0);
            sent.extend(out.msgs);
            acts.extend(out.actions);
            (ui.settle(&s, area()), s, sent, acts)
        },
    )
}

fn on_detail(k: KeyEvent) -> Outcome {
    handle_key(&UiState::default(), &detail_open(), k).1
}

fn draw(s: &State, ui: &UiState) -> Terminal<TestBackend> {
    let ui = ui.settle(s, area());
    let mut t = Terminal::new(TestBackend::new(W, H)).expect("test terminal");
    t.draw(|f| render(f, s, &ui)).expect("draw");
    t
}

fn row_text(t: &Terminal<TestBackend>, y: u16) -> String {
    let buf = t.backend().buffer();
    (0..W).map(|x| buf[(x, y)].symbol()).collect()
}

/// The first row containing `needle`, if any.
fn find_row(t: &Terminal<TestBackend>, needle: &str) -> Option<u16> {
    (0..H).find(|&y| row_text(t, y).contains(needle))
}

fn picker_mode(ui: &UiState) -> &Picker {
    match &ui.mode {
        Mode::Picker(p) => p,
        other => panic!("expected a picker, got {other:?}"),
    }
}

const ITEM3: &str = "item3";

// Opening and closing ---------------------------------------------------------

#[test]
fn shift_f_prompts_for_feedback_from_the_detail_too() {
    let (ui, s, sent, _) = press(UiState::default(), detail_open(), &[ch('F'), ch('x')]);
    assert_eq!(ui.mode, Mode::Feedback { input: "x".into() });
    assert!(sent.is_empty());
    assert!(
        s.detail.is_some(),
        "the detail stays open behind the prompt"
    );
}

#[test]
fn enter_on_the_board_opens_the_focused_card() {
    let s = step(loaded(), Msg::Focus(Direction::Down));
    let (ui, s, sent, _) = press(
        UiState {
            detail_scroll: 7,
            ..UiState::default()
        },
        s,
        &[key(KeyCode::Enter)],
    );
    assert_eq!(sent, vec![Msg::OpenDetail { item_id: None }]);
    assert_eq!(s.detail.as_ref().map(|d| d.item_id.as_str()), Some(ITEM3));
    assert_eq!(ui.detail_scroll, 0, "a new detail starts at the top");
}

#[test]
fn q_and_esc_close_the_detail() {
    assert_eq!(on_detail(ch('q')), msgs(vec![Msg::CloseDetail]));
    assert_eq!(on_detail(key(KeyCode::Esc)), msgs(vec![Msg::CloseDetail]));
    let (_, s, _, _) = press(UiState::default(), detail_open(), &[ch('q')]);
    assert!(s.detail.is_none());
}

#[test]
fn detail_keys_send_their_messages() {
    let item_id = ITEM3.to_string();
    assert_eq!(on_detail(ch('r')), msgs(vec![Msg::ReloadDetail]));
    assert_eq!(
        on_detail(ch('m')),
        msgs(vec![Msg::AssignMe {
            item_id: item_id.clone()
        }])
    );
    assert_eq!(
        on_detail(ch('u')),
        msgs(vec![Msg::UnassignAll {
            item_id: item_id.clone()
        }])
    );
    assert_eq!(on_detail(ch('<')), msgs(vec![Msg::MoveCard(Towards::Prev)]));
    assert_eq!(on_detail(ch('>')), msgs(vec![Msg::MoveCard(Towards::Next)]));
    assert_eq!(on_detail(ch('!')), msgs(vec![Msg::ReviewFailures]));
    assert_eq!(
        on_detail(ch('o')),
        actions(vec![Action::OpenUrl(
            "https://github.com/acme/plans/issues/3".into()
        )])
    );
    let url = "https://github.com/acme/plans/issues/3".to_string();
    assert_eq!(
        on_detail(ch('y')),
        actions(vec![Action::Copy {
            text: url.clone(),
            what: url
        }])
    );
}

#[test]
fn board_keys_do_not_leak_through_the_detail() {
    assert_eq!(on_detail(ch('h')), Outcome::default());
    assert_eq!(on_detail(ch('1')), Outcome::default());
    assert_eq!(on_detail(ch('D')), Outcome::default());
}

#[test]
fn help_in_the_detail_returns_to_the_detail() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('?')]);
    assert_eq!(ui.mode, Mode::Help);
    let (ui, s, sent, _) = press(ui, s, &[ch('x')]);
    assert_eq!(ui.mode, Mode::Board);
    assert!(sent.is_empty());
    assert!(s.detail.is_some());
}

// Scrolling -------------------------------------------------------------------

fn long_detail() -> State {
    let body: Vec<String> = (1..=80).map(|n| format!("Line {n} of the plan.")).collect();
    detail_with(&body.join("\n"), vec![], 0)
}

#[test]
fn j_k_scroll_and_g_gg_jump() {
    let (ui, s, _, _) = press(
        UiState::default(),
        long_detail(),
        &[ch('j'), ch('j'), ch('j')],
    );
    assert_eq!(ui.detail_scroll, 3);
    let (ui, s, _, _) = press(ui, s, &[ch('k')]);
    assert_eq!(ui.detail_scroll, 2);
    let (ui, s, _, _) = press(ui, s, &[ch('G')]);
    let bottom = ui.detail_scroll;
    assert!(bottom > 50, "G goes to the end, clamped: {bottom}");
    let (ui, s, _, _) = press(ui, s, &[ch('j')]);
    assert_eq!(ui.detail_scroll, bottom, "no scrolling past the end");
    let (ui, _, _, _) = press(ui, s, &[ch('g'), ch('g')]);
    assert_eq!(ui.detail_scroll, 0);
}

#[test]
fn ctrl_d_and_ctrl_u_move_half_a_page() {
    let (ui, s, _, _) = press(UiState::default(), long_detail(), &[ctrl('d')]);
    let half = ui.detail_height / 2;
    assert!(half > 5);
    assert_eq!(ui.detail_scroll, half);
    let (ui, _, _, _) = press(ui, s, &[ctrl('u')]);
    assert_eq!(ui.detail_scroll, 0);
}

#[test]
fn a_short_detail_does_not_scroll() {
    let (ui, _, _, _) = press(UiState::default(), detail_open(), &[ch('j'), ch('G')]);
    assert_eq!(ui.detail_scroll, 0);
}

// The editor ------------------------------------------------------------------

#[test]
fn c_asks_for_an_empty_comment() {
    assert_eq!(
        on_detail(ch('c')),
        actions(vec![Action::Edit(Edit {
            kind: EditKind::Comment {
                item_id: ITEM3.into()
            },
            initial: String::new(),
        })])
    );
}

#[test]
fn e_edits_the_description_as_shown() {
    assert_eq!(
        on_detail(ch('e')),
        actions(vec![Action::Edit(Edit {
            kind: EditKind::Description {
                item_id: ITEM3.into()
            },
            initial: "Draft the post.\n\nThen ask for review.".into(),
        })])
    );
}

#[test]
fn e_starts_from_an_unconfirmed_edit() {
    let s = step(
        detail_open(),
        Msg::SetBody {
            item_id: ITEM3.into(),
            body: "Newer text".into(),
        },
    );
    let (_, out) = handle_key(&UiState::default(), &s, ch('e'));
    assert!(matches!(&out.actions[..], [Action::Edit(e)] if e.initial == "Newer text"));
}

#[test]
fn e_waits_for_the_description_to_load() {
    let (_, out) = handle_key(&UiState::default(), &detail_loading(), ch('e'));
    assert!(matches!(&out.actions[..], [Action::Notify(_, Level::Info)]));
}

// Pickers ---------------------------------------------------------------------

#[test]
fn status_picker_starts_on_the_current_status_and_picks() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('s')]);
    let p = picker_mode(&ui);
    assert_eq!(p.kind, PickerKind::Status);
    assert_eq!(p.cursor, 0, "#3 is in Todo");
    let m = menu(&s, p).expect("a menu");
    assert_eq!(
        m.choices
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>(),
        ["Todo", "Blocked", "In Progress", "Ready", "Done"]
    );
    assert!(m.choices[0].marked);
    let (ui, s, sent, _) = press(ui, s, &[ch('j'), ch('j'), key(KeyCode::Enter)]);
    assert_eq!(ui.mode, Mode::Board, "picking closes the picker");
    assert_eq!(
        sent,
        vec![Msg::SetStatus {
            item_id: ITEM3.into(),
            status_id: "s-prog".into(),
        }]
    );
    assert_eq!(
        view(&s).cards[ITEM3].status_id.as_deref(),
        Some("s-prog"),
        "optimistic"
    );
}

#[test]
fn picker_cursor_stays_in_bounds() {
    let (ui, _, _, _) = press(
        UiState::default(),
        detail_open(),
        &[ch('s'), ch('k'), ch('G'), ch('j'), ch('j')],
    );
    assert_eq!(picker_mode(&ui).cursor, 4);
}

#[test]
fn esc_cancels_a_picker_without_sending() {
    let (ui, s, sent, _) = press(
        UiState::default(),
        detail_open(),
        &[ch('s'), ch('j'), key(KeyCode::Esc)],
    );
    assert_eq!(ui.mode, Mode::Board);
    assert!(sent.is_empty());
    assert!(s.detail.is_some(), "the detail stays open");
}

#[test]
fn priority_picker_has_none_and_marks_it_when_unset() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('p')]);
    let p = picker_mode(&ui);
    let m = menu(&s, p).expect("a menu");
    assert_eq!(
        m.choices
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>(),
        ["Urgent", "High", "Medium", "Low", "None"]
    );
    assert_eq!(p.cursor, 4, "#3 has no priority");
    let (_, _, sent, _) = press(ui, s, &[ch('g'), key(KeyCode::Enter)]);
    assert_eq!(
        sent,
        vec![Msg::SetPriority {
            item_id: ITEM3.into(),
            priority_id: Some("p-urgent".into()),
        }]
    );
}

#[test]
fn priority_none_clears_it() {
    // #2 is High.
    let s = step(
        loaded(),
        Msg::OpenDetail {
            item_id: Some("item2".into()),
        },
    );
    let (ui, s, _, _) = press(UiState::default(), s, &[ch('p')]);
    assert_eq!(picker_mode(&ui).cursor, 1);
    let (_, _, sent, _) = press(ui, s, &[ch('G'), key(KeyCode::Enter)]);
    assert_eq!(
        sent,
        vec![Msg::SetPriority {
            item_id: "item2".into(),
            priority_id: None,
        }]
    );
}

#[test]
fn x_on_an_open_issue_asks_why_it_is_closing() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('x')]);
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert_eq!(
        m.choices
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>(),
        ["Completed", "Not planned", "Duplicate of…"]
    );
    let (_, s, sent, _) = press(ui, s, &[ch('j'), key(KeyCode::Enter)]);
    assert_eq!(
        sent,
        vec![Msg::SetState {
            item_id: ITEM3.into(),
            issue_state: IssueState::Closed,
            reason: Some(StateReason::NotPlanned),
        }]
    );
    assert_eq!(view(&s).cards[ITEM3].state, Some(IssueState::Closed));
}

#[test]
fn duplicate_of_picks_the_original_from_the_other_issues_by_typing() {
    let (ui, s, sent, _) = press(
        UiState::default(),
        detail_open(),
        &[
            ch('x'),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
        ],
    );
    assert!(sent.is_empty(), "nothing sent until the original is picked");
    let p = picker_mode(&ui);
    assert_eq!(p.kind, PickerKind::Duplicate);
    let labels = |s: &State, p: &Picker| {
        menu(s, p)
            .expect("a menu")
            .choices
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>()
    };
    let all = labels(&s, p);
    assert_eq!(all.len(), 4, "every issue but this one: {all:?}");
    assert!(all.iter().all(|l| !l.starts_with("#3 ")));
    let (ui, s, _, _) = press(ui, s, &[ch('#'), ch('1')]);
    assert_eq!(labels(&s, picker_mode(&ui)).len(), 1);
    let (ui, s, sent, _) = press(ui, s, &[key(KeyCode::Enter)]);
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(
        sent,
        vec![Msg::CloseAsDuplicate {
            item_id: ITEM3.into(),
            original: "item1".into(),
        }]
    );
    let card = &view(&s).cards[ITEM3];
    assert_eq!(card.state, Some(IssueState::Closed));
    assert_eq!(card.state_reason, Some(StateReason::Duplicate));
}

#[test]
fn x_on_a_closed_issue_reopens_it_at_once() {
    let (_, s, _, _) = press(
        UiState::default(),
        detail_open(),
        &[ch('x'), key(KeyCode::Enter)],
    );
    let (ui, _, sent, _) = press(UiState::default(), s, &[ch('x')]);
    assert_eq!(ui.mode, Mode::Board, "no picker");
    assert_eq!(
        sent,
        vec![Msg::SetState {
            item_id: ITEM3.into(),
            issue_state: IssueState::Open,
            reason: None,
        }]
    );
}

#[test]
fn assignee_picker_loads_users_it_does_not_have() {
    // The standard board never answered its assignable prefetch.
    let (ui, s, sent, _) = press(UiState::default(), detail_open(), &[ch('a')]);
    assert_eq!(
        sent,
        vec![Msg::LoadAssignable {
            repo_id: "r-plans".into(),
            repo: "acme/plans".into(),
        }]
    );
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert!(m.loading);
    assert_eq!(
        labels(&m),
        ["Assign to me (@me)"],
        "assigning me needs no list"
    );
    // They arrive while the picker is open.
    let s = with_assignable(s);
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert!(!m.loading);
    assert_eq!(m.choices.len(), 4);
}

#[test]
fn assignee_picker_does_not_reload_known_users() {
    let (_, _, sent, _) = press(
        UiState::default(),
        with_assignable(detail_open()),
        &[ch('a')],
    );
    assert!(sent.is_empty());
}

fn labels(m: &Menu) -> Vec<&str> {
    m.choices.iter().map(|c| c.label.as_str()).collect()
}

/// The standard board's detail with its assignee picker open.
fn assignee_picker() -> (UiState, State) {
    let (ui, s, _, _) = press(
        UiState::default(),
        with_assignable(detail_open()),
        &[ch('a')],
    );
    (ui, s)
}

#[test]
fn assignee_picks_toggle_and_the_picker_stays_open() {
    let (ui, s) = assignee_picker();
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert_eq!(
        m.choices
            .iter()
            .map(|c| (c.label.as_str(), c.marked))
            .collect::<Vec<_>>(),
        [
            ("Assign to me (@me)", false),
            ("@amy", true),
            ("@jdetter", false),
            ("@zed (Zed Zedson)", false)
        ],
        "me first, then sorted by login"
    );
    // Toggle zed on, then amy off.
    let (ui, s, sent, _) = press(ui, s, &[key(KeyCode::End), key(KeyCode::Enter)]);
    assert_eq!(picker_mode(&ui).cursor, 3, "still open, same place");
    assert!(matches!(&sent[..], [Msg::ToggleAssignee { user, .. }] if user.login == "zed"));
    let (ui, s, _, _) = press(
        ui,
        s,
        &[key(KeyCode::Home), key(KeyCode::Down), key(KeyCode::Enter)],
    );
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert_eq!(
        m.choices.iter().map(|c| c.marked).collect::<Vec<_>>(),
        [false, false, false, true],
        "ticks follow the optimistic card"
    );
    let logins: Vec<_> = view(&s).cards[ITEM3]
        .assignees
        .iter()
        .map(|u| u.login.clone())
        .collect();
    assert_eq!(logins, ["zed"]);
    let (ui, _, sent, _) = press(ui, s, &[key(KeyCode::Esc)]);
    assert_eq!(ui.mode, Mode::Board);
    assert!(sent.is_empty());
}

#[test]
fn assign_to_me_is_first_and_toggles() {
    let (ui, s) = assignee_picker();
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert_eq!(
        m.choices.iter().filter(|c| c.label.contains("@me")).count(),
        1,
        "the viewer is not listed twice"
    );
    let (ui, s, sent, _) = press(ui, s, &[key(KeyCode::Enter)]);
    assert!(matches!(&sent[..], [Msg::ToggleAssignee { user, .. }] if user.login == "me"));
    assert!(view(&s).cards[ITEM3].is_assigned("u-me"));
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert!(m.choices[0].marked, "the tick follows the optimistic card");
    let (ui, s, _, _) = press(ui, s, &[key(KeyCode::Enter)]);
    assert!(!view(&s).cards[ITEM3].is_assigned("u-me"), "and back off");
    assert!(!menu(&s, picker_mode(&ui)).expect("a menu").choices[0].marked);
}

#[test]
fn typing_filters_the_assignees_fuzzily() {
    let (ui, s) = assignee_picker();
    let (ui, s, sent, _) = press(ui, s, &[ch('j'), ch('d'), ch('t')]);
    assert!(sent.is_empty());
    assert_eq!(picker_mode(&ui).filter, "jdt");
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert_eq!(m.filter.as_deref(), Some("jdt"));
    assert_eq!(labels(&m), ["@jdetter"]);
    let (_, _, sent, _) = press(ui, s, &[key(KeyCode::Enter)]);
    assert!(matches!(&sent[..], [Msg::ToggleAssignee { user, .. }] if user.login == "jdetter"));
}

#[test]
fn me_finds_the_assign_to_me_row() {
    let (ui, s) = assignee_picker();
    let (ui, s, _, _) = press(ui, s, &[ch('m'), ch('e')]);
    assert_eq!(
        labels(&menu(&s, picker_mode(&ui)).expect("a menu")),
        ["Assign to me (@me)"]
    );
}

#[test]
fn the_filter_matches_names_with_smart_case() {
    let (ui, s) = assignee_picker();
    let typed = |text: &str| {
        let keys: Vec<_> = text.chars().map(ch).collect();
        let (ui, s, _, _) = press(ui.clone(), s.clone(), &keys);
        labels(&menu(&s, picker_mode(&ui)).expect("a menu"))
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    assert_eq!(typed("zedson"), ["@zed (Zed Zedson)"]);
    assert_eq!(typed("Zedson"), ["@zed (Zed Zedson)"]);
    assert!(typed("ZEDSON").is_empty(), "a capital is case-sensitive");
}

#[test]
fn letters_type_in_the_assignee_picker_but_move_elsewhere() {
    let (ui, s) = assignee_picker();
    let (ui, _, sent, _) = press(ui, s, &[ch('j'), ch('k'), ch('g'), ch('G'), ch('q')]);
    assert!(sent.is_empty());
    let p = picker_mode(&ui);
    assert_eq!((p.filter.as_str(), p.cursor), ("jkgGq", 0));
    // The status picker still moves with them.
    let (ui, _, _, _) = press(
        UiState::default(),
        with_assignable(detail_open()),
        &[ch('s'), ch('j')],
    );
    assert_eq!(picker_mode(&ui).cursor, 1);
    assert_eq!(picker_mode(&ui).filter, "");
}

#[test]
fn ctrl_n_and_ctrl_p_move_in_the_assignee_picker() {
    let (ui, s) = assignee_picker();
    let (ui, s, _, _) = press(ui, s, &[ctrl('n'), ctrl('n')]);
    assert_eq!(picker_mode(&ui).cursor, 2);
    let (ui, _, _, _) = press(ui, s, &[ctrl('p')]);
    assert_eq!(picker_mode(&ui).cursor, 1);
}

#[test]
fn changing_the_filter_resets_the_cursor_and_deleting_widens_it() {
    let (ui, s) = assignee_picker();
    let (ui, s, _, _) = press(ui, s, &[key(KeyCode::End)]);
    assert_eq!(picker_mode(&ui).cursor, 3);
    let (ui, s, _, _) = press(ui, s, &[ch('e')]);
    let p = picker_mode(&ui);
    assert_eq!((p.filter.as_str(), p.cursor), ("e", 0));
    let (ui, s, _, _) = press(ui, s, &[ch('t'), ch('t')]);
    assert_eq!(
        labels(&menu(&s, picker_mode(&ui)).expect("a menu")),
        ["@jdetter"]
    );
    let (ui, s, _, _) = press(ui, s, &[key(KeyCode::Backspace), key(KeyCode::Backspace)]);
    assert_eq!(picker_mode(&ui).filter, "e");
    assert_eq!(
        labels(&menu(&s, picker_mode(&ui)).expect("a menu")),
        ["Assign to me (@me)", "@jdetter", "@zed (Zed Zedson)"]
    );
    let (ui, s, _, _) = press(ui, s, &[ch('x'), ctrl('u')]);
    assert_eq!(picker_mode(&ui).filter, "");
    assert_eq!(menu(&s, picker_mode(&ui)).expect("a menu").choices.len(), 4);
}

#[test]
fn esc_closes_the_assignee_picker_with_a_filter_set() {
    let (ui, s) = assignee_picker();
    let (ui, _, sent, _) = press(ui, s, &[ch('z'), key(KeyCode::Esc)]);
    assert_eq!(ui.mode, Mode::Board);
    assert!(sent.is_empty());
}

#[test]
fn enter_with_no_matches_does_nothing() {
    let (ui, s) = assignee_picker();
    let (ui, _, sent, _) = press(ui, s, &[ch('q'), ch('q'), key(KeyCode::Enter)]);
    assert!(matches!(ui.mode, Mode::Picker(_)));
    assert!(sent.is_empty());
}

/// The standard board's detail (#3, labelled "blog") with its label picker
/// open and the repository's labels loaded.
fn label_picker() -> (UiState, State) {
    let (ui, s, _, _) = press(UiState::default(), with_labels(detail_open()), &[ch('l')]);
    (ui, s)
}

#[test]
fn label_picker_loads_the_repositorys_labels_once() {
    let (ui, s, sent, _) = press(UiState::default(), detail_open(), &[ch('l')]);
    assert_eq!(picker_mode(&ui).kind, PickerKind::Label);
    assert_eq!(
        sent,
        vec![Msg::LoadLabels {
            repo_id: "r-plans".into(),
            repo: "acme/plans".into(),
        }]
    );
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert!(m.loading);
    assert!(m.choices.is_empty());
    let s = with_labels(s);
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert!(!m.loading);
    assert_eq!(labels(&m), ["blog", "Bug", "docs", "good first issue"]);

    let (_, _, sent, _) = press(UiState::default(), s, &[ch('l')]);
    assert!(sent.is_empty(), "already loaded");
}

#[test]
fn label_picks_toggle_and_the_picker_stays_open() {
    let (ui, s) = label_picker();
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert_eq!(
        m.choices.iter().map(|c| c.marked).collect::<Vec<_>>(),
        [true, false, false, false],
        "the card's labels are ticked"
    );
    assert!(m.stays_open);
    // Take "blog" off, put "docs" on.
    let (ui, s, sent, _) = press(ui, s, &[key(KeyCode::Enter)]);
    assert_eq!(
        sent,
        vec![Msg::ToggleLabel {
            item_id: ITEM3.into(),
            label: label("blog"),
        }]
    );
    let (ui, s, _, _) = press(
        ui,
        s,
        &[key(KeyCode::Down), key(KeyCode::Down), key(KeyCode::Enter)],
    );
    assert_eq!(picker_mode(&ui).cursor, 2, "still open, same place");
    let m = menu(&s, picker_mode(&ui)).expect("a menu");
    assert_eq!(
        m.choices.iter().map(|c| c.marked).collect::<Vec<_>>(),
        [false, false, true, false],
        "ticks follow the optimistic card"
    );
    assert_eq!(view(&s).cards[ITEM3].labels, [label("docs")]);
    let (ui, _, sent, _) = press(ui, s, &[key(KeyCode::Esc)]);
    assert_eq!(ui.mode, Mode::Board);
    assert!(sent.is_empty());
}

#[test]
fn typing_filters_the_labels_fuzzily() {
    let (ui, s) = label_picker();
    let (ui, s, sent, _) = press(ui, s, &[ch('g'), ch('f'), ch('i')]);
    assert!(sent.is_empty(), "letters type");
    assert_eq!(picker_mode(&ui).filter, "gfi");
    assert_eq!(
        labels(&menu(&s, picker_mode(&ui)).expect("a menu")),
        ["good first issue"]
    );
    let (_, _, sent, _) = press(ui, s, &[key(KeyCode::Enter)]);
    assert_eq!(
        sent,
        vec![Msg::ToggleLabel {
            item_id: ITEM3.into(),
            label: label("good first issue"),
        }]
    );
    // Smart case.
    let (ui, s) = label_picker();
    let (ui, s, _, _) = press(ui, s, &[ch('B')]);
    assert_eq!(
        labels(&menu(&s, picker_mode(&ui)).expect("a menu")),
        ["Bug"]
    );
}

#[test]
fn a_picker_closes_if_its_card_goes() {
    let s = with_assignable(detail_open());
    let ui = UiState {
        mode: Mode::Picker(Picker {
            kind: PickerKind::Status,
            item_id: "gone".into(),
            cursor: 0,
            filter: String::new(),
        }),
        ..UiState::default()
    };
    let (ui, out) = handle_key(&ui, &s, key(KeyCode::Enter));
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(out, Outcome::default());
}

// The quit guard --------------------------------------------------------------

fn sending() -> State {
    step(
        loaded(),
        Msg::SetPriority {
            item_id: "item1".into(),
            priority_id: Some("p-low".into()),
        },
    )
}

#[test]
fn quitting_with_nothing_pending_is_immediate() {
    let (_, out) = handle_key(&UiState::default(), &loaded(), ch('q'));
    assert_eq!(out, actions(vec![Action::Quit]));
}

#[test]
fn quitting_while_changes_are_sending_asks_first() {
    for k in [ch('q'), ctrl('c')] {
        let (ui, out) = handle_key(&UiState::default(), &sending(), k);
        assert_eq!(ui.mode, Mode::ConfirmQuit { pending: 1 });
        assert_eq!(out, Outcome::default());
        let (_, out) = handle_key(&ui, &sending(), ch('y'));
        assert_eq!(out, actions(vec![Action::Quit]));
        let (ui, out) = handle_key(&ui, &sending(), ch('n'));
        assert_eq!(ui.mode, Mode::Board);
        assert_eq!(out, Outcome::default());
    }
}

#[test]
fn any_other_answer_stays() {
    let (ui, _) = handle_key(&UiState::default(), &sending(), ch('q'));
    let (ui, out) = handle_key(&ui, &sending(), key(KeyCode::Esc));
    assert_eq!(ui.mode, Mode::Board);
    assert_eq!(out, Outcome::default());
}

#[test]
fn failed_changes_do_not_hold_up_quitting() {
    let (s, _) = with_failed_op();
    let (_, out) = handle_key(&UiState::default(), &s, ch('q'));
    assert_eq!(out, actions(vec![Action::Quit]));
}

#[test]
fn ctrl_c_in_the_detail_is_guarded_too() {
    let s = step(
        detail_open(),
        Msg::AssignMe {
            item_id: ITEM3.into(),
        },
    );
    let (ui, _) = handle_key(&UiState::default(), &s, ctrl('c'));
    assert_eq!(ui.mode, Mode::ConfirmQuit { pending: 1 });
}

// Rendering -------------------------------------------------------------------

#[test]
fn renders_detail_loading() {
    assert_snapshot!(draw(&detail_loading(), &UiState::default()).backend());
}

#[test]
fn renders_detail_loaded() {
    let t = draw(&detail_open(), &UiState::default());
    assert_snapshot!(t.backend());
    let buf = t.backend().buffer();
    assert!(find_row(&t, "#3 Card 3").is_some(), "the heading");
    let r = gh_board::ui::detail::rect(area());
    assert_eq!(buf[(r.x, r.y)].symbol(), "╭");
    assert_eq!(buf[(r.x, r.y)].fg, theme::CARD_FOCUS.fg.unwrap());
    assert!(row_text(&t, r.y).contains(" #3 "), "titled with the number");
}

#[test]
fn detail_failed_to_load() {
    let s = detail_loading();
    let seq = s.detail.as_ref().map_or(0, |d| d.seq);
    let s = step(
        s,
        Msg::DetailFailed {
            seq,
            err: "Could not resolve to a node".into(),
        },
    );
    assert_snapshot!(draw(&s, &UiState::default()).backend());
}

#[test]
fn detail_long_body_scrolled() {
    let (ui, s, _, _) = press(UiState::default(), long_detail(), &[ctrl('d')]);
    let t = draw(&s, &ui);
    assert_snapshot!(t.backend());
    assert!(find_row(&t, "Line 1 of").is_none(), "scrolled past the top");
}

#[test]
fn detail_shows_pending_and_failed_changes_live() {
    // A pending priority, an unsent comment and an unsaved description.
    let s = step(
        detail_open(),
        Msg::SetPriority {
            item_id: ITEM3.into(),
            priority_id: Some("p-high".into()),
        },
    );
    let s = step(
        s,
        Msg::Comment {
            item_id: ITEM3.into(),
            body: "On it.".into(),
        },
    );
    let (s, fx) = update(
        s,
        Msg::SetBody {
            item_id: ITEM3.into(),
            body: "Draft the post, then ship it.".into(),
        },
    );
    assert!(fx.is_empty(), "queued behind the priority change: {fx:?}");
    let t = draw(&s, &UiState::default());
    assert_snapshot!(t.backend());
    let y = find_row(&t, "High").expect("the priority field");
    let x = row_text(&t, y)
        .find("High")
        .map(|b| row_text(&t, y)[..b].chars().count());
    let buf = t.backend().buffer();
    assert_eq!(
        buf[(x.expect("High") as u16, y)].fg,
        theme::priority(1).fg.unwrap()
    );
    let y = find_row(&t, "~ changes pending").expect("pending mark");
    let x = row_text(&t, y).chars().position(|c| c == '~').expect("~") as u16;
    assert_eq!(buf[(x, y)].fg, theme::PENDING.fg.unwrap());
    assert!(
        s.ops
            .iter()
            .any(|o| matches!(o.kind, OpKind::Comment { .. }))
    );
}

#[test]
fn picker_over_the_detail() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('s'), ch('j')]);
    let t = draw(&s, &ui);
    assert_snapshot!(t.backend());
    let y = find_row(&t, "│   Blocked").expect("the cursor row");
    let x = row_text(&t, y)
        .find("Blocked")
        .map(|b| row_text(&t, y)[..b].chars().count());
    assert_eq!(
        t.backend().buffer()[(x.expect("Blocked") as u16, y)].bg,
        theme::HEADER_FOCUS.bg.unwrap(),
        "the cursor row is highlighted"
    );
}

#[test]
fn assignee_picker_loading_then_loaded() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('a')]);
    assert!(find_row(&draw(&s, &ui), "loading…").is_some());
    let s = with_assignable(s);
    assert_snapshot!(draw(&s, &ui).backend());
}

#[test]
fn assignee_picker_filtered() {
    let (ui, s) = assignee_picker();
    let (ui, s, _, _) = press(ui, s, &[ch('j'), ch('d'), ch('t')]);
    let t = draw(&s, &ui);
    assert!(find_row(&t, "› jdt").is_some());
    assert_snapshot!(t.backend());
}

#[test]
fn assignee_picker_with_no_matches() {
    let (ui, s) = assignee_picker();
    let (ui, s, _, _) = press(ui, s, &[ch('q'), ch('q')]);
    let t = draw(&s, &ui);
    assert!(find_row(&t, "(no matches)").is_some());
    assert_snapshot!(t.backend());
}

#[test]
fn label_picker_loading_then_loaded() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('l')]);
    assert!(find_row(&draw(&s, &ui), "loading…").is_some());
    let s = with_labels(s);
    assert_snapshot!(draw(&s, &ui).backend());
}

#[test]
fn label_picker_filtered() {
    let (ui, s) = label_picker();
    let (ui, s, _, _) = press(ui, s, &[ch('o')]);
    let t = draw(&s, &ui);
    assert!(find_row(&t, "› o").is_some());
    assert_snapshot!(t.backend());
}

#[test]
fn duplicate_picker() {
    let (ui, s, _, _) = press(
        UiState::default(),
        detail_open(),
        &[
            ch('x'),
            ch('G'),
            key(KeyCode::Enter),
            ch('c'),
            ch('a'),
            ch('r'),
        ],
    );
    let t = draw(&s, &ui);
    assert!(find_row(&t, "› car").is_some());
    assert_snapshot!(draw(&s, &ui).backend());
}

#[test]
fn quit_prompt() {
    let (ui, _) = handle_key(&UiState::default(), &sending(), ch('q'));
    let t = draw(&sending(), &ui);
    assert_eq!(
        row_text(&t, H - 2).trim_end(),
        "1 change is still being sent. Quit anyway? (y/n)"
    );
}

#[test]
fn detail_help() {
    let (ui, s, _, _) = press(UiState::default(), detail_open(), &[ch('?')]);
    assert_snapshot!(draw(&s, &ui).backend());
}
