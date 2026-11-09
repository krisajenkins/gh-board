//! Pickers: small list popups for a card's status, priority, assignees and
//! open/closed state. A picker stores only what it is for and where the cursor
//! is; its choices are derived from the live state each time, so the assignee
//! ticks update as soon as a toggle is dispatched. Pure.

use crate::model::{State, view};
use crate::types::{Card, IssueState, ItemId, StateReason};
use crate::update::Msg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKind {
    Status,
    Priority,
    /// Close an open issue as completed or not planned.
    Close,
    /// Toggle assignees; stays open until dismissed.
    Assignee,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub kind: PickerKind,
    pub item_id: ItemId,
    pub cursor: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    /// The current value (`●`), or an assigned user (`✓`).
    pub marked: bool,
    pub msg: Msg,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub choices: Vec<Choice>,
    /// The mark for `marked` choices.
    pub mark: &'static str,
    /// Assignable users are still being fetched.
    pub loading: bool,
    /// Whether picking leaves the picker open.
    pub stays_open: bool,
}

fn label(card: &Card) -> String {
    card.label()
}

/// The picker's choices against the current state. `None` when its card has
/// gone, or the board has no metadata yet.
pub fn menu(state: &State, picker: &Picker) -> Option<Menu> {
    let meta = state.meta.as_ref()?;
    let card = view(state).cards.remove(&picker.item_id)?;
    let item_id = card.item_id.clone();
    let simple = |title: String, choices: Vec<Choice>| Menu {
        title,
        choices,
        mark: "●",
        loading: false,
        stays_open: false,
    };
    Some(match picker.kind {
        PickerKind::Status => simple(
            format!("Status for {}", label(&card)),
            meta.status_field
                .options
                .iter()
                .map(|o| Choice {
                    label: o.name.clone(),
                    marked: card.status_id.as_deref() == Some(&o.id),
                    msg: Msg::SetStatus {
                        item_id: item_id.clone(),
                        status_id: o.id.clone(),
                    },
                })
                .collect(),
        ),
        PickerKind::Priority => {
            let options = meta
                .priority_field
                .as_ref()
                .map(|f| f.options.as_slice())
                .unwrap_or_default();
            let mut choices: Vec<Choice> = options
                .iter()
                .map(|o| Choice {
                    label: o.name.clone(),
                    marked: card.priority_id.as_deref() == Some(&o.id),
                    msg: Msg::SetPriority {
                        item_id: item_id.clone(),
                        priority_id: Some(o.id.clone()),
                    },
                })
                .collect();
            choices.push(Choice {
                label: "None".into(),
                marked: card.priority_id.is_none(),
                msg: Msg::SetPriority {
                    item_id: item_id.clone(),
                    priority_id: None,
                },
            });
            simple(format!("Priority for {}", label(&card)), choices)
        }
        PickerKind::Close => simple(
            format!("Close {} as", label(&card)),
            [
                ("Completed", StateReason::Completed),
                ("Not planned", StateReason::NotPlanned),
            ]
            .into_iter()
            .map(|(name, reason)| Choice {
                label: name.into(),
                marked: card.state == Some(IssueState::Closed) && card.state_reason == Some(reason),
                msg: Msg::SetState {
                    item_id: item_id.clone(),
                    issue_state: IssueState::Closed,
                    reason: Some(reason),
                },
            })
            .collect(),
        ),
        PickerKind::Assignee => {
            let users = card.repo_id.as_ref().and_then(|r| state.assignable.get(r));
            let choices = users
                .into_iter()
                .flatten()
                .map(|u| Choice {
                    label: match u.name.as_deref().filter(|n| !n.is_empty()) {
                        Some(name) => format!("@{} ({name})", u.login),
                        None => format!("@{}", u.login),
                    },
                    marked: card.is_assigned(&u.id),
                    msg: Msg::ToggleAssignee {
                        item_id: item_id.clone(),
                        user: u.clone(),
                    },
                })
                .collect();
            Menu {
                title: format!("Assignees for {}", label(&card)),
                choices,
                mark: "✓",
                loading: users.is_none() && card.repo_id.is_some(),
                stays_open: true,
            }
        }
    })
}

/// Where the cursor starts: on the current value, if there is one.
pub fn initial_cursor(state: &State, kind: PickerKind, item_id: &str) -> usize {
    let probe = Picker {
        kind,
        item_id: item_id.into(),
        cursor: 0,
    };
    match kind {
        PickerKind::Status | PickerKind::Priority => menu(state, &probe)
            .and_then(|m| m.choices.iter().position(|c| c.marked))
            .unwrap_or(0),
        PickerKind::Close | PickerKind::Assignee => 0,
    }
}
