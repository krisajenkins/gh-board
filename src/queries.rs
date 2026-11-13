//! GraphQL documents and their variables. Pure: every function returns a
//! [`Request`] and nothing else.
//!
//! Two shapes are shared so that one decoder serves every response:
//!
//! - `CardIssue` — the issue fields a card shows (`decode::issue`)
//! - `CardItem`  — a project item: its status plus its `CardIssue` (`decode::item`)
//!
//! Mutations select one of them in their payload, so a response can replace
//! the cached card wholesale rather than the cache guessing what the server
//! did.
//!
//! Priority is an org-level *issue field*, not a project field: it is read
//! from `issueFieldValues` on the issue and written with `setIssueFieldValue`.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::model::{Op, OpKind};
use crate::types::{Config, IssueState, Member, Meta, StateReason};

/// A GraphQL request, serialised as the JSON body `gh api graphql --input -`
/// reads from stdin. (Never use `-F`: it reads `@foo` as a file.)
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub query: String,
    pub variables: Value,
}

/// Build a request, leaving out variables that are `null`.
fn request(query: impl Into<String>, variables: Value) -> Request {
    let variables = match variables {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .filter(|(_, v)| !v.is_null())
                .collect::<Map<_, _>>(),
        ),
        v => v,
    };
    Request {
        query: query.into(),
        variables,
    }
}

const CARD_ISSUE: &str = r#"
fragment CardIssue on Issue {
  id number title url state stateReason
  repository { id nameWithOwner }
  assignees(first: 20) { nodes { id login } }
  labels(first: 20) { nodes { id name } }
  issueFieldValues(first: 20) {
    nodes {
      ... on IssueFieldSingleSelectValue { optionId field { ... on IssueFieldSingleSelect { id } } }
    }
  }
}
"#;

const CARD_ITEM: &str = r#"
fragment CardItem on ProjectV2Item {
  id
  fieldValueByName(name: $statusField) { ... on ProjectV2ItemFieldSingleSelectValue { optionId } }
  content { __typename ...CardIssue }
}
"#;

fn with_issue(doc: &str) -> String {
    format!("{doc}{CARD_ISSUE}")
}

fn with_item(doc: &str) -> String {
    format!("{doc}{CARD_ITEM}{CARD_ISSUE}")
}

const BOARD: &str = r#"
query($owner: String!, $number: Int!, $statusField: String!, $after: String) {
  viewer { id login }
  organization(login: $owner) {
    issueFields(first: 50) {
      nodes { ... on IssueFieldSingleSelect { id name options { id name } } }
    }
    projectV2(number: $number) {
      id title
      field(name: $statusField) { ... on ProjectV2SingleSelectField { id options { id name } } }
      items(first: 100, after: $after, query: "is:issue") {
        pageInfo { hasNextPage endCursor }
        nodes { ...CardItem }
      }
    }
  }
}
"#;

/// One page of the board. `after` is the cursor from the previous page.
pub fn board(config: &Config, after: Option<&str>) -> Request {
    request(
        with_item(BOARD),
        json!({
            "owner": config.owner,
            "number": config.project,
            "statusField": config.status_field,
            "after": after,
        }),
    )
}

/// The cursor for the next page of a board response, or `None` when it was
/// the last.
pub fn next_cursor(data: &Value) -> Option<String> {
    let info = data.pointer("/organization/projectV2/items/pageInfo")?;
    if info.get("hasNextPage")?.as_bool()? {
        info.get("endCursor")?.as_str().map(String::from)
    } else {
        None
    }
}

const DETAIL: &str = r#"
query($id: ID!) {
  node(id: $id) {
    ... on Issue {
      body
      comments(last: 50) { totalCount nodes { id body createdAt author { login } } }
    }
  }
}
"#;

/// An issue's body and most recent comments.
pub fn detail(issue_id: &str) -> Request {
    request(DETAIL, json!({ "id": issue_id }))
}

const ASSIGNABLE: &str = r#"
query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    assignableUsers(first: 100) { nodes { id login name } }
  }
}
"#;

/// A query on one repository, named `owner/name`.
fn on_repo(doc: &str, name_with_owner: &str) -> Request {
    let (owner, name) = name_with_owner
        .split_once('/')
        .unwrap_or(("", name_with_owner));
    request(doc, json!({ "owner": owner, "name": name }))
}

/// The users who may be assigned issues in a repository (`owner/name`).
pub fn assignable(name_with_owner: &str) -> Request {
    on_repo(ASSIGNABLE, name_with_owner)
}

const LABELS: &str = r#"
query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    labels(first: 100, orderBy: { field: NAME, direction: ASC }) { nodes { id name } }
  }
}
"#;

/// A repository's labels (`owner/name`).
pub fn labels(name_with_owner: &str) -> Request {
    on_repo(LABELS, name_with_owner)
}

const SET_STATUS: &str = r#"
mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $optionId: String!, $statusField: String!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId, itemId: $itemId, fieldId: $fieldId, value: { singleSelectOptionId: $optionId }
  }) {
    projectV2Item { ...CardItem }
  }
}
"#;

pub fn set_status(
    project_id: &str,
    item_id: &str,
    field_id: &str,
    option_id: &str,
    status_field: &str,
) -> Request {
    request(
        with_item(SET_STATUS),
        json!({
            "projectId": project_id,
            "itemId": item_id,
            "fieldId": field_id,
            "optionId": option_id,
            "statusField": status_field,
        }),
    )
}

const ITEM: &str = r#"
query($id: ID!, $statusField: String!) {
  node(id: $id) { ... on ProjectV2Item { ...CardItem } }
}
"#;

/// One project item, re-read: used after a change that a project workflow may
/// follow up on (closing an issue moves it to Done, and vice versa).
pub fn item(item_id: &str, status_field: &str) -> Request {
    request(
        with_item(ITEM),
        json!({ "id": item_id, "statusField": status_field }),
    )
}

const CLOSE: &str = r#"
mutation($issueId: ID!, $reason: IssueClosedStateReason, $duplicateOf: ID) {
  closeIssue(input: { issueId: $issueId, stateReason: $reason, duplicateIssueId: $duplicateOf }) {
    issue { ...CardIssue }
  }
}
"#;

const REOPEN: &str = r#"
mutation($issueId: ID!) {
  reopenIssue(input: { issueId: $issueId }) { issue { ...CardIssue } }
}
"#;

/// GraphQL's name for a close reason. `REOPENED` is not one.
fn reason_name(r: StateReason) -> Option<&'static str> {
    match r {
        StateReason::Completed => Some("COMPLETED"),
        StateReason::NotPlanned => Some("NOT_PLANNED"),
        StateReason::Duplicate => Some("DUPLICATE"),
        StateReason::Reopened => None,
    }
}

/// Close an issue, with a reason, or reopen it. Closing as a duplicate names
/// the original by its issue id.
pub fn set_state(
    issue_id: &str,
    issue_state: IssueState,
    reason: Option<StateReason>,
    duplicate_of: Option<&str>,
) -> Request {
    match issue_state {
        IssueState::Open => request(with_issue(REOPEN), json!({ "issueId": issue_id })),
        IssueState::Closed => {
            let mut vars = json!({ "issueId": issue_id, "reason": reason.and_then(reason_name) });
            if let Some(original) = duplicate_of {
                vars["duplicateOf"] = json!(original);
            }
            request(with_issue(CLOSE), vars)
        }
    }
}

const SET_PRIORITY: &str = r#"
mutation($issueId: ID!, $fields: [IssueFieldCreateOrUpdateInput!]!) {
  setIssueFieldValue(input: { issueId: $issueId, issueFields: $fields }) {
    issue { ...CardIssue }
  }
}
"#;

/// Set a single-select issue field, or clear it when `option_id` is `None`.
pub fn set_priority(issue_id: &str, field_id: &str, option_id: Option<&str>) -> Request {
    let field = match option_id {
        Some(o) => json!({ "fieldId": field_id, "singleSelectOptionId": o }),
        None => json!({ "fieldId": field_id, "delete": true }),
    };
    request(
        with_issue(SET_PRIORITY),
        json!({ "issueId": issue_id, "fields": [field] }),
    )
}

/// The pair of mutations that add members to and remove them from one of an
/// issue's sets (its assignees, its labels), and the names of their inputs.
struct SetMutations {
    add: &'static str,
    remove: &'static str,
    /// The input naming the issue, and the payload field holding it.
    subject: &'static str,
    /// The input listing the members' ids.
    ids: &'static str,
}

const ASSIGNEES: SetMutations = SetMutations {
    add: "addAssigneesToAssignable",
    remove: "removeAssigneesFromAssignable",
    subject: "assignable",
    ids: "assigneeIds",
};

const LABEL_SET: SetMutations = SetMutations {
    add: "addLabelsToLabelable",
    remove: "removeLabelsFromLabelable",
    subject: "labelable",
    ids: "labelIds",
};

/// Add and/or remove members of an issue's set in one request, aliased
/// `removed` and `added`. Root mutation fields run in order, so the last
/// one's payload reflects both changes; only the non-empty halves are
/// included, and the response is decoded from whichever ran last.
fn change_set(m: &SetMutations, issue_id: &str, add_ids: &[&str], remove_ids: &[&str]) -> Request {
    let SetMutations {
        add,
        remove,
        subject,
        ids,
    } = m;
    let payload = format!("{{ {subject} {{ ... on Issue {{ ...CardIssue }} }} }}");
    let mut params = vec!["$issueId: ID!"];
    let mut fields = Vec::new();
    let mut variables = Map::new();
    variables.insert("issueId".into(), json!(issue_id));
    if !remove_ids.is_empty() {
        params.push("$remove: [ID!]!");
        fields.push(format!(
            "  removed: {remove}(input: {{ {subject}Id: $issueId, {ids}: $remove }}) {payload}"
        ));
        variables.insert("remove".into(), json!(remove_ids));
    }
    if !add_ids.is_empty() {
        params.push("$add: [ID!]!");
        fields.push(format!(
            "  added: {add}(input: {{ {subject}Id: $issueId, {ids}: $add }}) {payload}"
        ));
        variables.insert("add".into(), json!(add_ids));
    }
    let query = format!(
        "mutation({}) {{\n{}\n}}\n{CARD_ISSUE}",
        params.join(", "),
        fields.join("\n")
    );
    request(query, Value::Object(variables))
}

/// Add and/or remove assignees, by user id.
pub fn set_assignees(issue_id: &str, add_ids: &[&str], remove_ids: &[&str]) -> Request {
    change_set(&ASSIGNEES, issue_id, add_ids, remove_ids)
}

/// Add and/or remove labels, by label id.
pub fn set_labels(issue_id: &str, add_ids: &[&str], remove_ids: &[&str]) -> Request {
    change_set(&LABEL_SET, issue_id, add_ids, remove_ids)
}

const COMMENT: &str = r#"
mutation($issueId: ID!, $body: String!) {
  addComment(input: { subjectId: $issueId, body: $body }) {
    commentEdge { node { id body createdAt author { login } } }
  }
}
"#;

pub fn comment(issue_id: &str, body: &str) -> Request {
    request(COMMENT, json!({ "issueId": issue_id, "body": body }))
}

const SET_BODY: &str = r#"
mutation($issueId: ID!, $body: String!) {
  updateIssue(input: { id: $issueId, body: $body }) {
    issue { id body }
  }
}
"#;

/// Replace an issue's description.
pub fn set_body(issue_id: &str, body: &str) -> Request {
    request(SET_BODY, json!({ "issueId": issue_id, "body": body }))
}

const CREATE_ISSUE: &str = r#"
mutation($repoId: ID!, $title: String!, $fields: [IssueFieldCreateOrUpdateInput!]) {
  createIssue(input: { repositoryId: $repoId, title: $title, issueFields: $fields }) {
    issue { id number url }
  }
}
"#;

/// Create an issue, with its priority set in the same call when there is one.
pub fn create_issue(repo_id: &str, title: &str, priority: Option<(&str, &str)>) -> Request {
    let fields = priority.map(
        |(field_id, option_id)| json!([{ "fieldId": field_id, "singleSelectOptionId": option_id }]),
    );
    request(
        CREATE_ISSUE,
        json!({ "repoId": repo_id, "title": title, "fields": fields }),
    )
}

/// Where `F` files feedback about gh-board itself, as `(owner, name)`.
pub const FEEDBACK_REPO: (&str, &str) = ("krisajenkins", "gh-board");

const REPOSITORY_ID: &str = r#"
query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) { id }
}
"#;

/// A repository's node id, which `createIssue` needs.
pub fn repository_id(owner: &str, name: &str) -> Request {
    request(REPOSITORY_ID, json!({ "owner": owner, "name": name }))
}

const FEEDBACK: &str = r#"
mutation($repoId: ID!, $title: String!, $body: String!) {
  createIssue(input: { repositoryId: $repoId, title: $title, body: $body }) {
    issue { id url }
  }
}
"#;

/// File feedback about gh-board as an issue. The body is the user's
/// description (if any) above a footer saying where it came from, so the
/// version is on record.
pub fn feedback(repo_id: &str, title: &str, description: &str) -> Request {
    let footer = format!(
        "Filed from inside gh-board {} with `F`.",
        env!("CARGO_PKG_VERSION")
    );
    let body = match description.trim() {
        "" => footer,
        d => format!("{d}\n\n---\n{footer}"),
    };
    request(
        FEEDBACK,
        json!({ "repoId": repo_id, "title": title, "body": body }),
    )
}

const ADD_TO_PROJECT: &str = r#"
mutation($projectId: ID!, $contentId: ID!, $statusField: String!) {
  addProjectV2ItemById(input: { projectId: $projectId, contentId: $contentId }) {
    item { ...CardItem }
  }
}
"#;

/// Add an issue to the project. Idempotent on GitHub's side, which is what
/// makes retrying a half-finished create safe.
pub fn add_to_project(project_id: &str, content_id: &str, status_field: &str) -> Request {
    request(
        with_item(ADD_TO_PROJECT),
        json!({ "projectId": project_id, "contentId": content_id, "statusField": status_field }),
    )
}

fn ids(members: &[impl Member]) -> Vec<&str> {
    members.iter().map(Member::id).collect()
}

/// The single request that carries out an op. `Err` for a create, which is
/// several requests (`create_issue`, `add_to_project`, `set_status`) the
/// shell runs in turn, and for an op the board cannot express.
pub fn for_op(op: &Op, meta: &Meta, config: &Config) -> Result<Request, String> {
    let issue_id = || {
        op.issue_id
            .as_deref()
            .ok_or_else(|| format!("{}: the card has no issue", op.desc))
    };
    Ok(match &op.kind {
        OpKind::Status { status_id } => set_status(
            &meta.project_id,
            &op.item_id,
            &meta.status_field.id,
            status_id,
            &config.status_field,
        ),
        OpKind::Priority { priority_id } => {
            let field = meta
                .priority_field
                .as_ref()
                .ok_or("this board has no priority field")?;
            set_priority(issue_id()?, &field.id, priority_id.as_deref())
        }
        OpKind::Assign { add, remove } => set_assignees(issue_id()?, &ids(add), &ids(remove)),
        OpKind::Label { add, remove } => set_labels(issue_id()?, &ids(add), &ids(remove)),
        OpKind::State {
            issue_state,
            state_reason,
            duplicate_of,
        } => set_state(
            issue_id()?,
            *issue_state,
            *state_reason,
            duplicate_of.as_deref(),
        ),
        OpKind::Body { body } => set_body(issue_id()?, body),
        OpKind::Comment { body } => comment(issue_id()?, body),
        OpKind::Create { .. } => return Err("a create is several requests".into()),
    })
}
