//! GitHub-GraphQL-shaped response data, built from the domain types. The
//! shapes mirror what `queries.rs` selects (checked against the live API by
//! `scripts/check-schema.sh`); the contents are synthetic.

use serde_json::{Value, json};

use gh_board::types::{Card, IssueState, Meta, StateReason};

/// A second single-select issue field with a same-named option ("High"), so
/// priority must be matched by field id, not option name.
pub const EFFORT_FIELD_ID: &str = "f-effort";

fn state_name(s: IssueState) -> &'static str {
    match s {
        IssueState::Open => "OPEN",
        IssueState::Closed => "CLOSED",
    }
}

fn reason_name(r: StateReason) -> &'static str {
    match r {
        StateReason::Completed => "COMPLETED",
        StateReason::NotPlanned => "NOT_PLANNED",
        StateReason::Duplicate => "DUPLICATE",
        StateReason::Reopened => "REOPENED",
    }
}

/// A `CardIssue` node.
pub fn issue(c: &Card, priority_field_id: &str) -> Value {
    let mut values = vec![
        json!({ "optionId": "e-high", "field": { "id": EFFORT_FIELD_ID } }),
        // A value of some other field type: the inline fragment selects nothing.
        json!({}),
    ];
    if let Some(p) = &c.priority_id {
        values.push(json!({ "optionId": p, "field": { "id": priority_field_id } }));
    }
    json!({
        "__typename": "Issue",
        "id": c.issue_id,
        "number": c.number,
        "title": c.title,
        "url": c.url,
        "state": c.state.map(state_name),
        "stateReason": c.state_reason.map(reason_name),
        "repository": c.repo_id.as_ref().map(|id| json!({ "id": id, "nameWithOwner": c.repo })),
        "assignees": { "nodes": c.assignees.iter().map(|u| json!({ "id": u.id, "login": u.login })).collect::<Vec<_>>() },
        "labels": { "nodes": c.labels.iter().map(|l| json!({ "name": l })).collect::<Vec<_>>() },
        "issueFieldValues": { "nodes": values },
    })
}

/// A `CardItem` node.
pub fn item(c: &Card, priority_field_id: &str) -> Value {
    json!({
        "id": c.item_id,
        "fieldValueByName": c.status_id.as_ref().map(|s| json!({ "optionId": s })),
        "content": issue(c, priority_field_id),
    })
}

fn options(field: &gh_board::types::Field) -> Vec<Value> {
    field
        .options
        .iter()
        .map(|o| json!({ "id": o.id, "name": o.name }))
        .collect()
}

/// One page of the board query's `data`. `cursor` set means another page
/// follows.
pub fn page(meta: &Meta, items: Vec<Value>, cursor: Option<&str>) -> Value {
    let mut fields = vec![
        json!({}),
        json!({ "id": EFFORT_FIELD_ID, "name": "Effort", "options": [{ "id": "e-high", "name": "High" }] }),
    ];
    if let Some(pf) = &meta.priority_field {
        fields.insert(
            0,
            json!({ "id": pf.id, "name": "Priority", "options": options(pf) }),
        );
    }
    json!({
        "viewer": { "id": meta.viewer.id, "login": meta.viewer.login },
        "organization": {
            "issueFields": { "nodes": fields },
            "projectV2": {
                "id": meta.project_id,
                "title": meta.title,
                "field": { "id": meta.status_field.id, "options": options(&meta.status_field) },
                "items": {
                    "pageInfo": { "hasNextPage": cursor.is_some(), "endCursor": cursor },
                    "nodes": items,
                },
            },
        },
    })
}

/// A draft item: on the board, but not an issue.
pub fn draft(id: &str) -> Value {
    json!({ "id": id, "fieldValueByName": null, "content": { "__typename": "DraftIssue" } })
}
