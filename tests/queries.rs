//! GraphQL documents and variables.

mod common;

use common::*;
use gh_board::model::{Op, OpKind};
use gh_board::queries;
use gh_board::types::{IssueState, StateReason};
use serde_json::json;

#[test]
fn board_variables_carry_the_owner_project_status_field_and_cursor() {
    let r = queries::board(&config(), Some("cur"));
    assert_eq!(
        r.variables,
        json!({ "owner": "acme", "number": 1, "statusField": "Status", "after": "cur" })
    );
    let first = queries::board(&config(), None);
    assert_eq!(
        first.variables.get("after"),
        None,
        "null variables are left out"
    );
}

#[test]
fn documents_include_the_fragments_they_use() {
    let r = queries::board(&config(), None);
    assert!(r.query.contains("fragment CardItem"));
    assert!(r.query.contains("fragment CardIssue"));
    let r = queries::set_priority("I", "F", None);
    assert!(r.query.contains("fragment CardIssue"));
    assert!(
        !r.query.contains("fragment CardItem"),
        "an unused fragment is a GraphQL error"
    );
}

#[test]
fn next_cursor_follows_page_info() {
    assert_eq!(
        queries::next_cursor(&gh::page(&meta(), vec![], Some("c1"))),
        Some("c1".into())
    );
    assert_eq!(queries::next_cursor(&gh::page(&meta(), vec![], None)), None);
    assert_eq!(queries::next_cursor(&json!({})), None);
}

#[test]
fn assignee_changes_include_only_the_non_empty_halves_removal_first() {
    let add_only = queries::set_assignees("I", &["U1"], &[]);
    assert_eq!(add_only.variables, json!({ "issueId": "I", "add": ["U1"] }));
    assert!(!add_only.query.contains("removeAssignees"));
    assert!(!add_only.query.contains("$remove"));

    let both = queries::set_assignees("I", &["U1"], &["U2"]);
    let remove_at = both.query.find("removeAssignees").unwrap();
    let add_at = both.query.find("addAssignees").unwrap();
    assert!(
        remove_at < add_at,
        "add runs last, so its payload has both changes"
    );
}

#[test]
fn label_changes_use_the_labelable_mutations_removal_first() {
    let add_only = queries::set_labels("I", &["L1"], &[]);
    assert_eq!(add_only.variables, json!({ "issueId": "I", "add": ["L1"] }));
    assert!(
        add_only.query.contains(
            "added: addLabelsToLabelable(input: { labelableId: $issueId, labelIds: $add })"
        )
    );
    assert!(!add_only.query.contains("removeLabels"));

    let both = queries::set_labels("I", &["L1"], &["L2"]);
    assert_eq!(
        both.variables,
        json!({ "issueId": "I", "add": ["L1"], "remove": ["L2"] })
    );
    let remove_at = both
        .query
        .find("removed: removeLabelsFromLabelable")
        .unwrap();
    let add_at = both.query.find("added: addLabelsToLabelable").unwrap();
    assert!(
        remove_at < add_at,
        "add runs last, so its payload has both changes"
    );
    assert!(
        both.query
            .contains("labelable { ... on Issue { ...CardIssue } }")
    );
    assert!(
        !both.query.contains("Assignee"),
        "nothing of the assignee pair"
    );
}

#[test]
fn clearing_priority_sends_delete() {
    assert_eq!(
        queries::set_priority("I", "F", None).variables["fields"],
        json!([{ "fieldId": "F", "delete": true }])
    );
    assert_eq!(
        queries::set_priority("I", "F", Some("O")).variables["fields"],
        json!([{ "fieldId": "F", "singleSelectOptionId": "O" }])
    );
}

#[test]
fn create_sets_priority_only_when_there_is_one() {
    assert_eq!(
        queries::create_issue("R", "t", None)
            .variables
            .get("fields"),
        None
    );
    assert_eq!(
        queries::create_issue("R", "t", Some(("F", "O"))).variables["fields"],
        json!([{ "fieldId": "F", "singleSelectOptionId": "O" }])
    );
}

#[test]
fn set_state_closes_with_a_reason_or_reopens() {
    let close = queries::set_state("I", IssueState::Closed, Some(StateReason::NotPlanned), None);
    assert_eq!(
        close.variables,
        json!({ "issueId": "I", "reason": "NOT_PLANNED" })
    );
    assert!(close.query.contains("closeIssue"));
    let reopen = queries::set_state("I", IssueState::Open, None, None);
    assert_eq!(reopen.variables, json!({ "issueId": "I" }));
    assert!(reopen.query.contains("reopenIssue"));
}

#[test]
fn closing_as_a_duplicate_names_the_original() {
    let close = queries::set_state(
        "I",
        IssueState::Closed,
        Some(StateReason::Duplicate),
        Some("I_orig"),
    );
    assert_eq!(
        close.variables,
        json!({ "issueId": "I", "reason": "DUPLICATE", "duplicateOf": "I_orig" })
    );
    assert!(close.query.contains("duplicateIssueId: $duplicateOf"));
}

#[test]
fn set_body_sends_the_new_description() {
    let r = queries::set_body("I_1", "hello");
    assert!(r.query.contains("updateIssue"));
    assert_eq!(r.variables, json!({ "issueId": "I_1", "body": "hello" }));
}

#[test]
fn assignable_splits_name_with_owner() {
    assert_eq!(
        queries::assignable("acme/plans").variables,
        json!({ "owner": "acme", "name": "plans" })
    );
}

#[test]
fn labels_are_read_from_the_named_repository() {
    let r = queries::labels("acme/plans");
    assert_eq!(r.variables, json!({ "owner": "acme", "name": "plans" }));
    assert!(r.query.contains("labels(first: 100"));
}

#[test]
fn a_request_serialises_as_gh_expects_on_stdin() {
    let body = serde_json::to_value(queries::detail("I_1")).unwrap();
    assert_eq!(body["variables"], json!({ "id": "I_1" }));
    assert!(body["query"].as_str().unwrap().contains("node(id: $id)"));
}

fn op(kind: OpKind) -> Op {
    let mut op = Op::new("item1".into(), Some("issue1".into()), "desc".into(), kind);
    op.id = 1;
    op
}

#[test]
fn each_op_maps_to_its_request() {
    let (meta, config) = (meta(), config());
    let status = queries::for_op(
        &op(OpKind::Status {
            status_id: "s-prog".into(),
        }),
        &meta,
        &config,
    )
    .unwrap();
    assert_eq!(
        status.variables,
        json!({
            "projectId": "proj", "itemId": "item1", "fieldId": "f-status",
            "optionId": "s-prog", "statusField": "Status",
        })
    );
    let priority = queries::for_op(
        &op(OpKind::Priority {
            priority_id: Some("p-low".into()),
        }),
        &meta,
        &config,
    )
    .unwrap();
    assert_eq!(priority.variables["fields"][0]["fieldId"], "f-priority");
    let assign = queries::for_op(
        &op(OpKind::Assign {
            add: vec![me()],
            remove: vec![user("u-zed", "zed")],
        }),
        &meta,
        &config,
    )
    .unwrap();
    assert_eq!(
        assign.variables,
        json!({ "issueId": "issue1", "add": ["u-me"], "remove": ["u-zed"] })
    );
    let labels = queries::for_op(
        &op(OpKind::Label {
            add: vec![label("bug")],
            remove: vec![label("blog")],
        }),
        &meta,
        &config,
    )
    .unwrap();
    assert!(labels.query.contains("addLabelsToLabelable"));
    assert_eq!(
        labels.variables,
        json!({ "issueId": "issue1", "add": ["l-bug"], "remove": ["l-blog"] })
    );
    let comment =
        queries::for_op(&op(OpKind::Comment { body: "hi".into() }), &meta, &config).unwrap();
    assert!(comment.query.contains("addComment"));
}

#[test]
fn a_create_is_not_a_single_request() {
    let create = op(OpKind::Create {
        title: "t".into(),
        repo_id: "r".into(),
        status_id: None,
        priority_id: None,
        created_issue_id: None,
        created_issue_url: None,
    });
    assert!(queries::for_op(&create, &meta(), &config()).is_err());
}

#[test]
fn feedback_goes_to_gh_boards_repo_and_says_where_it_came_from() {
    let (owner, name) = queries::FEEDBACK_REPO;
    assert_eq!(
        queries::repository_id(owner, name).variables,
        json!({ "owner": "krisajenkins", "name": "gh-board" })
    );
    let footer = format!(
        "Filed from inside gh-board {} with `F`.",
        env!("CARGO_PKG_VERSION")
    );
    let r = queries::feedback("R", "Make it pink", "  \n");
    assert_eq!(r.variables["repoId"], "R");
    assert_eq!(r.variables["title"], "Make it pink");
    assert_eq!(
        r.variables["body"], footer,
        "no description: the footer alone"
    );
    let r = queries::feedback("R", "Make it pink", "\nPinker.\n");
    assert_eq!(
        r.variables["body"],
        format!("Pinker.\n\n---\n{footer}"),
        "a description goes above the footer"
    );
}
