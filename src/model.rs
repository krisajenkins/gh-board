//! The state shape, and everything derived from it. Pure.
//!
//! The central idea: what the board shows is never stored. It is derived as
//!
//! ```text
//! view = fold(apply, base, ops)
//! ```
//!
//! where `base` is the last thing the server told us and `ops` are the changes
//! the user has made that the server has not yet confirmed. Optimism is just
//! the fold; confirming an op moves its effect into `base`; abandoning one
//! deletes it, and the rollback falls out for free. A refresh replaces `base`
//! while unconfirmed ops keep applying on top of it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::mem::discriminant;

use crate::search;
use crate::types::{
    Card, Cards, Config, DetailData, IssueState, ItemId, Label, Member, Meta, StateReason, User,
};

/// What an op changes. The payload is the user's intent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpKind {
    Status {
        status_id: String,
    },
    /// `None` clears the priority.
    Priority {
        priority_id: Option<String>,
    },
    Assign {
        add: Vec<User>,
        remove: Vec<User>,
    },
    /// Add and/or remove labels.
    Label {
        add: Vec<Label>,
        remove: Vec<Label>,
    },
    Comment {
        body: String,
    },
    /// Replace the issue's description.
    Body {
        body: String,
    },
    State {
        issue_state: IssueState,
        /// Set when closing.
        state_reason: Option<StateReason>,
        /// The original's issue id, when closing as a duplicate.
        duplicate_of: Option<String>,
    },
    Create {
        title: String,
        repo_id: String,
        status_id: Option<String>,
        priority_id: Option<String>,
        /// Set once the issue exists, so a retry does not create it twice.
        created_issue_id: Option<String>,
        /// That issue's link, for the warning if the op is abandoned.
        created_issue_url: Option<String>,
    },
}

impl OpKind {
    pub fn same_kind(&self, other: &OpKind) -> bool {
        discriminant(self) == discriminant(other)
    }

    /// Whether a success of this kind makes older failures of the same kind,
    /// on the same card, moot. (A newer comment does not replace an old one.)
    pub fn supersedes(&self) -> bool {
        matches!(
            self,
            OpKind::Status { .. }
                | OpKind::Priority { .. }
                | OpKind::State { .. }
                | OpKind::Body { .. }
        )
    }
}

/// A list fetched once per repository, when first needed: the users who can
/// be assigned there, or its labels. Keyed by repo id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerRepo<T> {
    pub loaded: BTreeMap<String, Vec<T>>,
    pub loading: BTreeSet<String>,
}

impl<T> Default for PerRepo<T> {
    fn default() -> Self {
        PerRepo {
            loaded: BTreeMap::new(),
            loading: BTreeSet::new(),
        }
    }
}

impl<T> PerRepo<T> {
    pub fn get(&self, repo_id: &str) -> Option<&[T]> {
        self.loaded.get(repo_id).map(Vec::as_slice)
    }

    pub fn has(&self, repo_id: &str) -> bool {
        self.loaded.contains_key(repo_id)
    }

    /// Note a fetch starting. `false` when the list is already loaded or on
    /// its way, so there is nothing to fetch.
    pub fn start(&mut self, repo_id: &str) -> bool {
        !self.has(repo_id) && self.loading.insert(repo_id.into())
    }

    pub fn arrived(&mut self, repo_id: String, items: Vec<T>) {
        self.loading.remove(&repo_id);
        self.loaded.insert(repo_id, items);
    }

    /// A failed fetch: the next request for the list tries again.
    pub fn failed(&mut self, repo_id: &str) {
        self.loading.remove(repo_id);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpState {
    Queued,
    Inflight,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    /// Monotonic; later ids are later intents.
    pub id: u64,
    pub item_id: ItemId,
    pub issue_id: Option<String>,
    /// Human description, for prompts and notifications.
    pub desc: String,
    pub state: OpState,
    pub err: Option<String>,
    pub kind: OpKind,
}

impl Op {
    /// An op not yet enqueued: `id` and `state` are assigned by the reducer.
    pub fn new(item_id: ItemId, issue_id: Option<String>, desc: String, kind: OpKind) -> Self {
        Op {
            id: 0,
            item_id,
            issue_id,
            desc,
            state: OpState::Queued,
            err: None,
            kind,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    NoPriority,
    /// A priority option id.
    Priority(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKey {
    NoStatus,
    /// A status option id.
    Status(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    /// The focused column's key.
    pub key: Option<ColumnKey>,
    /// Column index, the fallback when `key` is no longer shown.
    pub col: usize,
    /// Row within the column, the fallback when the card is gone.
    pub row: usize,
    pub item_id: Option<ItemId>,
    /// The row up/down last chose; left/right aim for it, like Vim's curswant.
    pub want: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub item_id: ItemId,
    pub issue_id: String,
    pub seq: u64,
    pub loading: bool,
    pub err: Option<String>,
    pub data: Option<DetailData>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub config: Config,
    pub loading: bool,
    pub error: Option<String>,
    /// Board fetch sequence; stale responses are dropped.
    pub seq: u64,
    /// A clock that ticks each time a confirmed op writes to `base`.
    pub confirms: u64,
    /// Item id -> `confirms` when a confirmed op last wrote its card.
    pub confirmed_at: BTreeMap<ItemId, u64>,
    /// `confirms` when the current board fetch was sent.
    pub refresh_from: u64,
    pub meta: Option<Meta>,
    /// Last server truth.
    pub base: Cards,
    /// The cards the board fetch in flight has read so far, in order; `None`
    /// until its first page arrives.
    pub incoming: Option<Cards>,
    /// Unconfirmed changes, oldest first. Ids ascend.
    pub ops: Vec<Op>,
    pub next_op: u64,
    pub filter: Filter,
    /// Fuzzy search query; "" shows everything.
    pub search: String,
    pub show_hidden: bool,
    /// Whether `config.priority` has been turned into a filter yet.
    pub priority_applied: bool,
    pub focus: Focus,
    /// The users each repository's issues can be assigned to.
    pub assignable: PerRepo<User>,
    /// Each repository's labels.
    pub labels: PerRepo<Label>,
    pub detail: Option<Detail>,
    pub detail_seq: u64,
    /// Op id of the failure currently being asked about.
    pub prompting: Option<u64>,
}

impl State {
    pub fn new(config: Config) -> Self {
        State {
            config,
            loading: false,
            error: None,
            seq: 0,
            confirms: 0,
            confirmed_at: BTreeMap::new(),
            refresh_from: 0,
            meta: None,
            base: Cards::default(),
            incoming: None,
            ops: Vec::new(),
            next_op: 1,
            filter: Filter::All,
            search: String::new(),
            show_hidden: false,
            priority_applied: false,
            focus: Focus::default(),
            assignable: PerRepo::default(),
            labels: PerRepo::default(),
            detail: None,
            detail_seq: 0,
            prompting: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub key: ColumnKey,
    pub name: String,
    pub cards: Vec<Card>,
}

/// Apply one op to a card.
fn apply(card: &mut Card, op: &Op) {
    match &op.kind {
        OpKind::Status { status_id } => card.status_id = Some(status_id.clone()),
        OpKind::Priority { priority_id } => card.priority_id = priority_id.clone(),
        OpKind::State {
            issue_state,
            state_reason,
            ..
        } => {
            card.state = Some(*issue_state);
            card.state_reason = match issue_state {
                IssueState::Closed => *state_reason,
                IssueState::Open => Some(StateReason::Reopened),
            };
        }
        OpKind::Assign { add, remove } => change_set(&mut card.assignees, add, remove),
        OpKind::Label { add, remove } => change_set(&mut card.labels, add, remove),
        OpKind::Comment { .. } | OpKind::Body { .. } | OpKind::Create { .. } => {}
    }
    if op.state == OpState::Failed {
        card.failed = true;
    } else {
        card.pending = true;
    }
}

/// Remove then add members of a set (a card's assignees or labels), matching
/// them by id. Adding one already there changes nothing.
pub fn change_set<T: Member + Clone>(set: &mut Vec<T>, add: &[T], remove: &[T]) {
    set.retain(|m| !remove.iter().any(|r| r.id() == m.id()));
    for m in add {
        if !set.iter().any(|x| x.id() == m.id()) {
            set.push(m.clone());
        }
    }
}

/// The cards as the user should see them: base with every unconfirmed op applied.
pub fn view(state: &State) -> Cards {
    let mut out = state.base.clone();
    for op in &state.ops {
        if let OpKind::Create {
            title,
            repo_id,
            status_id,
            priority_id,
            ..
        } = &op.kind
            && !out.cards.contains_key(&op.item_id)
        {
            out.put(Card {
                item_id: op.item_id.clone(),
                title: title.clone(),
                status_id: status_id.clone(),
                priority_id: priority_id.clone(),
                repo_id: Some(repo_id.clone()),
                temp: true,
                ..Card::default()
            });
        }
        if let Some(card) = out.cards.get_mut(&op.item_id) {
            apply(card, op);
        }
    }
    out
}

/// An issue's description as the user should see it: the newest unconfirmed
/// edit if there is one, otherwise what the detail view loaded. The same fold
/// as the cards, for the one field that lives outside them. `None` when it is
/// not known yet.
pub fn description<'a>(
    detail: Option<&'a Detail>,
    ops: &'a [Op],
    item_id: &str,
) -> Option<(&'a str, Option<&'a Op>)> {
    let edit = ops.iter().rev().find_map(|op| match &op.kind {
        OpKind::Body { body } if op.item_id == item_id => Some((body.as_str(), Some(op))),
        _ => None,
    });
    edit.or_else(|| {
        detail
            .filter(|d| d.item_id == item_id)
            .and_then(|d| d.data.as_ref())
            .map(|data| (data.body.as_str(), None))
    })
}

pub fn matches_filter(filter: &Filter, card: &Card) -> bool {
    match filter {
        Filter::All => true,
        Filter::NoPriority => card.priority_id.is_none(),
        Filter::Priority(id) => card.priority_id.as_deref() == Some(id),
    }
}

/// A card's assignee sort key: "" when unassigned (so it sorts first),
/// otherwise the alphabetically first login, lowercased.
pub fn assignee_key(card: &Card) -> String {
    card.assignees
        .iter()
        .map(|u| u.login.to_lowercase())
        .min()
        .unwrap_or_default()
}

/// Cards ordered by assignee, unassigned first. Stable: cards with the same
/// key keep their order (GitHub's item order).
pub fn sort_by_assignee(cards: &mut [Card]) {
    cards.sort_by_cached_key(assignee_key);
}

/// The visible columns, in status-field order, with "No Status" first when it
/// has anything in it.
pub fn columns(state: &State, cards: &Cards) -> Vec<Column> {
    let Some(meta) = &state.meta else {
        return Vec::new();
    };
    let hidden: HashSet<&str> = state
        .config
        .hidden_statuses
        .iter()
        .map(String::as_str)
        .collect();
    let mut cols = vec![Column {
        key: ColumnKey::NoStatus,
        name: "No Status".into(),
        cards: Vec::new(),
    }];
    cols.extend(
        meta.status_field
            .options
            .iter()
            .filter(|opt| state.show_hidden || !hidden.contains(opt.name.as_str()))
            .map(|opt| Column {
                key: ColumnKey::Status(opt.id.clone()),
                name: opt.name.clone(),
                cards: Vec::new(),
            }),
    );
    let terms = search::parse(&state.search);
    let pf = meta.priority_field.as_ref();
    for card in cards.iter() {
        if !matches_filter(&state.filter, card) {
            continue;
        }
        if !terms.is_empty() {
            let priority = pf
                .zip(card.priority_id.as_deref())
                .and_then(|(f, id)| f.option_name(id));
            if !search::matches(&terms, &search::fields(card, priority)) {
                continue;
            }
        }
        let key = match card.status_id.as_deref() {
            Some(id) if meta.status_field.option_name(id).is_some() => ColumnKey::Status(id.into()),
            _ => ColumnKey::NoStatus,
        };
        if let Some(col) = cols.iter_mut().find(|c| c.key == key) {
            col.cards.push(card.clone());
        }
    }
    for col in &mut cols {
        sort_by_assignee(&mut col.cards);
    }
    if cols[0].cards.is_empty() {
        cols.remove(0);
    }
    cols
}

/// Where the focus lands among these columns: the focused card if it is still
/// visible, otherwise the same column (by status, then by index) at the same
/// row. `None` when there are no columns; the row is `None` when the column is
/// empty.
pub fn locate(columns: &[Column], focus: &Focus) -> Option<(usize, Option<usize>)> {
    if columns.is_empty() {
        return None;
    }
    if let Some(id) = &focus.item_id {
        for (ci, col) in columns.iter().enumerate() {
            if let Some(ri) = col.cards.iter().position(|c| &c.item_id == id) {
                return Some((ci, Some(ri)));
            }
        }
    }
    let ci = columns
        .iter()
        .position(|c| Some(&c.key) == focus.key.as_ref())
        .unwrap_or(focus.col.min(columns.len() - 1));
    let n = columns[ci].cards.len();
    Some((ci, (n > 0).then(|| focus.row.min(n - 1))))
}

/// A focus pointing exactly at (col, row) in these columns.
pub fn focus_at(columns: &[Column], ci: usize, ri: Option<usize>, want: Option<usize>) -> Focus {
    match columns.get(ci) {
        None => Focus {
            want,
            ..Focus::default()
        },
        Some(col) => Focus {
            key: Some(col.key.clone()),
            col: ci,
            row: ri.unwrap_or(0),
            item_id: ri.and_then(|r| col.cards.get(r)).map(|c| c.item_id.clone()),
            want,
        },
    }
}

/// Everything a renderer needs, derived in one go.
#[derive(Clone, Debug)]
pub struct BoardView {
    pub cards: Cards,
    pub columns: Vec<Column>,
    /// `None` when there are no columns.
    pub col: Option<usize>,
    /// `None` when the focused column is empty.
    pub row: Option<usize>,
}

impl BoardView {
    pub fn focused(&self) -> Option<&Card> {
        self.columns.get(self.col?)?.cards.get(self.row?)
    }

    pub fn focused_column(&self) -> Option<&Column> {
        self.columns.get(self.col?)
    }
}

pub fn board(state: &State) -> BoardView {
    let cards = view(state);
    let columns = columns(state, &cards);
    let at = locate(&columns, &state.focus);
    BoardView {
        cards,
        columns,
        col: at.map(|(c, _)| c),
        row: at.and_then(|(_, r)| r),
    }
}

pub fn filter_label(state: &State) -> String {
    match &state.filter {
        Filter::All => "All".into(),
        Filter::NoPriority => "No priority".into(),
        Filter::Priority(id) => state
            .meta
            .as_ref()
            .and_then(|m| m.priority_field.as_ref())
            .and_then(|f| f.option_name(id))
            .unwrap_or("?")
            .into(),
    }
}

/// Counts of (unconfirmed, failed) ops.
pub fn op_counts(state: &State) -> (usize, usize) {
    let failed = state
        .ops
        .iter()
        .filter(|op| op.state == OpState::Failed)
        .count();
    (state.ops.len() - failed, failed)
}

/// The repository new issues go into: the configured one if it is on the
/// board, otherwise the one most of the board's issues live in.
pub fn target_repo(state: &State) -> Result<String, String> {
    // repo id -> (count, name)
    let mut repos: BTreeMap<&str, (usize, &str)> = BTreeMap::new();
    for card in state.base.cards.values() {
        if let Some(id) = &card.repo_id {
            let entry = repos
                .entry(id)
                .or_insert((0, card.repo.as_deref().unwrap_or("")));
            entry.0 += 1;
        }
    }
    if let Some(wanted) = &state.config.repo {
        return repos
            .iter()
            .find(|(_, (_, name))| name == wanted)
            .map(|(id, _)| id.to_string())
            .ok_or_else(|| format!("repo \"{wanted}\" has no issues on this board"));
    }
    repos
        .iter()
        // Most cards wins; ties go to the alphabetically first name.
        .max_by(|(_, (n1, name1)), (_, (n2, name2))| n1.cmp(n2).then(name2.cmp(name1)))
        .map(|(id, _)| id.to_string())
        .ok_or_else(|| "no repository to create issues in: set `repo` in the config".into())
}
