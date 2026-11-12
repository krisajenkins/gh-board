//! Print every GraphQL request the app can make, one per line as
//! `<name>\t<json>`, with made-up ids. `scripts/check-schema.sh` sends them to
//! GitHub: a validation error means a document is wrong; NOT_FOUND means it is
//! fine (and nothing was changed).

use gh_board::queries::{self, Request};
use gh_board::types::{Config, IssueState, StateReason};

fn main() {
    const ID: &str = "GH_BOARD_SCHEMA_CHECK_BOGUS_ID";
    let config = Config::new("gh-board-schema-check-no-such-org", 1);
    let requests: Vec<(&str, Request)> = vec![
        ("board", queries::board(&config, Some("cursor"))),
        ("detail", queries::detail(ID)),
        (
            "assignable",
            queries::assignable("gh-board-schema-check-no-such-org/repo"),
        ),
        ("item", queries::item(ID, "Status")),
        (
            "set_status",
            queries::set_status(ID, ID, ID, "opt", "Status"),
        ),
        (
            "close",
            queries::set_state(ID, IssueState::Closed, Some(StateReason::NotPlanned), None),
        ),
        (
            "close_duplicate",
            queries::set_state(
                ID,
                IssueState::Closed,
                Some(StateReason::Duplicate),
                Some(ID),
            ),
        ),
        (
            "reopen",
            queries::set_state(ID, IssueState::Open, None, None),
        ),
        ("set_priority", queries::set_priority(ID, ID, Some("opt"))),
        ("clear_priority", queries::set_priority(ID, ID, None)),
        ("assign_add", queries::set_assignees(ID, &[ID], &[])),
        ("assign_remove", queries::set_assignees(ID, &[], &[ID])),
        ("assign_both", queries::set_assignees(ID, &[ID], &[ID])),
        ("comment", queries::comment(ID, "schema check")),
        ("set_body", queries::set_body(ID, "schema check")),
        (
            "create_issue",
            queries::create_issue(ID, "schema check", Some((ID, "opt"))),
        ),
        ("add_to_project", queries::add_to_project(ID, ID, "Status")),
        (
            "repository_id",
            queries::repository_id("gh-board-schema-check-no-such-org", "repo"),
        ),
        (
            "feedback",
            queries::feedback(ID, "schema check", "schema check"),
        ),
    ];
    for (name, r) in requests {
        println!(
            "{name}\t{}",
            serde_json::to_string(&r).expect("serialisable")
        );
    }
}
