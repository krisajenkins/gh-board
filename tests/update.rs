//! The reducer, as data. Most behaviour lives here.

mod common;

use common::*;
use gh_board::model::{Filter, OpKind, OpState, State, view};
use gh_board::types::{Board, Card, Comment, DetailData, IssueState, StateReason};
use gh_board::update::{Direction, Effect, Msg, MutationResult, Towards, update};

fn row(name: &str, cards: &[u64]) -> (String, Vec<u64>) {
    (name.into(), cards.to_vec())
}

fn get(s: &State, n: u64) -> Card {
    view(s).cards[&format!("item{n}")].clone()
}

fn priority(n: u64, p: Option<&str>) -> Msg {
    Msg::SetPriority {
        item_id: format!("item{n}"),
        priority_id: p.map(String::from),
    }
}

fn ok_card(op_id: u64, card: Card) -> Msg {
    Msg::OpOk {
        op_id,
        result: Ok(MutationResult::Card(card)),
    }
}

// Loading ---------------------------------------------------------------------

#[test]
fn refresh_asks_for_the_board_and_marks_the_state_loading() {
    let (s, fx) = update(State::new(config()), Msg::Refresh);
    assert!(s.loading);
    assert_eq!(fx, vec![Effect::FetchBoard { seq: s.seq }]);
}

#[test]
fn a_loaded_board_is_shown_in_status_columns_done_hidden() {
    let s = loaded();
    assert!(!s.loading);
    assert_eq!(
        shape(&s),
        vec![
            row("Todo", &[1, 3, 2]),
            row("Blocked", &[]),
            row("In Progress", &[4]),
            row("Ready", &[])
        ]
    );
    let s = step(s, Msg::ToggleHidden);
    assert_eq!(shape(&s).last(), Some(&row("Done", &[5])));
}

#[test]
fn cards_with_an_unknown_status_go_in_a_leading_no_status_column() {
    let s = loaded_with(
        config(),
        cards([
            Card {
                status_id: None,
                ..card(1)
            },
            card(2),
        ]),
    );
    assert_eq!(shape(&s)[0], row("No Status", &[1]));
}

#[test]
fn loading_prefetches_each_repositorys_assignable_users_once() {
    let s = step(State::new(config()), Msg::Refresh);
    let seq = s.seq;
    let (_, fx) = update(
        s,
        Msg::BoardLoaded {
            seq,
            board: Board {
                meta: meta(),
                cards: items(),
            },
        },
    );
    let fetches: Vec<_> = fx
        .iter()
        .filter(|e| matches!(e, Effect::FetchAssignable { .. }))
        .collect();
    assert_eq!(
        fetches,
        vec![&Effect::FetchAssignable {
            repo_id: "r-plans".into(),
            repo: "acme/plans".into()
        }]
    );
}

#[test]
fn a_stale_board_response_is_ignored() {
    let s = step(State::new(config()), Msg::Refresh);
    let stale = s.seq;
    let s = step(s, Msg::Refresh);
    let s = step(
        s,
        Msg::BoardLoaded {
            seq: stale,
            board: Board {
                meta: meta(),
                cards: items(),
            },
        },
    );
    assert!(s.meta.is_none());
    assert!(s.loading);
}

#[test]
fn the_configured_priority_becomes_the_starting_filter() {
    let mut config = config();
    config.priority = Some("high".into());
    let s = loaded_with(config, items());
    assert_eq!(s.filter, Filter::Priority("p-high".into()));
    assert_eq!(shape(&s)[0], row("Todo", &[2]));
}

#[test]
fn an_unknown_configured_priority_warns_and_shows_all() {
    let mut config = config();
    config.priority = Some("Whenever".into());
    let s = step(State::new(config), Msg::Refresh);
    let seq = s.seq;
    let (s, fx) = update(
        s,
        Msg::BoardLoaded {
            seq,
            board: Board {
                meta: meta(),
                cards: items(),
            },
        },
    );
    assert_eq!(s.filter, Filter::All);
    assert_eq!(
        notifications(&fx),
        vec!["No priority named \"Whenever\": showing all"]
    );
}

// View ------------------------------------------------------------------------

#[test]
fn number_keys_filter_by_priority_in_field_order() {
    let s = loaded();
    let s = step(s, Msg::FilterKey(1));
    assert_eq!(s.filter, Filter::Priority("p-urgent".into()));
    let s = step(s, Msg::FilterKey(5));
    assert_eq!(s.filter, Filter::NoPriority);
    assert_eq!(shape(&s)[0], row("Todo", &[1, 3]));
    let s = step(s, Msg::FilterKey(6));
    assert_eq!(s.filter, Filter::NoPriority, "out of range is ignored");
    let s = step(s, Msg::FilterKey(0));
    assert_eq!(s.filter, Filter::All);
}

#[test]
fn focus_moves_between_cards_and_remembers_the_wanted_row() {
    let s = loaded();
    assert_eq!(focused_number(&s), Some(1));
    let s = step(s, Msg::Focus(Direction::Down));
    let s = step(s, Msg::Focus(Direction::Down));
    assert_eq!(focused_number(&s), Some(2));
    let s = step(s, Msg::Focus(Direction::Down));
    assert_eq!(focused_number(&s), Some(2), "stops at the bottom");
    let s = step(s, Msg::Focus(Direction::Right)); // Blocked: empty
    assert_eq!(focused_number(&s), None);
    let s = step(s, Msg::Focus(Direction::Left));
    assert_eq!(focused_number(&s), Some(2), "back to the wanted row");
    let s = step(s, Msg::Focus(Direction::Top));
    assert_eq!(focused_number(&s), Some(1));
}

#[test]
fn search_narrows_the_board_and_focus_jumps_to_the_first_match() {
    let s = loaded();
    let s = step(s, Msg::Focus(Direction::Right));
    let s = step(s, Msg::Search("urgent".into()));
    assert_eq!(
        shape(&s),
        vec![
            row("Todo", &[]),
            row("Blocked", &[]),
            row("In Progress", &[4]),
            row("Ready", &[])
        ]
    );
    assert_eq!(focused_number(&s), Some(4));
    let s = step(s, Msg::Search("@amy blog".into()));
    assert_eq!(shape(&s)[0], row("Todo", &[3]));
}

// Operations ------------------------------------------------------------------

#[test]
fn a_change_shows_at_once_and_goes_to_github() {
    let (s, fx) = update(loaded(), priority(1, Some("p-low")));
    let c = get(&s, 1);
    assert_eq!(c.priority_id.as_deref(), Some("p-low"));
    assert!(c.pending);
    let ops = mutations(&fx);
    assert_eq!(ops.len(), 1);
    assert_eq!(
        ops[0].kind,
        OpKind::Priority {
            priority_id: Some("p-low".into())
        }
    );
    assert_eq!(ops[0].desc, "Set #1 priority to Low");
}

#[test]
fn a_no_op_change_is_not_sent() {
    let (s, fx) = update(loaded(), priority(2, Some("p-high")));
    assert!(s.ops.is_empty());
    assert!(fx.is_empty());
}

#[test]
fn confirming_merges_the_server_card_into_base() {
    let (s, fx) = update(loaded(), priority(1, Some("p-low")));
    let op_id = mutations(&fx)[0].id;
    // A priority response carries issue fields only: no item id or status.
    let server = Card {
        item_id: String::new(),
        status_id: None,
        priority_id: Some("p-low".into()),
        ..card(1)
    };
    let s = step(s, ok_card(op_id, server));
    assert!(s.ops.is_empty());
    let c = get(&s, 1);
    assert_eq!(c.priority_id.as_deref(), Some("p-low"));
    assert_eq!(c.status_id.as_deref(), Some("s-todo"));
    assert_eq!(c.item_id, "item1");
    assert!(!c.pending);
}

#[test]
fn changes_to_one_card_are_sent_one_at_a_time() {
    let (s, fx1) = update(loaded(), priority(1, Some("p-low")));
    let (s, fx2) = update(s, priority(1, Some("p-high")));
    let (s, fx3) = update(s, priority(2, Some("p-low")));
    assert_eq!(mutations(&fx1).len(), 1);
    assert!(mutations(&fx2).is_empty(), "queued behind the first");
    assert_eq!(mutations(&fx3).len(), 1, "another card goes out at once");
    assert_eq!(
        get(&s, 1).priority_id.as_deref(),
        Some("p-high"),
        "the view shows the latest intent"
    );

    let first = mutations(&fx1)[0].id;
    let (_, fx) = update(
        s,
        ok_card(
            first,
            Card {
                priority_id: Some("p-low".into()),
                ..card(1)
            },
        ),
    );
    let sent = mutations(&fx);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].kind,
        OpKind::Priority {
            priority_id: Some("p-high".into())
        }
    );
}

#[test]
fn a_failure_asks_and_abandoning_rolls_back() {
    let (s, fx) = update(loaded(), priority(1, Some("p-low")));
    let op_id = mutations(&fx)[0].id;
    let (s, fx) = update(
        s,
        Msg::OpFailed {
            op_id,
            err: "nope".into(),
        },
    );
    assert!(matches!(&fx[..], [Effect::PromptFailure { op }] if op.id == op_id));
    let c = get(&s, 1);
    assert!(c.failed);
    assert_eq!(
        c.priority_id.as_deref(),
        Some("p-low"),
        "still shown until abandoned"
    );

    let s = step(s, Msg::OpAbandon { op_id });
    assert!(s.ops.is_empty());
    assert_eq!(get(&s, 1), items().cards["item1"]);
}

#[test]
fn retrying_resends_with_a_fresh_id_as_the_latest_intent() {
    let (s, fx) = update(loaded(), priority(1, Some("p-low")));
    let op_id = mutations(&fx)[0].id;
    let s = step(
        s,
        Msg::OpFailed {
            op_id,
            err: "nope".into(),
        },
    );
    let (s, fx) = update(s, Msg::OpRetry { op_id });
    let sent = mutations(&fx);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].id > op_id);
    assert_eq!(s.ops.len(), 1);
    assert_eq!(s.ops[0].state, OpState::Inflight);
    assert_eq!(s.prompting, None);
}

#[test]
fn a_success_supersedes_older_failures_of_the_same_kind() {
    let (s, fx) = update(loaded(), priority(1, Some("p-low")));
    let first = mutations(&fx)[0].id;
    let (s, fx) = update(
        s,
        Msg::OpFailed {
            op_id: first,
            err: "nope".into(),
        },
    );
    assert!(mutations(&fx).is_empty());
    let (s, fx) = update(s, priority(1, Some("p-high")));
    let second = mutations(&fx)[0].id;
    let s = step(
        s,
        ok_card(
            second,
            Card {
                priority_id: Some("p-high".into()),
                ..card(1)
            },
        ),
    );
    assert!(s.ops.is_empty());
}

#[test]
fn an_abandoned_comment_is_saved_rather_than_lost() {
    let (s, fx) = update(
        loaded(),
        Msg::Comment {
            item_id: "item1".into(),
            body: "  hello  ".into(),
        },
    );
    let op_id = mutations(&fx)[0].id;
    let s = step(
        s,
        Msg::OpFailed {
            op_id,
            err: "nope".into(),
        },
    );
    let (_, fx) = update(s, Msg::OpAbandon { op_id });
    assert!(fx.contains(&Effect::SaveText {
        text: "hello".into(),
        what: "Abandoned comment".into()
    }));
}

#[test]
fn an_empty_comment_is_not_posted() {
    let (s, fx) = update(
        loaded(),
        Msg::Comment {
            item_id: "item1".into(),
            body: " \n ".into(),
        },
    );
    assert!(s.ops.is_empty());
    assert_eq!(notifications(&fx), vec!["Empty comment not posted"]);
}

#[test]
fn a_confirmed_comment_is_appended_to_the_open_detail() {
    let (s, fx) = update(loaded(), Msg::OpenDetail { item_id: None });
    let Effect::FetchDetail { seq, .. } = fx[0] else {
        panic!("{fx:?}")
    };
    let s = step(
        s,
        Msg::DetailLoaded {
            seq,
            data: DetailData {
                body: "b".into(),
                comments: vec![],
                total: 0,
            },
        },
    );
    let (s, fx) = update(
        s,
        Msg::Comment {
            item_id: "item1".into(),
            body: "hi".into(),
        },
    );
    let op_id = mutations(&fx)[0].id;
    let comment = Comment {
        id: "c1".into(),
        author: "me".into(),
        body: "hi".into(),
        created_at: "now".into(),
    };
    let s = step(
        s,
        Msg::OpOk {
            op_id,
            result: Ok(MutationResult::Comment(comment.clone())),
        },
    );
    let data = s.detail.unwrap().data.unwrap();
    assert_eq!(data.comments, vec![comment]);
    assert_eq!(data.total, 1);
}

#[test]
fn an_unchanged_description_is_not_sent() {
    let (s, fx) = update(
        loaded(),
        Msg::OpenDetail {
            item_id: Some("item1".into()),
        },
    );
    let Effect::FetchDetail { seq, .. } = fx[0] else {
        panic!("{fx:?}")
    };
    let s = step(
        s,
        Msg::DetailLoaded {
            seq,
            data: DetailData {
                body: "text".into(),
                comments: vec![],
                total: 0,
            },
        },
    );
    let (s, fx) = update(
        s,
        Msg::SetBody {
            item_id: "item1".into(),
            body: "text\r\n\n".into(),
        },
    );
    assert!(s.ops.is_empty());
    assert_eq!(notifications(&fx), vec!["Description unchanged"]);
}

#[test]
fn moving_a_card_skips_the_no_status_column() {
    let s = loaded_with(
        config(),
        cards([
            Card {
                status_id: None,
                ..card(9)
            },
            card(1),
        ]),
    );
    let s = step(s, Msg::Focus(Direction::Right)); // Todo, card 1
    assert_eq!(focused_number(&s), Some(1));
    let (s, fx) = update(s, Msg::MoveCard(Towards::Prev));
    assert!(fx.is_empty());
    let (s, fx) = update(s, Msg::MoveCard(Towards::Next));
    assert_eq!(
        mutations(&fx)[0].kind,
        OpKind::Status {
            status_id: "s-blocked".into()
        }
    );
    assert_eq!(focused_number(&s), Some(1), "focus follows the card");
}

#[test]
fn a_confirmed_status_change_rereads_the_card_later() {
    let (s, fx) = update(
        loaded(),
        Msg::SetStatus {
            item_id: "item1".into(),
            status_id: "s-done".into(),
        },
    );
    let op_id = mutations(&fx)[0].id;
    let (_, fx) = update(
        s,
        ok_card(
            op_id,
            Card {
                status_id: Some("s-done".into()),
                ..card(1)
            },
        ),
    );
    assert!(fx.contains(&Effect::FetchItem {
        item_id: "item1".into(),
        delay_ms: Some(4000),
        rev: 1
    }));
}

#[test]
fn closing_defaults_to_completed() {
    let (s, fx) = update(
        loaded(),
        Msg::SetState {
            item_id: "item1".into(),
            issue_state: IssueState::Closed,
            reason: None,
        },
    );
    assert_eq!(mutations(&fx)[0].desc, "Close #1 as completed");
    assert_eq!(get(&s, 1).state_reason, Some(StateReason::Completed));
}

#[test]
fn an_unexpected_response_refreshes() {
    let (s, fx) = update(loaded(), priority(1, Some("p-low")));
    let op_id = mutations(&fx)[0].id;
    let (s, fx) = update(
        s,
        Msg::OpOk {
            op_id,
            result: Err("no issue in response".into()),
        },
    );
    assert!(s.loading);
    assert!(fx.contains(&Effect::FetchBoard { seq: s.seq }));
}

// Ordering of server truths ---------------------------------------------------

#[test]
fn a_change_confirmed_during_a_refresh_survives_it() {
    let (s, fx) = update(loaded(), priority(1, Some("p-low")));
    let op_id = mutations(&fx)[0].id;
    let s = step(s, Msg::Refresh);
    let seq = s.seq;
    let s = step(
        s,
        ok_card(
            op_id,
            Card {
                priority_id: Some("p-low".into()),
                ..card(1)
            },
        ),
    );
    // The board's pages were read before the change landed.
    let s = step(
        s,
        Msg::BoardLoaded {
            seq,
            board: Board {
                meta: meta(),
                cards: items(),
            },
        },
    );
    assert_eq!(get(&s, 1).priority_id.as_deref(), Some("p-low"));
}

#[test]
fn an_item_read_older_than_a_confirmation_is_read_again() {
    let s = loaded();
    let rev = s.confirms;
    let (s, fx) = update(s, priority(1, Some("p-low")));
    let op_id = mutations(&fx)[0].id;
    let s = step(
        s,
        ok_card(
            op_id,
            Card {
                priority_id: Some("p-low".into()),
                ..card(1)
            },
        ),
    );
    let (s, fx) = update(s, Msg::ItemLoaded { rev, card: card(1) });
    assert_eq!(get(&s, 1).priority_id.as_deref(), Some("p-low"));
    assert_eq!(
        fx,
        vec![Effect::FetchItem {
            item_id: "item1".into(),
            delay_ms: None,
            rev: s.confirms
        }]
    );
}

// Create ----------------------------------------------------------------------

#[test]
fn a_new_issue_appears_at_once_in_the_focused_column_and_takes_its_real_id_on_confirm() {
    let s = step(loaded(), Msg::FilterKey(2)); // High
    let s = step(s, Msg::Focus(Direction::Right));
    let s = step(s, Msg::Focus(Direction::Right)); // In Progress
    let (s, fx) = update(
        s,
        Msg::Create {
            title: " Write it ".into(),
        },
    );
    let op = mutations(&fx)[0].clone();
    let OpKind::Create {
        title,
        status_id,
        priority_id,
        repo_id,
        ..
    } = &op.kind
    else {
        panic!("{op:?}")
    };
    assert_eq!(title, "Write it");
    assert_eq!(status_id.as_deref(), Some("s-prog"));
    assert_eq!(priority_id.as_deref(), Some("p-high"));
    assert_eq!(repo_id, "r-plans");
    let temp = board_focused_id(&s);
    assert_eq!(temp, op.item_id);
    assert!(get_id(&s, &temp).temp);

    // Acting on a card that is still being created is refused.
    let (_, fx) = update(
        s.clone(),
        Msg::AssignMe {
            item_id: temp.clone(),
        },
    );
    assert_eq!(notifications(&fx), vec!["That card is still being created"]);

    let real = Card {
        title: "Write it".into(),
        status_id: Some("s-prog".into()),
        priority_id: Some("p-high".into()),
        ..card(99)
    };
    let s = step(s, ok_card(op.id, real));
    assert_eq!(board_focused_id(&s), "item99");
    assert!(!view(&s).cards.contains_key(&temp));
}

#[test]
fn abandoning_a_half_created_issue_warns_with_its_link() {
    let (s, fx) = update(loaded(), Msg::Create { title: "T".into() });
    let op_id = mutations(&fx)[0].id;
    let s = step(
        s,
        Msg::OpProgress {
            op_id,
            created_issue_id: "issue77".into(),
            created_issue_url: Some("https://x/77".into()),
        },
    );
    let (s, fx) = update(
        s,
        Msg::OpFailed {
            op_id,
            err: "add failed".into(),
        },
    );
    let Effect::PromptFailure { op } = &fx[0] else {
        panic!("{fx:?}")
    };
    assert!(
        matches!(&op.kind, OpKind::Create { created_issue_id: Some(id), .. } if id == "issue77")
    );
    let (_, fx) = update(s, Msg::OpAbandon { op_id });
    assert_eq!(
        notifications(&fx),
        vec!["“T” was already created on GitHub; abandoning does not delete it: https://x/77"]
    );
}

fn board_focused_id(s: &State) -> String {
    gh_board::model::board(s).focused().unwrap().item_id.clone()
}

fn get_id(s: &State, id: &str) -> Card {
    view(s).cards[id].clone()
}
