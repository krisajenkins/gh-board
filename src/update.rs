//! The reducer: `(State, Msg) -> (State, Vec<Effect>)`. Pure.
//!
//! Messages come from two places: the user (keys, pickers) and the shell
//! (responses, already decoded). Effects are plain data describing what the
//! shell should do next — fetch, mutate, prompt, notify — and the shell turns
//! their outcomes back into messages. Nothing here blocks or performs I/O.
//!
//! Operations on the same card are serialised: only one is in flight per card
//! at a time and the rest queue behind it, so two quick priority changes cannot
//! reach GitHub out of order.

use crate::model::{
    self, ColumnKey, Detail, Filter, Focus, Op, OpKind, OpState, State, board, focus_at, view,
};
use crate::types::{
    Board, Card, Cards, Comment, DetailData, IssueState, ItemId, Label, Meta, StateReason, User,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Towards {
    Prev,
    Next,
}

/// A successful mutation's payload, decoded for the op's kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationResult {
    /// status, create: the whole card. priority, assign, label, state: the issue's
    /// fields (its item id and status are kept from the cached card).
    Card(Card),
    Comment(Comment),
    /// body: the new description.
    Body(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    // Loading
    Refresh,
    /// A page of the board, with more to come.
    BoardPage {
        seq: u64,
        board: Board,
    },
    /// The board's last page.
    BoardLoaded {
        seq: u64,
        board: Board,
    },
    BoardFailed {
        seq: u64,
        err: String,
    },
    AssignableLoaded {
        repo_id: String,
        users: Vec<User>,
    },
    AssignableFailed {
        repo_id: String,
        err: String,
    },
    LoadAssignable {
        repo_id: String,
        repo: String,
    },
    LabelsLoaded {
        repo_id: String,
        labels: Vec<Label>,
    },
    LabelsFailed {
        repo_id: String,
        err: String,
    },
    LoadLabels {
        repo_id: String,
        repo: String,
    },
    // View
    /// `0` shows everything, `1..=n` the n priority options in field order,
    /// and `n+1` the cards with no priority.
    FilterKey(usize),
    Search(String),
    ToggleHidden,
    Focus(Direction),
    // Operations
    SetStatus {
        item_id: ItemId,
        status_id: String,
    },
    MoveCard(Towards),
    SetPriority {
        item_id: ItemId,
        priority_id: Option<String>,
    },
    ToggleAssignee {
        item_id: ItemId,
        user: User,
    },
    AssignMe {
        item_id: ItemId,
    },
    UnassignAll {
        item_id: ItemId,
    },
    /// Add the label if the card lacks it, otherwise remove it.
    ToggleLabel {
        item_id: ItemId,
        label: Label,
    },
    SetState {
        item_id: ItemId,
        issue_state: IssueState,
        reason: Option<StateReason>,
    },
    /// Close an issue as a duplicate of another card's issue.
    CloseAsDuplicate {
        item_id: ItemId,
        original: ItemId,
    },
    ItemLoaded {
        rev: u64,
        card: Card,
    },
    Comment {
        item_id: ItemId,
        body: String,
    },
    SetBody {
        item_id: ItemId,
        body: String,
    },
    Create {
        title: String,
    },
    /// `F`: file feedback about gh-board itself. Not an op: it never touches
    /// the board, so it is not serialised, retried or folded.
    SendFeedback {
        title: String,
        /// The description; may be empty.
        body: String,
    },
    FeedbackSent {
        url: Option<String>,
    },
    FeedbackFailed {
        title: String,
        body: String,
        err: String,
    },
    // Responses to operations
    OpProgress {
        op_id: u64,
        created_issue_id: String,
        created_issue_url: Option<String>,
    },
    /// `Err` when GitHub accepted the change but the response was not what we asked for.
    OpOk {
        op_id: u64,
        result: Result<MutationResult, String>,
    },
    OpFailed {
        op_id: u64,
        err: String,
    },
    OpRetry {
        op_id: u64,
    },
    OpAbandon {
        op_id: u64,
    },
    PromptDismissed {
        op_id: u64,
    },
    ReviewFailures,
    // Detail
    /// `None` opens the focused card.
    OpenDetail {
        item_id: Option<ItemId>,
    },
    ReloadDetail,
    CloseDetail,
    DetailLoaded {
        seq: u64,
        data: DetailData,
    },
    DetailFailed {
        seq: u64,
        err: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Notify {
        message: String,
        level: Level,
    },
    Mutate {
        op: Op,
        meta: Box<Meta>,
    },
    FetchBoard {
        seq: u64,
    },
    FetchAssignable {
        repo_id: String,
        repo: String,
    },
    FetchLabels {
        repo_id: String,
        repo: String,
    },
    /// Re-read one card, after `delay_ms` if set. `rev` goes back in `ItemLoaded`.
    FetchItem {
        item_id: ItemId,
        delay_ms: Option<u64>,
        rev: u64,
        /// For decoding the card that comes back.
        priority_field_id: Option<String>,
    },
    FetchDetail {
        seq: u64,
        issue_id: String,
    },
    PromptFailure {
        op: Op,
    },
    /// File an issue on gh-board's own repository.
    SendFeedback {
        title: String,
        body: String,
    },
    /// Keep text the user would otherwise lose (an abandoned comment).
    SaveText {
        text: String,
        what: String,
    },
}

type Fx = Vec<Effect>;

pub fn update(mut s: State, msg: Msg) -> (State, Vec<Effect>) {
    let mut fx = Vec::new();
    handle(&mut s, msg, &mut fx);
    settle_focus(&mut s);
    (s, fx)
}

fn handle(s: &mut State, msg: Msg, fx: &mut Fx) {
    match msg {
        Msg::Refresh => refresh(s, fx),
        Msg::BoardPage { seq, board } => board_page(s, fx, seq, board, false),
        Msg::BoardLoaded { seq, board } => board_page(s, fx, seq, board, true),
        Msg::BoardFailed { seq, err } => {
            if seq == s.seq {
                // The pages read so far stay: they are server truth too.
                s.loading = false;
                s.incoming = None;
                notify(fx, Level::Error, format!("Loading the board failed: {err}"));
                s.error = Some(err);
            }
        }
        Msg::AssignableLoaded { repo_id, mut users } => {
            users.sort_by_cached_key(|u| u.login.to_lowercase());
            s.assignable.arrived(repo_id, users);
        }
        Msg::AssignableFailed { repo_id, err } => {
            s.assignable.failed(&repo_id);
            notify(
                fx,
                Level::Warn,
                format!("Loading assignable users failed: {err}"),
            );
        }
        Msg::LoadAssignable { repo_id, repo } => {
            if s.assignable.start(&repo_id) {
                fx.push(Effect::FetchAssignable { repo_id, repo });
            }
        }
        Msg::LabelsLoaded {
            repo_id,
            mut labels,
        } => {
            labels.sort_by_cached_key(|l| l.name.to_lowercase());
            s.labels.arrived(repo_id, labels);
        }
        Msg::LabelsFailed { repo_id, err } => {
            s.labels.failed(&repo_id);
            notify(fx, Level::Warn, format!("Loading labels failed: {err}"));
        }
        Msg::LoadLabels { repo_id, repo } => {
            if s.labels.start(&repo_id) {
                fx.push(Effect::FetchLabels { repo_id, repo });
            }
        }
        Msg::FilterKey(n) => filter_key(s, n),
        Msg::Search(query) => search(s, query),
        Msg::ToggleHidden => s.show_hidden = !s.show_hidden,
        Msg::Focus(dir) => focus(s, dir),
        Msg::SetStatus { item_id, status_id } => set_status(s, fx, &item_id, status_id),
        Msg::MoveCard(towards) => move_card(s, fx, towards),
        Msg::SetPriority {
            item_id,
            priority_id,
        } => set_priority(s, fx, &item_id, priority_id),
        Msg::ToggleAssignee { item_id, user } => {
            if let Some(card) = actionable(s, fx, Some(&item_id)) {
                // The name is for pickers; it is not part of an assignment.
                let user = User { name: None, ..user };
                let (add, remove) = toggle(card.is_assigned(&user.id), user);
                assign(s, fx, &card, add, remove);
            }
        }
        Msg::ToggleLabel { item_id, label } => {
            if let Some(card) = actionable(s, fx, Some(&item_id)) {
                let (add, remove) = toggle(card.has_label(&label.id), label);
                change_labels(s, fx, &card, add, remove);
            }
        }
        Msg::AssignMe { item_id } => assign_me(s, fx, &item_id),
        Msg::UnassignAll { item_id } => {
            if let Some(card) = actionable(s, fx, Some(&item_id)) {
                let everyone = card.assignees.clone();
                assign(s, fx, &card, vec![], everyone);
            }
        }
        Msg::SetState {
            item_id,
            issue_state,
            reason,
        } => set_state(s, fx, &item_id, issue_state, reason),
        Msg::CloseAsDuplicate { item_id, original } => {
            close_as_duplicate(s, fx, &item_id, &original)
        }
        Msg::ItemLoaded { rev, card } => item_loaded(s, fx, rev, card),
        Msg::Comment { item_id, body } => comment(s, fx, &item_id, &body),
        Msg::SetBody { item_id, body } => set_body(s, fx, &item_id, &body),
        Msg::Create { title } => create(s, fx, &title),
        Msg::SendFeedback { title, body } => {
            notify(fx, Level::Info, "Sending feedback…".into());
            fx.push(Effect::SendFeedback { title, body });
        }
        Msg::FeedbackSent { url } => {
            let filed = url.map_or_else(
                || "Feedback filed".into(),
                |u| format!("Feedback filed: {u}"),
            );
            notify(fx, Level::Info, format!("{filed}. Thanks!"));
        }
        Msg::FeedbackFailed { title, body, err } => {
            // Quote the title: the prompt has gone. Keep the description,
            // which may be long, on the clipboard. Its own notice comes first
            // so the error is the one left showing.
            let mut message = format!("Sending feedback \"{title}\" failed: {err}");
            if !body.is_empty() {
                fx.push(Effect::SaveText {
                    text: body,
                    what: "The feedback's description".into(),
                });
                message.push_str(". Its description is on the clipboard");
            }
            notify(fx, Level::Error, message);
        }
        Msg::OpProgress {
            op_id,
            created_issue_id,
            created_issue_url,
        } => {
            if let Some(op) = s.ops.iter_mut().find(|o| o.id == op_id)
                && let OpKind::Create {
                    created_issue_id: id,
                    created_issue_url: url,
                    ..
                } = &mut op.kind
            {
                *id = Some(created_issue_id);
                *url = created_issue_url;
            }
        }
        Msg::OpOk { op_id, result } => op_ok(s, fx, op_id, result),
        Msg::OpFailed { op_id, err } => {
            if let Some(op) = s.ops.iter_mut().find(|o| o.id == op_id) {
                op.state = OpState::Failed;
                op.err = Some(err);
                let item_id = op.item_id.clone();
                dequeue(s, fx, &item_id);
                prompt_next(s, fx);
            }
        }
        Msg::OpRetry { op_id } => {
            if let Some(mut again) = take_failed(s, op_id) {
                // Retrying re-asserts the change as the latest intent: it moves
                // to the end with a fresh id, so it applies on top of anything
                // done since. A retried create keeps its temp item id, so focus
                // stays on the card.
                again.err = None;
                enqueue(s, fx, again);
                prompt_next(s, fx);
            }
        }
        Msg::OpAbandon { op_id } => {
            if let Some(op) = take_failed(s, op_id) {
                abandoned(fx, op);
                prompt_next(s, fx);
            }
        }
        Msg::PromptDismissed { op_id } => {
            // Leave the op failed (the card stays marked) until the user
            // reviews failures again.
            if s.prompting == Some(op_id) {
                s.prompting = None;
            }
        }
        Msg::ReviewFailures => {
            s.prompting = None;
            prompt_next(s, fx);
            if s.prompting.is_none() {
                notify(fx, Level::Info, "No failed changes".into());
            }
        }
        Msg::OpenDetail { item_id } => open_detail(s, fx, item_id),
        Msg::ReloadDetail => {
            s.detail_seq += 1;
            if let Some(d) = &mut s.detail {
                d.seq = s.detail_seq;
                d.loading = true;
                d.err = None;
                fx.push(Effect::FetchDetail {
                    seq: d.seq,
                    issue_id: d.issue_id.clone(),
                });
            }
        }
        Msg::CloseDetail => s.detail = None,
        Msg::DetailLoaded { seq, data } => {
            if let Some(d) = s.detail.as_mut().filter(|d| d.seq == seq) {
                d.loading = false;
                d.err = None;
                d.data = Some(data);
            }
        }
        Msg::DetailFailed { seq, err } => {
            if let Some(d) = s.detail.as_mut().filter(|d| d.seq == seq) {
                d.loading = false;
                d.err = Some(err);
            }
        }
    }
}

// Helpers ---------------------------------------------------------------------

fn notify(fx: &mut Fx, level: Level, message: String) {
    fx.push(Effect::Notify { message, level });
}

fn has_inflight(s: &State, item_id: &str) -> bool {
    s.ops
        .iter()
        .any(|op| op.item_id == item_id && op.state == OpState::Inflight)
}

fn send(s: &State, fx: &mut Fx, op: &Op) {
    // Every op is on a card from a loaded board, so meta is always there.
    if let Some(meta) = &s.meta {
        fx.push(Effect::Mutate {
            op: op.clone(),
            meta: Box::new(meta.clone()),
        });
    }
}

/// Add an op: it goes in flight now, or queues behind one already in flight
/// for the same card.
fn enqueue(s: &mut State, fx: &mut Fx, mut op: Op) {
    op.id = s.next_op;
    s.next_op += 1;
    op.state = if has_inflight(s, &op.item_id) {
        OpState::Queued
    } else {
        OpState::Inflight
    };
    if op.state == OpState::Inflight {
        send(s, fx, &op);
    }
    s.ops.push(op);
}

/// Promote the oldest queued op for this card, if nothing else is in flight.
fn dequeue(s: &mut State, fx: &mut Fx, item_id: &str) {
    if has_inflight(s, item_id) {
        return;
    }
    if let Some(op) = s
        .ops
        .iter_mut()
        .find(|o| o.item_id == item_id && o.state == OpState::Queued)
    {
        op.state = OpState::Inflight;
        let op = op.clone();
        send(s, fx, &op);
    }
}

/// Remove a failed op, to retry or abandon it.
fn take_failed(s: &mut State, op_id: u64) -> Option<Op> {
    let i = s
        .ops
        .iter()
        .position(|o| o.id == op_id && o.state == OpState::Failed)?;
    if s.prompting == Some(op_id) {
        s.prompting = None;
    }
    Some(s.ops.remove(i))
}

/// Ask about the next failed op, unless a prompt is already showing.
fn prompt_next(s: &mut State, fx: &mut Fx) {
    if let Some(id) = s.prompting
        && s.ops.iter().any(|o| o.id == id)
    {
        return;
    }
    s.prompting = None;
    if let Some(op) = s.ops.iter().find(|o| o.state == OpState::Failed) {
        s.prompting = Some(op.id);
        fx.push(Effect::PromptFailure { op: op.clone() });
    }
}

/// Look up a card in the current view. `None` (with a notification queued for
/// a card still being created) when it cannot be acted on.
fn actionable(s: &State, fx: &mut Fx, item_id: Option<&str>) -> Option<Card> {
    let card = view(s).cards.remove(item_id?)?;
    if card.temp {
        notify(fx, Level::Warn, "That card is still being created".into());
        return None;
    }
    Some(card)
}

fn fetch_item(s: &State, item_id: ItemId, delay_ms: Option<u64>) -> Effect {
    Effect::FetchItem {
        item_id,
        delay_ms,
        rev: s.confirms,
        priority_field_id: s
            .meta
            .as_ref()
            .and_then(|m| m.priority_field.as_ref())
            .map(|f| f.id.clone()),
    }
}

// Loading ---------------------------------------------------------------------

fn refresh(s: &mut State, fx: &mut Fx) {
    s.seq += 1;
    s.loading = true;
    s.error = None;
    s.refresh_from = s.confirms;
    s.incoming = None;
    fx.push(Effect::FetchBoard { seq: s.seq });
}

/// The freshly loaded cards, except where an op confirmed while the fetch was
/// running. The fetch's pages may have been read before that change landed,
/// and the op is no longer in `ops` to re-apply it, so the confirmed card
/// (also server truth, and at least as new as the change) is kept instead.
fn keep_confirmed(s: &State, mut cards: Cards) -> Cards {
    for (item_id, &at) in &s.confirmed_at {
        if at > s.refresh_from
            && let Some(card) = s.base.cards.get(item_id)
        {
            // If it is missing, it was created (or added to the project) after
            // its page was read.
            cards.put(card.clone());
        }
    }
    cards
}

/// One page of a board fetch. The board shows every card read so far, in
/// GitHub's order, followed by any cards from before the fetch it has not
/// reached yet: the first load fills in page by page, and a refresh never
/// blanks the board. The last page drops the cards it never saw.
fn board_page(s: &mut State, fx: &mut Fx, seq: u64, board: Board, last: bool) {
    if seq != s.seq {
        return;
    }
    let Board { meta, cards } = board;
    let first = s.incoming.is_none();

    let mut repos: Vec<(String, String)> = Vec::new();
    let mut incoming = s.incoming.take().unwrap_or_default();
    for card in cards.iter() {
        if let (Some(id), Some(name)) = (&card.repo_id, &card.repo)
            && !repos.iter().any(|(r, _)| r == id)
        {
            repos.push((id.clone(), name.clone()));
        }
        incoming.put(card.clone());
    }

    let mut shown = incoming.clone();
    if last {
        s.loading = false;
    } else {
        for card in s.base.iter() {
            if !shown.cards.contains_key(&card.item_id) {
                shown.put(card.clone());
            }
        }
        s.incoming = Some(incoming);
    }
    s.base = keep_confirmed(s, shown);
    if first {
        board_meta(s, fx, &meta);
    }
    s.meta = Some(meta);

    for (repo_id, repo) in repos {
        if s.assignable.start(&repo_id) {
            fx.push(Effect::FetchAssignable { repo_id, repo });
        }
    }
}

/// Check the filters against a fetch's fields, once per fetch.
fn board_meta(s: &mut State, fx: &mut Fx, meta: &Meta) {
    let field = meta.priority_field.as_ref();
    if let Filter::Priority(id) = &s.filter
        && field.and_then(|f| f.option_name(id)).is_none()
    {
        s.filter = Filter::All;
    }
    if !s.priority_applied {
        // The configured starting filter, resolved once option ids are known.
        s.priority_applied = true;
        if let (Some(wanted), Some(field)) = (&s.config.priority, field) {
            match field.option_id(wanted) {
                Some(id) => s.filter = Filter::Priority(id.into()),
                None => notify(
                    fx,
                    Level::Warn,
                    format!("No priority named \"{wanted}\": showing all"),
                ),
            }
        }
    }
    if field.is_none() {
        notify(
            fx,
            Level::Warn,
            format!(
                "No issue field named \"{}\": priority filters are off",
                s.config.priority_field
            ),
        );
    }
}

// View ------------------------------------------------------------------------

fn filter_key(s: &mut State, n: usize) {
    let field = s.meta.as_ref().and_then(|m| m.priority_field.as_ref());
    let options = field.map(|f| f.options.as_slice()).unwrap_or_default();
    if n == 0 {
        s.filter = Filter::All;
    } else if let Some(opt) = options.get(n - 1) {
        s.filter = Filter::Priority(opt.id.clone());
    } else if n == options.len() + 1 && field.is_some() {
        s.filter = Filter::NoPriority;
    }
}

/// Set the fuzzy search query; "" clears it. When the focused column has
/// nothing left in it, focus jumps to the first match, so Enter opens it.
fn search(s: &mut State, query: String) {
    s.search = query;
    let v = board(s);
    if v.focused().is_some() {
        return;
    }
    if let Some(ci) = v.columns.iter().position(|c| !c.cards.is_empty()) {
        s.focus = focus_at(&v.columns, ci, Some(0), None);
    }
}

fn focus(s: &mut State, dir: Direction) {
    let v = board(s);
    let Some(mut ci) = v.col else {
        return;
    };
    let mut ri = v.row;
    let mut want = s.focus.want.unwrap_or(ri.unwrap_or(0));
    let len = |ci: usize| v.columns[ci].cards.len();
    match dir {
        Direction::Left | Direction::Right => {
            ci = if dir == Direction::Left {
                ci.saturating_sub(1)
            } else {
                (ci + 1).min(v.columns.len() - 1)
            };
            let n = len(ci);
            ri = (n > 0).then(|| want.min(n - 1));
        }
        Direction::Up | Direction::Down | Direction::Top | Direction::Bottom => {
            let n = len(ci);
            let Some(r) = ri.filter(|_| n > 0) else {
                return;
            };
            let r = match dir {
                Direction::Up => r.saturating_sub(1),
                Direction::Down => (r + 1).min(n - 1),
                Direction::Top => 0,
                _ => n - 1,
            };
            want = r;
            ri = Some(r);
        }
    }
    s.focus = focus_at(&v.columns, ci, ri, Some(want));
}

// Operations ------------------------------------------------------------------

fn set_status(s: &mut State, fx: &mut Fx, item_id: &str, status_id: String) {
    let Some(card) = actionable(s, fx, Some(item_id)) else {
        return;
    };
    if card.status_id.as_deref() == Some(&status_id) {
        return;
    }
    let Some(meta) = &s.meta else {
        return;
    };
    let name = meta.status_field.option_name(&status_id).unwrap_or("?");
    let desc = format!("Move {} to {name}", card.label());
    enqueue(
        s,
        fx,
        Op::new(
            card.item_id,
            card.issue_id,
            desc,
            OpKind::Status { status_id },
        ),
    );
}

/// Move the focused card to the next visible status column left or right.
fn move_card(s: &mut State, fx: &mut Fx, towards: Towards) {
    let v = board(s);
    let (Some(card), Some(ci)) = (v.focused(), v.col) else {
        return;
    };
    let is_status = |c: &&model::Column| c.key != ColumnKey::NoStatus;
    let target = match towards {
        Towards::Prev => v.columns[..ci].iter().rev().find(is_status),
        Towards::Next => v.columns[ci + 1..].iter().find(is_status),
    };
    if let Some(model::Column {
        key: ColumnKey::Status(status_id),
        ..
    }) = target
    {
        set_status(s, fx, &card.item_id, status_id.clone());
    }
}

fn set_priority(s: &mut State, fx: &mut Fx, item_id: &str, priority_id: Option<String>) {
    let Some(card) = actionable(s, fx, Some(item_id)) else {
        return;
    };
    if card.priority_id == priority_id {
        return;
    }
    let Some(field) = s.meta.as_ref().and_then(|m| m.priority_field.as_ref()) else {
        return notify(fx, Level::Warn, "This board has no priority field".into());
    };
    let name = priority_id
        .as_deref()
        .and_then(|id| field.option_name(id))
        .unwrap_or("none");
    let desc = format!("Set {} priority to {name}", card.label());
    enqueue(
        s,
        fx,
        Op::new(
            card.item_id,
            card.issue_id,
            desc,
            OpKind::Priority { priority_id },
        ),
    );
}

/// Toggling a member of a set: `(add, remove)`, one of them holding it.
fn toggle<T>(present: bool, member: T) -> (Vec<T>, Vec<T>) {
    if present {
        (vec![], vec![member])
    } else {
        (vec![member], vec![])
    }
}

/// Enqueue a change to one of a card's sets (assignees, labels), described
/// as `<verb> #3 +added -removed`. Nothing happens when it changes nothing.
fn enqueue_set_change<T>(
    s: &mut State,
    fx: &mut Fx,
    card: &Card,
    verb: &str,
    (add, remove): (Vec<T>, Vec<T>),
    name: impl Fn(&T) -> String,
    kind: impl FnOnce(Vec<T>, Vec<T>) -> OpKind,
) {
    if add.is_empty() && remove.is_empty() {
        return;
    }
    let words: Vec<String> = add
        .iter()
        .map(|m| format!("+{}", name(m)))
        .chain(remove.iter().map(|m| format!("-{}", name(m))))
        .collect();
    let desc = format!("{verb} {} {}", card.label(), words.join(" "));
    enqueue(
        s,
        fx,
        Op::new(
            card.item_id.clone(),
            card.issue_id.clone(),
            desc,
            kind(add, remove),
        ),
    );
}

fn assign(s: &mut State, fx: &mut Fx, card: &Card, add: Vec<User>, remove: Vec<User>) {
    enqueue_set_change(
        s,
        fx,
        card,
        "Assign",
        (add, remove),
        |u| format!("@{}", u.login),
        |add, remove| OpKind::Assign { add, remove },
    );
}

fn change_labels(s: &mut State, fx: &mut Fx, card: &Card, add: Vec<Label>, remove: Vec<Label>) {
    enqueue_set_change(
        s,
        fx,
        card,
        "Label",
        (add, remove),
        |l| l.name.clone(),
        |add, remove| OpKind::Label { add, remove },
    );
}

fn assign_me(s: &mut State, fx: &mut Fx, item_id: &str) {
    let Some(card) = actionable(s, fx, Some(item_id)) else {
        return;
    };
    let Some(me) = s.meta.as_ref().map(|m| m.viewer.clone()) else {
        return;
    };
    if card.is_assigned(&me.id) {
        return notify(
            fx,
            Level::Info,
            format!("You are already assigned to {}", card.label()),
        );
    }
    assign(s, fx, &card, vec![me], vec![]);
}

/// Close (with a reason, default completed) or reopen an issue.
fn set_state(
    s: &mut State,
    fx: &mut Fx,
    item_id: &str,
    issue_state: IssueState,
    reason: Option<StateReason>,
) {
    let Some(card) = actionable(s, fx, Some(item_id)) else {
        return;
    };
    let reason = match issue_state {
        IssueState::Closed => Some(reason.unwrap_or(StateReason::Completed)),
        IssueState::Open => None,
    };
    if card.state == Some(issue_state)
        && (issue_state == IssueState::Open || card.state_reason == reason)
    {
        return;
    }
    let desc = match reason {
        None => format!("Reopen {}", card.label()),
        Some(r) => format!("Close {} as {}", card.label(), r.describe()),
    };
    enqueue(
        s,
        fx,
        Op::new(
            card.item_id,
            card.issue_id,
            desc,
            OpKind::State {
                issue_state,
                state_reason: reason,
                duplicate_of: None,
            },
        ),
    );
}

/// Close an issue as a duplicate of another card's issue.
fn close_as_duplicate(s: &mut State, fx: &mut Fx, item_id: &str, original: &str) {
    if item_id == original {
        notify(fx, Level::Warn, "An issue cannot duplicate itself".into());
        return;
    }
    let Some(card) = actionable(s, fx, Some(item_id)) else {
        return;
    };
    let Some(orig) = actionable(s, fx, Some(original)) else {
        return;
    };
    let Some(original_issue) = orig.issue_id.clone() else {
        notify(fx, Level::Warn, format!("{} is not an issue", orig.label()));
        return;
    };
    if card.state == Some(IssueState::Closed) {
        notify(
            fx,
            Level::Info,
            format!("{} is already closed", card.label()),
        );
        return;
    }
    let desc = format!("Close {} as a duplicate of {}", card.label(), orig.label());
    enqueue(
        s,
        fx,
        Op::new(
            card.item_id,
            card.issue_id,
            desc,
            OpKind::State {
                issue_state: IssueState::Closed,
                state_reason: Some(StateReason::Duplicate),
                duplicate_of: Some(original_issue),
            },
        ),
    );
}

/// A re-read of one item arrived. It replaces the cached card; unconfirmed
/// ops keep applying on top, as after a full refresh.
///
/// If an op on the card confirmed after the re-read was asked for, the read
/// may predate that change, so it is thrown away and the card read again.
fn item_loaded(s: &mut State, fx: &mut Fx, rev: u64, card: Card) {
    if !s.base.cards.contains_key(&card.item_id) {
        return;
    }
    if s.confirmed_at.get(&card.item_id).copied().unwrap_or(0) > rev {
        fx.push(fetch_item(s, card.item_id, None));
        return;
    }
    // A later page of a fetch in flight would otherwise bring back the older
    // copy it read.
    if let Some(incoming) = s
        .incoming
        .as_mut()
        .filter(|i| i.cards.contains_key(&card.item_id))
    {
        incoming.put(card.clone());
    }
    s.base.put(card);
}

fn comment(s: &mut State, fx: &mut Fx, item_id: &str, body: &str) {
    let Some(card) = actionable(s, fx, Some(item_id)) else {
        return;
    };
    let body = body.trim();
    if body.is_empty() {
        return notify(fx, Level::Warn, "Empty comment not posted".into());
    }
    let desc = format!("Comment on {}", card.label());
    enqueue(
        s,
        fx,
        Op::new(
            card.item_id,
            card.issue_id,
            desc,
            OpKind::Comment { body: body.into() },
        ),
    );
}

/// Line endings and trailing whitespace, which an editor round trip changes
/// without anyone meaning to.
fn normalise_body(body: &str) -> String {
    body.replace('\r', "").trim_end().to_string()
}

/// Replace an issue's description. A body that is what the detail view
/// already shows is not sent.
fn set_body(s: &mut State, fx: &mut Fx, item_id: &str, body: &str) {
    let Some(card) = actionable(s, fx, Some(item_id)) else {
        return;
    };
    if card.issue_id.is_none() {
        return;
    }
    let body = normalise_body(body);
    let current = model::description(s.detail.as_ref(), &s.ops, &card.item_id);
    if current.is_some_and(|(text, _)| normalise_body(text) == body) {
        return notify(fx, Level::Info, "Description unchanged".into());
    }
    let desc = format!("Edit the description of {}", card.label());
    enqueue(
        s,
        fx,
        Op::new(card.item_id, card.issue_id, desc, OpKind::Body { body }),
    );
}

/// Create an issue in the focused column, at the priority being filtered on.
fn create(s: &mut State, fx: &mut Fx, title: &str) {
    let title = title.trim();
    if title.is_empty() || s.meta.is_none() {
        return;
    }
    let repo_id = match model::target_repo(s) {
        Ok(id) => id,
        Err(e) => return notify(fx, Level::Error, format!("Cannot create an issue: {e}")),
    };
    let v = board(s);
    let col = v.focused_column();
    let status_id = match col.map(|c| &c.key) {
        Some(ColumnKey::Status(id)) => Some(id.clone()),
        _ => None,
    };
    let priority_id = match &s.filter {
        Filter::Priority(id) => Some(id.clone()),
        _ => None,
    };
    let item_id = format!("tmp:{}", s.next_op);
    let kind = OpKind::Create {
        title: title.into(),
        repo_id,
        status_id,
        priority_id,
        created_issue_id: None,
        created_issue_url: None,
    };
    enqueue(
        s,
        fx,
        Op::new(item_id.clone(), None, format!("Create “{title}”"), kind),
    );
    s.focus = Focus {
        key: col.map(|c| c.key.clone()),
        col: v.col.unwrap_or(0),
        row: s.focus.row,
        item_id: Some(item_id),
        want: None,
    };
}

// Responses to operations -----------------------------------------------------

fn op_ok(s: &mut State, fx: &mut Fx, op_id: u64, result: Result<MutationResult, String>) {
    let Some(op) = s.ops.iter().find(|o| o.id == op_id).cloned() else {
        return;
    };
    // The op itself is done, and older failures it overrides are moot.
    s.ops.retain(|o| {
        let superseded = o.state == OpState::Failed
            && o.id < op.id
            && o.item_id == op.item_id
            && o.kind.same_kind(&op.kind)
            && op.kind.supersedes();
        o.id != op.id && !superseded
    });

    let mut written: Option<ItemId> = None;
    let confirmed = match (&op.kind, result) {
        (OpKind::Body { .. }, Ok(MutationResult::Body(body))) => {
            if let Some(data) = detail_data(s, &op.item_id) {
                data.body = body;
            }
            true
        }
        (OpKind::Comment { .. }, Ok(MutationResult::Comment(c))) => {
            if let Some(data) = detail_data(s, &op.item_id) {
                data.comments.push(c);
                data.total += 1;
            }
            true
        }
        (OpKind::Status { .. } | OpKind::Create { .. }, Ok(MutationResult::Card(card))) => {
            if matches!(op.kind, OpKind::Create { .. })
                && s.focus.item_id.as_ref() == Some(&op.item_id)
            {
                s.focus.item_id = Some(card.item_id.clone());
            }
            written = Some(card.item_id.clone());
            s.base.put(card);
            true
        }
        (
            OpKind::Priority { .. }
            | OpKind::Assign { .. }
            | OpKind::Label { .. }
            | OpKind::State { .. },
            Ok(MutationResult::Card(card)),
        ) => {
            if let Some(old) = s.base.cards.get(&op.item_id) {
                let merged = Card {
                    item_id: old.item_id.clone(),
                    status_id: old.status_id.clone(),
                    ..card
                };
                written = Some(merged.item_id.clone());
                s.base.put(merged);
            }
            true
        }
        (_, result) => {
            // GitHub accepted the change but the response was not what we
            // asked for; the cache can't be trusted, so reload it.
            let err = result
                .err()
                .unwrap_or_else(|| "response does not match the change".into());
            notify(
                fx,
                Level::Warn,
                format!("{}: unexpected response ({err}); refreshing", op.desc),
            );
            refresh(s, fx);
            false
        }
    };

    if let Some(item_id) = written {
        // Tick the clock, so a board or item fetch already running knows its
        // copy of this card may be older than what we now hold.
        s.confirms += 1;
        s.confirmed_at.insert(item_id, s.confirms);
    }

    if confirmed && matches!(op.kind, OpKind::State { .. } | OpKind::Status { .. }) {
        // Project workflows react to these (closing moves an item to Done;
        // Done closes the issue) a few seconds later, outside our response.
        fx.push(fetch_item(s, op.item_id.clone(), Some(4000)));
    }

    dequeue(s, fx, &op.item_id);
    prompt_next(s, fx);
}

fn detail_data<'a>(s: &'a mut State, item_id: &str) -> Option<&'a mut DetailData> {
    s.detail
        .as_mut()
        .filter(|d| d.item_id == item_id)
        .and_then(|d| d.data.as_mut())
}

fn abandoned(fx: &mut Fx, op: Op) {
    match op.kind {
        OpKind::Comment { body } => fx.push(Effect::SaveText {
            text: body,
            what: "Abandoned comment".into(),
        }),
        OpKind::Body { body } => fx.push(Effect::SaveText {
            text: body,
            what: "Abandoned description".into(),
        }),
        OpKind::Create {
            title,
            created_issue_id: Some(id),
            created_issue_url,
            ..
        } => {
            // A later step failed (adding it to the project, or setting its
            // status), so the issue may well be missing from the board.
            let link = created_issue_url.unwrap_or(id);
            notify(
                fx,
                Level::Warn,
                format!(
                    "“{title}” was already created on GitHub; abandoning does not delete it: {link}"
                ),
            );
        }
        _ => {}
    }
}

// Detail ----------------------------------------------------------------------

fn open_detail(s: &mut State, fx: &mut Fx, item_id: Option<ItemId>) {
    let item_id = item_id.or_else(|| board(s).focused().map(|c| c.item_id.clone()));
    let Some(card) = actionable(s, fx, item_id.as_deref()) else {
        return;
    };
    let Some(issue_id) = card.issue_id else {
        return;
    };
    s.detail_seq += 1;
    s.detail = Some(Detail {
        item_id: card.item_id,
        issue_id: issue_id.clone(),
        seq: s.detail_seq,
        loading: true,
        err: None,
        data: None,
    });
    fx.push(Effect::FetchDetail {
        seq: s.detail_seq,
        issue_id,
    });
}

// Entry point -----------------------------------------------------------------

/// Keep the stored focus pointing at what is actually on screen, so the next
/// movement starts from the right place after cards move or filters change.
/// A focused card hidden by a filter is forgotten; one moved by an op is
/// followed.
fn settle_focus(s: &mut State) {
    let v = board(s);
    if let Some(ci) = v.col {
        s.focus = focus_at(&v.columns, ci, v.row, s.focus.want);
    }
}
