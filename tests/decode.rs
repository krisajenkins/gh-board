//! GraphQL response data -> domain types.

mod common;

use common::*;
use gh_board::decode;
use gh_board::model::OpKind;
use gh_board::types::{Card, Comment, IssueState, StateReason, User};
use gh_board::update::MutationResult;
use proptest::prelude::*;
use serde_json::{Value, json};

const PF: &str = "f-priority";

fn standard_page(cursor: Option<&str>) -> Value {
    let items = items().iter().map(|c| gh::item(c, PF)).collect();
    gh::page(&meta(), items, cursor)
}

#[test]
fn a_board_decodes_to_its_meta_and_cards() {
    let (board, skipped) = decode::board(&[standard_page(None)], &config()).unwrap();
    assert_eq!(board.meta, meta());
    assert_eq!(board.cards, items());
    assert_eq!(skipped, 0);
}

#[test]
fn pages_are_concatenated_in_order() {
    let all: Vec<Card> = items().iter().cloned().collect();
    let page = |cards: &[Card]| {
        gh::page(
            &meta(),
            cards.iter().map(|c| gh::item(c, PF)).collect(),
            None,
        )
    };
    let (board, _) = decode::board(&[page(&all[..2]), page(&all[2..])], &config()).unwrap();
    assert_eq!(board.cards, items());
}

#[test]
fn non_issue_items_are_skipped_and_counted() {
    let page = gh::page(
        &meta(),
        vec![gh::item(&card(1), PF), gh::draft("itemD")],
        None,
    );
    let (board, skipped) = decode::board(&[page], &config()).unwrap();
    assert_eq!(board.cards.order, vec!["item1"]);
    assert_eq!(skipped, 1);
}

#[test]
fn a_missing_priority_field_leaves_priority_off_rather_than_failing() {
    let mut config = config();
    config.priority_field = "Urgency".into();
    let (board, _) = decode::board(&[standard_page(None)], &config).unwrap();
    assert_eq!(board.meta.priority_field, None);
    assert!(board.cards.iter().all(|c| c.priority_id.is_none()));
}

#[test]
fn priority_is_matched_by_field_id_not_option_name() {
    // Effort has a "High" option too, set on every fixture issue.
    let (board, _) = decode::board(&[standard_page(None)], &config()).unwrap();
    assert_eq!(board.cards.cards["item1"].priority_id, None);
}

#[test]
fn a_missing_project_or_status_field_is_an_error() {
    let mut page = standard_page(None);
    page["organization"]["projectV2"]["field"] = Value::Null;
    assert_eq!(
        decode::board(&[page.clone()], &config()).unwrap_err(),
        "project has no single-select field named \"Status\""
    );
    page["organization"]["projectV2"] = Value::Null;
    assert_eq!(
        decode::board(&[page.clone()], &config()).unwrap_err(),
        "project not found"
    );
    page["organization"] = Value::Null;
    assert!(
        decode::board(&[page], &config())
            .unwrap_err()
            .starts_with("organization not found")
    );
}

#[test]
fn null_connections_and_nodes_are_tolerated() {
    let mut node = gh::issue(&card(1), PF);
    node["assignees"] = Value::Null;
    node["labels"] = json!({ "nodes": [null, { "name": "x" }] });
    let c = decode::issue(&node, Some(PF)).unwrap();
    assert!(c.assignees.is_empty());
    assert_eq!(c.labels, vec!["x"]);
}

#[test]
fn close_and_reopen_responses_decode_to_issue_fields() {
    let state = OpKind::State {
        issue_state: IssueState::Closed,
        state_reason: None,
        duplicate_of: None,
    };
    let closed = Card {
        state: Some(IssueState::Closed),
        state_reason: Some(StateReason::Completed),
        ..card(1)
    };
    let data = json!({ "closeIssue": { "issue": gh::issue(&closed, PF) } });
    let Ok(MutationResult::Card(c)) = decode::mutation(&state, &data, Some(PF)) else {
        panic!()
    };
    assert_eq!(
        (c.state, c.state_reason),
        (Some(IssueState::Closed), Some(StateReason::Completed))
    );
    assert_eq!(c.item_id, "", "issue fields only");

    let reopened = Card {
        state_reason: Some(StateReason::Reopened),
        ..card(1)
    };
    let data = json!({ "reopenIssue": { "issue": gh::issue(&reopened, PF) } });
    let Ok(MutationResult::Card(c)) = decode::mutation(&state, &data, Some(PF)) else {
        panic!()
    };
    assert_eq!(c.state, Some(IssueState::Open));
}

#[test]
fn an_assign_response_is_read_from_the_mutation_that_ran_last() {
    let kind = OpKind::Assign {
        add: vec![],
        remove: vec![],
    };
    let before = card(1);
    let after = Card {
        assignees: vec![user("u-amy", "amy")],
        ..card(1)
    };
    let data = json!({
        "removed": { "assignable": gh::issue(&before, PF) },
        "added": { "assignable": gh::issue(&after, PF) },
    });
    let Ok(MutationResult::Card(c)) = decode::mutation(&kind, &data, Some(PF)) else {
        panic!()
    };
    assert_eq!(c.assignees, after.assignees);
}

#[test]
fn a_status_response_is_a_whole_card() {
    let kind = OpKind::Status {
        status_id: "s-prog".into(),
    };
    let moved = Card {
        status_id: Some("s-prog".into()),
        ..card(1)
    };
    let data =
        json!({ "updateProjectV2ItemFieldValue": { "projectV2Item": gh::item(&moved, PF) } });
    assert_eq!(
        decode::mutation(&kind, &data, Some(PF)),
        Ok(MutationResult::Card(moved))
    );
}

#[test]
fn a_response_without_the_payload_is_an_error() {
    let kind = OpKind::Priority { priority_id: None };
    assert_eq!(
        decode::mutation(&kind, &json!({ "setIssueFieldValue": null }), Some(PF)),
        Err("no issue in response".into())
    );
}

#[test]
fn body_and_comment_responses() {
    let data = json!({ "updateIssue": { "issue": { "id": "I", "body": "new" } } });
    assert_eq!(
        decode::mutation(&OpKind::Body { body: "x".into() }, &data, None),
        Ok(MutationResult::Body("new".into()))
    );
    let data = json!({ "addComment": { "commentEdge": { "node": {
        "id": "C", "body": "hi", "createdAt": "2026-01-02T00:00:00Z", "author": { "login": "al" }
    } } } });
    assert_eq!(
        decode::mutation(&OpKind::Comment { body: "x".into() }, &data, None),
        Ok(MutationResult::Comment(Comment {
            id: "C".into(),
            author: "al".into(),
            body: "hi".into(),
            created_at: "2026-01-02T00:00:00Z".into(),
        }))
    );
}

#[test]
fn assignable_users_are_sorted_case_insensitively_and_keep_their_names() {
    let data = json!({ "repository": { "assignableUsers": { "nodes": [
        { "id": "U_bob", "login": "bob", "name": "" },
        { "id": "U_a", "login": "Aaron", "name": "Aaron A" },
    ] } } });
    let users = decode::assignable(&data).unwrap();
    assert_eq!(
        users,
        vec![
            User {
                id: "U_a".into(),
                login: "Aaron".into(),
                name: Some("Aaron A".into())
            },
            User {
                id: "U_bob".into(),
                login: "bob".into(),
                name: None
            },
        ]
    );
    assert_eq!(
        decode::assignable(&json!({ "repository": null })),
        Err("repository not found".into())
    );
}

#[test]
fn detail_reads_body_and_comments() {
    let data = json!({ "node": { "body": "b", "comments": { "totalCount": 3, "nodes": [
        { "id": "C", "body": "x", "createdAt": "2026-01-02T00:00:00Z", "author": { "login": "al" } }
    ] } } });
    let d = decode::detail(&data).unwrap();
    assert_eq!(d.body, "b");
    assert_eq!(d.total, 3);
    assert_eq!(d.comments.len(), 1);
    assert_eq!(
        decode::detail(&json!({ "node": null })),
        Err("issue not found".into())
    );
}

#[test]
fn a_comment_by_a_deleted_user_is_attributed_to_ghost() {
    let c = decode::comment(&json!({ "id": "C", "body": "x", "author": null })).unwrap();
    assert_eq!(c.author, "ghost");
}

#[test]
fn create_steps() {
    let data =
        json!({ "createIssue": { "issue": { "id": "I_9", "number": 9, "url": "https://x/9" } } });
    assert_eq!(
        decode::created_issue(&data),
        Ok(("I_9".into(), Some("https://x/9".into())))
    );
    let data = json!({ "addProjectV2ItemById": { "item": gh::item(&card(9), PF) } });
    assert_eq!(decode::added_item(&data, Some(PF)), Ok(card(9)));
}

#[test]
fn a_reread_item_that_is_gone_is_none() {
    assert_eq!(
        decode::item_reread(&json!({ "node": null }), Some(PF)),
        Ok(None)
    );
    assert_eq!(
        decode::item_reread(&json!({ "node": {} }), Some(PF)),
        Ok(None)
    );
    let data = json!({ "node": gh::item(&card(1), PF) });
    assert_eq!(decode::item_reread(&data, Some(PF)), Ok(Some(card(1))));
}

// Round trip ------------------------------------------------------------------

fn opt_id(prefix: &'static str) -> impl Strategy<Value = Option<String>> {
    prop::option::of((0..5u8).prop_map(move |n| format!("{prefix}{n}")))
}

prop_compose! {
    fn arb_card()(
        n in 1..10_000u64,
        number in prop::option::of(1..10_000u64),
        title in ".*",
        url in prop::option::of("https://[a-z/]{1,20}"),
        state in prop::option::of(prop_oneof![Just(IssueState::Open), Just(IssueState::Closed)]),
        state_reason in prop::option::of(prop_oneof![
            Just(StateReason::Completed),
            Just(StateReason::NotPlanned),
            Just(StateReason::Duplicate),
            Just(StateReason::Reopened),
        ]),
        repo in prop::option::of(("R[0-9]{1,3}", "[a-z]{1,8}/[a-z]{1,8}")),
        status_id in opt_id("s"),
        priority_id in opt_id("p"),
        assignees in prop::collection::vec(("U[0-9]{1,4}", "[a-zA-Z0-9-]{1,12}"), 0..4),
        labels in prop::collection::vec(".{0,12}", 0..4),
    ) -> Card {
        Card {
            item_id: format!("item{n}"),
            issue_id: Some(format!("issue{n}")),
            number,
            title,
            url,
            state,
            state_reason,
            repo_id: repo.as_ref().map(|r| r.0.clone()),
            repo: repo.map(|r| r.1),
            status_id,
            priority_id,
            assignees: assignees.into_iter().map(|(id, login)| User { id, login, name: None }).collect(),
            labels,
            ..Card::default()
        }
    }
}

proptest! {
    /// Any card GitHub could describe decodes back to itself.
    #[test]
    fn any_card_round_trips_through_the_api_shape(card in arb_card()) {
        prop_assert_eq!(decode::item(&gh::item(&card, PF), Some(PF)), Ok(Some(card)));
    }

    /// The decoder never panics on arbitrary JSON, however malformed.
    #[test]
    fn decoding_garbage_is_an_error_not_a_panic(v in arb_json()) {
        let _ = decode::item(&v, Some(PF));
        let _ = decode::board(std::slice::from_ref(&v), &config());
        let _ = decode::detail(&v);
        let _ = decode::assignable(&v);
        let _ = decode::mutation(&OpKind::Status { status_id: "s".into() }, &v, Some(PF));
    }
}

fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        "[a-z]{0,6}".prop_map(Value::String),
    ];
    let keys = prop_oneof![
        Just("id"),
        Just("nodes"),
        Just("organization"),
        Just("projectV2"),
        Just("content"),
        Just("__typename"),
        Just("node"),
        Just("items"),
        Just("field"),
        Just("options"),
    ];
    leaf.prop_recursive(4, 32, 6, move |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(keys.clone().prop_map(String::from), inner, 0..4)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}
