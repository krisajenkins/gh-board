//! Property tests: the reducer driven by random users against a simulated
//! GitHub that answers requests in any order, and sometimes fails them.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use gh_board::model::{OpKind, OpState, State, board, view};
use gh_board::types::{Card, ItemId, User};
use gh_board::update::{Direction, Effect, Msg, MutationResult, Towards, update};
use proptest::prelude::*;

const CARDS: u64 = 4;

fn users() -> Vec<User> {
    vec![user("u-amy", "amy"), user("u-bob", "bob"), me()]
}

#[derive(Clone, Debug)]
enum Action {
    SetPriority(u64, Option<usize>),
    SetStatus(u64, usize),
    ToggleAssignee(u64, usize),
    Focus(Direction),
    FilterKey(usize),
    Move(Towards),
    /// GitHub answers the `pick`th outstanding request.
    Respond {
        pick: usize,
        ok: bool,
    },
    Retry(usize),
    Abandon(usize),
}

fn intent() -> impl Strategy<Value = Action> {
    let card = 1..=CARDS;
    prop_oneof![
        (card.clone(), prop::option::of(0..PRIORITIES.len()))
            .prop_map(|(c, p)| Action::SetPriority(c, p)),
        (card.clone(), 0..STATUSES.len()).prop_map(|(c, s)| Action::SetStatus(c, s)),
        (card, 0..3usize).prop_map(|(c, u)| Action::ToggleAssignee(c, u)),
    ]
}

fn browse() -> impl Strategy<Value = Action> {
    prop_oneof![
        prop_oneof![
            Just(Direction::Left),
            Just(Direction::Right),
            Just(Direction::Up),
            Just(Direction::Down),
            Just(Direction::Top),
            Just(Direction::Bottom),
        ]
        .prop_map(Action::Focus),
        (0..=6usize).prop_map(Action::FilterKey),
    ]
}

fn respond(fail_rate: f64) -> impl Strategy<Value = Action> {
    (any::<usize>(), prop::bool::weighted(1.0 - fail_rate))
        .prop_map(|(pick, ok)| Action::Respond { pick, ok })
}

/// Anything a user or GitHub might do.
fn anything() -> impl Strategy<Value = Action> {
    prop_oneof![
        3 => intent(),
        2 => browse(),
        1 => prop_oneof![Just(Action::Move(Towards::Prev)), Just(Action::Move(Towards::Next))],
        4 => respond(0.3),
        1 => any::<usize>().prop_map(Action::Retry),
        1 => any::<usize>().prop_map(Action::Abandon),
    ]
}

/// Users changing cards while GitHub always succeeds.
fn no_failures() -> impl Strategy<Value = Action> {
    prop_oneof![3 => intent(), 1 => browse(), 3 => respond(0.0)]
}

/// The reducer plus a fake GitHub.
struct Sim {
    s: State,
    /// Requests sent and not yet answered.
    net: Vec<gh_board::model::Op>,
    /// GitHub's copy of each card.
    server: BTreeMap<ItemId, Card>,
    /// The last op id GitHub was sent for each card.
    last_sent: BTreeMap<ItemId, u64>,
}

impl Sim {
    fn new() -> Self {
        let base = cards((1..=CARDS).map(card));
        let server = base.cards.clone();
        Sim {
            s: loaded_with(config(), base),
            net: vec![],
            server,
            last_sent: BTreeMap::new(),
        }
    }

    fn send(&mut self, msg: Msg) -> Result<(), TestCaseError> {
        let (s, fx) = update(self.s.clone(), msg);
        self.s = s;
        for e in fx {
            if let Effect::Mutate { op, .. } = e {
                prop_assert!(
                    !self.net.iter().any(|o| o.item_id == op.item_id),
                    "two requests in flight for {}",
                    op.item_id
                );
                let last = self
                    .last_sent
                    .insert(op.item_id.clone(), op.id)
                    .unwrap_or(0);
                prop_assert!(
                    op.id > last,
                    "{} sent out of order: {} after {last}",
                    op.item_id,
                    op.id
                );
                self.net.push(op);
            }
        }
        self.check()
    }

    /// GitHub answers one outstanding request.
    fn respond(&mut self, pick: usize, ok: bool) -> Result<(), TestCaseError> {
        if self.net.is_empty() {
            return Ok(());
        }
        let op = self.net.remove(pick % self.net.len());
        if !ok {
            return self.send(Msg::OpFailed {
                op_id: op.id,
                err: "boom".into(),
            });
        }
        let card = self
            .server
            .get_mut(&op.item_id)
            .expect("server has the card");
        match &op.kind {
            OpKind::Status { status_id } => card.status_id = Some(status_id.clone()),
            OpKind::Priority { priority_id } => card.priority_id = priority_id.clone(),
            OpKind::Assign { add, remove } => {
                card.assignees.retain(|u| !remove.contains(u));
                for u in add {
                    if !card.assignees.contains(u) {
                        card.assignees.push(u.clone());
                    }
                }
            }
            other => panic!("not simulated: {other:?}"),
        }
        let result = Ok(MutationResult::Card(card.clone()));
        self.send(Msg::OpOk {
            op_id: op.id,
            result,
        })
    }

    fn act(&mut self, action: &Action) -> Result<(), TestCaseError> {
        let item = |n: u64| format!("item{n}");
        match action {
            Action::SetPriority(n, p) => self.send(Msg::SetPriority {
                item_id: item(*n),
                priority_id: p.map(|i| PRIORITIES[i].to_string()),
            }),
            Action::SetStatus(n, i) => self.send(Msg::SetStatus {
                item_id: item(*n),
                status_id: STATUSES[*i].into(),
            }),
            Action::ToggleAssignee(n, u) => self.send(Msg::ToggleAssignee {
                item_id: item(*n),
                user: users()[*u].clone(),
            }),
            Action::Focus(d) => self.send(Msg::Focus(*d)),
            Action::FilterKey(k) => self.send(Msg::FilterKey(*k)),
            Action::Move(t) => self.send(Msg::MoveCard(*t)),
            Action::Respond { pick, ok } => self.respond(*pick, *ok),
            Action::Retry(i) | Action::Abandon(i) => {
                let failed: Vec<u64> = self
                    .s
                    .ops
                    .iter()
                    .filter(|o| o.state == OpState::Failed)
                    .map(|o| o.id)
                    .collect();
                if failed.is_empty() {
                    return Ok(());
                }
                let op_id = failed[i % failed.len()];
                if matches!(action, Action::Retry(_)) {
                    self.send(Msg::OpRetry { op_id })
                } else {
                    self.send(Msg::OpAbandon { op_id })
                }
            }
        }
    }

    /// Invariants that hold after every message.
    fn check(&self) -> Result<(), TestCaseError> {
        let s = &self.s;
        let ids: Vec<u64> = s.ops.iter().map(|o| o.id).collect();
        prop_assert!(
            ids.windows(2).all(|w| w[0] < w[1]),
            "op ids not ascending: {ids:?}"
        );

        let inflight: BTreeSet<u64> = s
            .ops
            .iter()
            .filter(|o| o.state == OpState::Inflight)
            .map(|o| o.id)
            .collect();
        let on_the_wire: BTreeSet<u64> = self.net.iter().map(|o| o.id).collect();
        prop_assert_eq!(
            &inflight,
            &on_the_wire,
            "in-flight ops are exactly the outstanding requests"
        );

        for op in s.ops.iter().filter(|o| o.state == OpState::Queued) {
            prop_assert!(
                s.ops
                    .iter()
                    .any(|o| o.item_id == op.item_id && o.state == OpState::Inflight),
                "op {} queued with nothing in flight ahead of it",
                op.id
            );
        }

        prop_assert_eq!(
            &s.base.cards,
            &self.server,
            "base is GitHub's latest answer"
        );

        let v = board(s);
        if !v.columns.is_empty() {
            prop_assert!(s.focus.col < v.columns.len(), "focus off the board");
        }
        Ok(())
    }

    /// Answer everything, retrying failures, until nothing is outstanding.
    fn settle(&mut self) -> Result<(), TestCaseError> {
        loop {
            if !self.net.is_empty() {
                self.respond(0, true)?;
            } else if let Some(op) = self.s.ops.iter().find(|o| o.state == OpState::Failed) {
                let op_id = op.id;
                self.send(Msg::OpRetry { op_id })?;
            } else {
                return Ok(());
            }
        }
    }
}

/// What a card looks like to the user, minus the pending/failed marks.
fn essence(c: &Card) -> (Option<String>, Option<String>, Vec<String>) {
    (
        c.status_id.clone(),
        c.priority_id.clone(),
        c.assignees.iter().map(|u| u.login.clone()).collect(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Whatever users do and however GitHub answers, ops stay serialised per
    /// card, sent in intent order, and base tracks GitHub.
    #[test]
    fn invariants_hold_under_any_interleaving(actions in prop::collection::vec(anything(), 0..60)) {
        let mut sim = Sim::new();
        for a in &actions {
            sim.act(a)?;
        }
        sim.settle()?;
        prop_assert!(sim.s.ops.is_empty());
        prop_assert!(view(&sim.s).cards.values().all(|c| !c.pending && !c.failed));
    }

    /// Optimism is honest: while GitHub succeeds, the board always shows
    /// exactly what the user asked for, before and after every answer.
    #[test]
    fn the_view_is_always_the_users_intent(actions in prop::collection::vec(no_failures(), 0..60)) {
        let mut sim = Sim::new();
        let mut intent: BTreeMap<ItemId, Card> = sim.server.clone();
        for a in &actions {
            match a {
                Action::SetPriority(n, p) => {
                    intent.get_mut(&format!("item{n}")).unwrap().priority_id = p.map(|i| PRIORITIES[i].into());
                }
                Action::SetStatus(n, i) => {
                    intent.get_mut(&format!("item{n}")).unwrap().status_id = Some(STATUSES[*i].into());
                }
                Action::ToggleAssignee(n, u) => {
                    let card = intent.get_mut(&format!("item{n}")).unwrap();
                    let user = &users()[*u];
                    if card.assignees.contains(user) {
                        card.assignees.retain(|x| x != user);
                    } else {
                        card.assignees.push(user.clone());
                    }
                }
                _ => {}
            }
            sim.act(a)?;
            let shown = view(&sim.s);
            for (id, want) in &intent {
                prop_assert_eq!(essence(&shown.cards[id]), essence(want), "card {} after {:?}", id, a);
            }
        }
        sim.settle()?;
        prop_assert_eq!(&sim.server.values().map(essence).collect::<Vec<_>>(),
                        &intent.values().map(essence).collect::<Vec<_>>());
    }

    /// Abandoning every failed change puts the board back to GitHub's truth.
    #[test]
    fn abandoning_everything_is_a_rollback(actions in prop::collection::vec(intent(), 0..30)) {
        let mut sim = Sim::new();
        for a in &actions {
            sim.act(a)?;
        }
        loop {
            if !sim.net.is_empty() {
                sim.respond(0, false)?;
            } else if let Some(op) = sim.s.ops.iter().find(|o| o.state == OpState::Failed) {
                let op_id = op.id;
                sim.send(Msg::OpAbandon { op_id })?;
            } else {
                break;
            }
        }
        prop_assert!(sim.s.ops.is_empty());
        prop_assert_eq!(view(&sim.s), sim.s.base.clone());
        prop_assert_eq!(&sim.s.base.cards, &cards((1..=CARDS).map(card)).cards);
    }
}
