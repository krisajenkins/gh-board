//! Pickers: small list popups for a card's status, priority, assignees and
//! open/closed state. A picker stores only what it is for and where the cursor
//! is; its choices are derived from the live state each time, so the assignee
//! ticks update as soon as a toggle is dispatched. Pure.
//!
//! The assignee picker can be filtered by typing, fuzzily as in the board's
//! search (`jdt` finds `@jdetter`), and offers "Assign to me" first.

use crate::model::{State, view};
use crate::search;
use crate::types::{Card, IssueState, ItemId, StateReason, User};
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
    /// An index into the menu's (filtered) choices.
    pub cursor: usize,
    /// What has been typed to narrow the choices (assignee picker only).
    pub filter: String,
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
    /// The filter typed so far, for a menu that can be filtered.
    pub filter: Option<String>,
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
        filter: None,
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
            let viewer = &meta.viewer;
            let users = card.repo_id.as_ref().and_then(|r| state.assignable.get(r));
            let toggle = |u: &User| Msg::ToggleAssignee {
                item_id: item_id.clone(),
                user: u.clone(),
            };
            let mut me_fields = user_fields(viewer);
            me_fields.push("me".into());
            let me = (
                me_fields,
                Choice {
                    label: format!("Assign to me (@{})", viewer.login),
                    marked: card.is_assigned(&viewer.id),
                    msg: toggle(viewer),
                },
            );
            let others = users
                .into_iter()
                .flatten()
                .filter(|u| u.id != viewer.id)
                .map(|u| {
                    (
                        user_fields(u),
                        Choice {
                            label: match u.name.as_deref().filter(|n| !n.is_empty()) {
                                Some(name) => format!("@{} ({name})", u.login),
                                None => format!("@{}", u.login),
                            },
                            marked: card.is_assigned(&u.id),
                            msg: toggle(u),
                        },
                    )
                });
            let terms = search::parse(&picker.filter);
            let choices = std::iter::once(me)
                .chain(others)
                .filter(|(fields, _)| search::matches(&terms, fields))
                .map(|(_, choice)| choice)
                .collect();
            Menu {
                title: format!("Assignees for {}", label(&card)),
                choices,
                mark: "✓",
                loading: users.is_none() && card.repo_id.is_some(),
                stays_open: true,
                filter: Some(picker.filter.clone()),
            }
        }
    })
}

/// What the assignee filter matches a user against: `@login`, and the
/// display name if there is one.
pub fn user_fields(user: &User) -> Vec<String> {
    let mut out = vec![format!("@{}", user.login)];
    out.extend(user.name.clone().filter(|n| !n.is_empty()));
    out
}

/// Where the cursor starts: on the current value, if there is one.
pub fn initial_cursor(state: &State, kind: PickerKind, item_id: &str) -> usize {
    let probe = Picker {
        kind,
        item_id: item_id.into(),
        cursor: 0,
        filter: String::new(),
    };
    match kind {
        PickerKind::Status | PickerKind::Priority => menu(state, &probe)
            .and_then(|m| m.choices.iter().position(|c| c.marked))
            .unwrap_or(0),
        PickerKind::Close | PickerKind::Assignee => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(query: &str, user: &User) -> bool {
        search::matches(&search::parse(query), &user_fields(user))
    }

    #[test]
    fn users_match_by_login_and_name_fuzzily_with_smart_case() {
        let jd = User {
            id: "u-jd".into(),
            login: "jdetter".into(),
            name: Some("John Detter".into()),
        };
        assert!(hit("jdt", &jd));
        assert!(hit("@jd", &jd));
        assert!(hit("john", &jd));
        assert!(hit("John", &jd));
        assert!(!hit("JOHN", &jd));
        assert!(!hit("'jdt", &jd));
        assert!(hit("'dett", &jd));
        assert!(!hit("xyz", &jd));
        assert!(hit("", &jd));
    }
}
