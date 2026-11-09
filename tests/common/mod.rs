//! Synthetic board data. No real project data in fixtures.
#![allow(dead_code)]

pub mod gh;

use gh_board::model::{Op, State, board};
use gh_board::types::{Board, Card, Cards, Config, Field, FieldOption, IssueState, Meta, User};
use gh_board::update::{Effect, Msg, update};

pub fn config() -> Config {
    Config::new("acme", 1)
}

fn field(id: &str, options: &[(&str, &str)]) -> Field {
    Field {
        id: id.into(),
        options: options
            .iter()
            .map(|(id, name)| FieldOption {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
    }
}

pub fn user(id: &str, login: &str) -> User {
    User {
        id: id.into(),
        login: login.into(),
        name: None,
    }
}

pub fn me() -> User {
    user("u-me", "me")
}

pub const STATUSES: [&str; 5] = ["s-todo", "s-blocked", "s-prog", "s-ready", "s-done"];
pub const PRIORITIES: [&str; 4] = ["p-urgent", "p-high", "p-medium", "p-low"];

pub fn meta() -> Meta {
    Meta {
        viewer: me(),
        project_id: "proj".into(),
        title: "Plans".into(),
        status_field: field(
            "f-status",
            &[
                ("s-todo", "Todo"),
                ("s-blocked", "Blocked"),
                ("s-prog", "In Progress"),
                ("s-ready", "Ready"),
                ("s-done", "Done"),
            ],
        ),
        priority_field: Some(field(
            "f-priority",
            &[
                ("p-urgent", "Urgent"),
                ("p-high", "High"),
                ("p-medium", "Medium"),
                ("p-low", "Low"),
            ],
        )),
    }
}

/// Card `n`: item `item<n>`, issue `issue<n>`, in Todo, unassigned.
pub fn card(n: u64) -> Card {
    Card {
        item_id: format!("item{n}"),
        issue_id: Some(format!("issue{n}")),
        number: Some(n),
        title: format!("Card {n}"),
        url: Some(format!("https://github.com/acme/plans/issues/{n}")),
        state: Some(IssueState::Open),
        repo_id: Some("r-plans".into()),
        repo: Some("acme/plans".into()),
        status_id: Some("s-todo".into()),
        ..Card::default()
    }
}

pub fn cards(list: impl IntoIterator<Item = Card>) -> Cards {
    let mut out = Cards::default();
    for c in list {
        out.put(c);
    }
    out
}

/// The standard board: Todo has 1 (unassigned), 2 (@zed), 3 (@amy); 4 is in
/// progress; 5 is done.
pub fn items() -> Cards {
    cards([
        card(1),
        Card {
            assignees: vec![user("u-zed", "zed")],
            priority_id: Some("p-high".into()),
            ..card(2)
        },
        Card {
            assignees: vec![user("u-amy", "amy")],
            labels: vec!["blog".into()],
            ..card(3)
        },
        Card {
            status_id: Some("s-prog".into()),
            priority_id: Some("p-urgent".into()),
            ..card(4)
        },
        Card {
            status_id: Some("s-done".into()),
            ..card(5)
        },
    ])
}

pub fn loaded_with(config: Config, cards: Cards) -> State {
    let s = step(State::new(config), Msg::Refresh);
    let seq = s.seq;
    step(
        s,
        Msg::BoardLoaded {
            seq,
            board: Board {
                meta: meta(),
                cards,
            },
        },
    )
}

pub fn loaded() -> State {
    loaded_with(config(), items())
}

/// Update, discarding effects.
pub fn step(s: State, msg: Msg) -> State {
    update(s, msg).0
}

/// Column name -> card numbers, top to bottom.
pub fn shape(s: &State) -> Vec<(String, Vec<u64>)> {
    board(s)
        .columns
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                c.cards.iter().filter_map(|c| c.number).collect(),
            )
        })
        .collect()
}

pub fn mutations(fx: &[Effect]) -> Vec<&Op> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Mutate { op, .. } => Some(op),
            _ => None,
        })
        .collect()
}

pub fn notifications(fx: &[Effect]) -> Vec<&str> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Notify { message, .. } => Some(message.as_str()),
            _ => None,
        })
        .collect()
}

pub fn focused_number(s: &State) -> Option<u64> {
    board(s).focused().and_then(|c| c.number)
}

/// The standard board after a priority change to #2 failed: the state, and
/// the op the reducer asks the UI to prompt about.
pub fn with_failed_op() -> (State, Op) {
    let (s, fx) = update(
        loaded(),
        Msg::SetPriority {
            item_id: "item2".into(),
            priority_id: Some("p-low".into()),
        },
    );
    let op_id = mutations(&fx)[0].id;
    let (s, fx) = update(
        s,
        Msg::OpFailed {
            op_id,
            err: "rate limited".into(),
        },
    );
    let op = fx
        .into_iter()
        .find_map(|e| match e {
            Effect::PromptFailure { op } => Some(op),
            _ => None,
        })
        .expect("a failure prompt");
    (s, op)
}

pub fn comment(n: u64, author: &str, body: &str) -> gh_board::types::Comment {
    gh_board::types::Comment {
        id: format!("c{n}"),
        author: author.into(),
        body: body.into(),
        created_at: format!("2025-01-{:02}T10:00:00Z", n.min(28)),
    }
}

/// The standard board with #3's detail open but not yet loaded.
pub fn detail_loading() -> State {
    let s = step(loaded(), Msg::Focus(gh_board::update::Direction::Down));
    step(s, Msg::OpenDetail { item_id: None })
}

/// #3's detail, loaded with this body and comments (`total` may be more).
pub fn detail_with(body: &str, comments: Vec<gh_board::types::Comment>, total: u64) -> State {
    let s = detail_loading();
    let seq = s.detail.as_ref().map_or(0, |d| d.seq);
    step(
        s,
        Msg::DetailLoaded {
            seq,
            data: gh_board::types::DetailData {
                body: body.into(),
                comments,
                total,
            },
        },
    )
}

/// #3's detail with a short description and two comments.
pub fn detail_open() -> State {
    detail_with(
        "Draft the post.\n\nThen ask for review.",
        vec![
            comment(1, "zed", "I can review it."),
            comment(2, "amy", "Thanks!"),
        ],
        5,
    )
}

/// Assignable users for the standard board's repo.
pub fn with_assignable(s: State) -> State {
    step(
        s,
        Msg::AssignableLoaded {
            repo_id: "r-plans".into(),
            users: vec![
                user("u-amy", "amy"),
                User {
                    name: Some("Zed Zedson".into()),
                    ..user("u-zed", "zed")
                },
                me(),
            ],
        },
    )
}
