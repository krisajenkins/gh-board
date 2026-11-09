//! What the board knows about GitHub, already decoded from GraphQL.

use std::collections::BTreeMap;

/// A project item id, or `tmp:<n>` for a card still being created.
pub type ItemId = String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub login: String,
    /// Display name, where GitHub gave one (assignable users, not assignees).
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldOption {
    pub id: String,
    pub name: String,
}

/// A single-select field: the project's status, or the org's priority issue field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: String,
    pub options: Vec<FieldOption>,
}

impl Field {
    pub fn option_name(&self, id: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.name.as_str())
    }

    /// The id of the option with this name, ignoring case.
    pub fn option_id(&self, name: &str) -> Option<&str> {
        let name = name.to_lowercase();
        self.options
            .iter()
            .find(|o| o.name.to_lowercase() == name)
            .map(|o| o.id.as_str())
    }

    /// 0-based position of an option in its field, for rank-based styling.
    pub fn option_rank(&self, id: &str) -> Option<usize> {
        self.options.iter().position(|o| o.id == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateReason {
    Completed,
    NotPlanned,
    Duplicate,
    Reopened,
}

impl StateReason {
    pub fn describe(self) -> &'static str {
        match self {
            StateReason::Completed => "completed",
            StateReason::NotPlanned => "not planned",
            StateReason::Duplicate => "duplicate",
            StateReason::Reopened => "reopened",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Card {
    pub item_id: ItemId,
    pub issue_id: Option<String>,
    pub number: Option<u64>,
    pub title: String,
    pub url: Option<String>,
    pub state: Option<IssueState>,
    pub state_reason: Option<StateReason>,
    pub repo_id: Option<String>,
    /// `nameWithOwner`.
    pub repo: Option<String>,
    pub status_id: Option<String>,
    pub priority_id: Option<String>,
    pub assignees: Vec<User>,
    pub labels: Vec<String>,
    /// Still being created: exists only as an op.
    pub temp: bool,
    /// Derived: an operation on this card is unconfirmed.
    pub pending: bool,
    /// Derived: an operation on this card failed.
    pub failed: bool,
}

impl Card {
    /// How messages refer to the card: `#12`, or its title while it has no number.
    pub fn label(&self) -> String {
        match self.number {
            Some(n) => format!("#{n}"),
            None => format!("“{}”", self.title),
        }
    }

    pub fn is_assigned(&self, user_id: &str) -> bool {
        self.assignees.iter().any(|u| u.id == user_id)
    }
}

/// Cards by id, plus the order GitHub gave them in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cards {
    pub cards: BTreeMap<ItemId, Card>,
    pub order: Vec<ItemId>,
}

impl Cards {
    /// Insert or replace a card, appending it to the order if it is new.
    pub fn put(&mut self, card: Card) {
        if !self.cards.contains_key(&card.item_id) {
            self.order.push(card.item_id.clone());
        }
        self.cards.insert(card.item_id.clone(), card);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Card> {
        self.order.iter().filter_map(|id| self.cards.get(id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub viewer: User,
    pub project_id: String,
    pub title: String,
    pub status_field: Field,
    pub priority_field: Option<Field>,
}

/// A whole board, as loaded (every page already assembled).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub meta: Meta,
    pub cards: Cards,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailData {
    pub body: String,
    /// The latest comments, oldest first.
    pub comments: Vec<Comment>,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The organization that owns the project.
    pub owner: String,
    pub project: u64,
    /// `owner/name` new issues go into. Default: the repo most cards live in.
    pub repo: Option<String>,
    pub status_field: String,
    pub priority_field: String,
    pub hidden_statuses: Vec<String>,
    /// Priority option to start filtered to, by name, any case.
    pub priority: Option<String>,
}

impl Config {
    pub fn new(owner: impl Into<String>, project: u64) -> Self {
        Config {
            owner: owner.into(),
            project,
            repo: None,
            status_field: "Status".into(),
            priority_field: "Priority".into(),
            hidden_statuses: vec!["Done".into()],
            priority: None,
        }
    }
}
