//! Pickers: small list popups for a card's status, priority, assignees,
//! labels and open/closed state. A picker stores only what it is for and where
//! the cursor is; its choices are derived from the live state each time, so
//! the assignee and label ticks update as soon as a toggle is dispatched. Pure.
//!
//! The assignee and label pickers are multiselects over a list fetched per
//! repository: filtered by typing, fuzzily as in the board's search (`jdt`
//! finds `@jdetter`), with a tick on each member the card has. The assignee
//! picker offers "Assign to me" first.
//!
//! The close picker's "Duplicate of…" opens the duplicate picker: every other
//! issue on the board, filtered by typing as the board's `/` search is.

use crate::model::{State, view};
use crate::search;
use crate::types::{Card, IssueState, ItemId, Label, StateReason, User};
use crate::update::Msg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKind {
    Status,
    Priority,
    /// Close an open issue as completed, not planned, or a duplicate.
    Close,
    /// Toggle assignees; stays open until dismissed.
    Assignee,
    /// Toggle labels; stays open until dismissed.
    Label,
    /// Pick the issue an issue duplicates, from the board's other issues.
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub kind: PickerKind,
    pub item_id: ItemId,
    /// An index into the menu's (filtered) choices.
    pub cursor: usize,
    /// What has been typed to narrow the choices (filterable pickers only).
    pub filter: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    /// The current value (`●`), or an assigned user (`✓`).
    pub marked: bool,
    pub pick: Pick,
}

/// What picking a choice does.
// Choices are built per frame and were always this size: boxing the Msg buys
// nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Send(Msg),
    /// Open another picker on the same card.
    Open(PickerKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub choices: Vec<Choice>,
    /// The mark for `marked` choices.
    pub mark: &'static str,
    /// The list to pick from (assignable users, labels) is still being
    /// fetched.
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
                    pick: Pick::Send(Msg::SetStatus {
                        item_id: item_id.clone(),
                        status_id: o.id.clone(),
                    }),
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
                    pick: Pick::Send(Msg::SetPriority {
                        item_id: item_id.clone(),
                        priority_id: Some(o.id.clone()),
                    }),
                })
                .collect();
            choices.push(Choice {
                label: "None".into(),
                marked: card.priority_id.is_none(),
                pick: Pick::Send(Msg::SetPriority {
                    item_id: item_id.clone(),
                    priority_id: None,
                }),
            });
            simple(format!("Priority for {}", label(&card)), choices)
        }
        PickerKind::Close => {
            let mut choices: Vec<Choice> = [
                ("Completed", StateReason::Completed),
                ("Not planned", StateReason::NotPlanned),
            ]
            .into_iter()
            .map(|(name, reason)| Choice {
                label: name.into(),
                marked: card.state == Some(IssueState::Closed) && card.state_reason == Some(reason),
                pick: Pick::Send(Msg::SetState {
                    item_id: item_id.clone(),
                    issue_state: IssueState::Closed,
                    reason: Some(reason),
                }),
            })
            .collect();
            choices.push(Choice {
                label: "Duplicate of…".into(),
                marked: card.state == Some(IssueState::Closed)
                    && card.state_reason == Some(StateReason::Duplicate),
                pick: Pick::Open(PickerKind::Duplicate),
            });
            simple(format!("Close {} as", label(&card)), choices)
        }
        PickerKind::Duplicate => {
            let pf = meta.priority_field.as_ref();
            let terms = search::parse(&picker.filter);
            let mut others: Vec<Card> = view(state)
                .cards
                .into_values()
                .filter(|c| c.item_id != item_id && c.issue_id.is_some() && !c.temp)
                .filter(|c| {
                    let priority = pf
                        .zip(c.priority_id.as_deref())
                        .and_then(|(f, id)| f.option_name(id));
                    search::matches(&terms, &search::fields(c, priority))
                })
                .collect();
            others.sort_by_key(|c| c.number);
            Menu {
                title: format!("Close {} as a duplicate of", label(&card)),
                choices: others
                    .into_iter()
                    .map(|c| Choice {
                        label: format!("{} {}", label(&c), c.title),
                        marked: false,
                        pick: Pick::Send(Msg::CloseAsDuplicate {
                            item_id: item_id.clone(),
                            original: c.item_id,
                        }),
                    })
                    .collect(),
                mark: "●",
                loading: false,
                stays_open: false,
                filter: Some(picker.filter.clone()),
            }
        }
        PickerKind::Assignee => {
            let viewer = &meta.viewer;
            let users = card
                .repo_id
                .as_deref()
                .and_then(|r| state.assignable.get(r));
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
                    pick: Pick::Send(toggle(viewer)),
                },
            );
            let others = users
                .unwrap_or_default()
                .iter()
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
                            pick: Pick::Send(toggle(u)),
                        },
                    )
                });
            multiselect(
                format!("Assignees for {}", label(&card)),
                std::iter::once(me).chain(others),
                &picker.filter,
                users.is_none() && card.repo_id.is_some(),
            )
        }
        PickerKind::Label => {
            let labels = card.repo_id.as_deref().and_then(|r| state.labels.get(r));
            let choices = labels.unwrap_or_default().iter().map(|l: &Label| {
                (
                    vec![l.name.clone()],
                    Choice {
                        label: l.name.clone(),
                        marked: card.has_label(&l.id),
                        pick: Pick::Send(Msg::ToggleLabel {
                            item_id: item_id.clone(),
                            label: l.clone(),
                        }),
                    },
                )
            });
            multiselect(
                format!("Labels for {}", label(&card)),
                choices,
                &picker.filter,
                labels.is_none() && card.repo_id.is_some(),
            )
        }
    })
}

/// A menu that toggles members of a set and stays open: each entry is the
/// text the filter matches against and its choice, ticked when the card has
/// it. `loading` while the list it offers is being fetched.
fn multiselect(
    title: String,
    entries: impl Iterator<Item = (Vec<String>, Choice)>,
    filter: &str,
    loading: bool,
) -> Menu {
    let terms = search::parse(filter);
    Menu {
        title,
        choices: entries
            .filter(|(fields, _)| search::matches(&terms, fields))
            .map(|(_, choice)| choice)
            .collect(),
        mark: "✓",
        loading,
        stays_open: true,
        filter: Some(filter.into()),
    }
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
        PickerKind::Close | PickerKind::Assignee | PickerKind::Label | PickerKind::Duplicate => 0,
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
