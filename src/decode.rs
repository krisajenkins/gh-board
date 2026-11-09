//! GraphQL response data -> the domain types. Pure.
//!
//! Input is the `data` member of a response. Every shape here is selected by
//! `queries.rs`. Decoding is lenient where GitHub is: a missing or `null`
//! optional field becomes `None` or empty rather than an error, and only the
//! things the board cannot do without (the project, its status field, a
//! mutation's payload) are errors.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::model::OpKind;
use crate::types::{
    Board, Card, Cards, Comment, Config, DetailData, Field, FieldOption, IssueState, Meta,
    StateReason, User,
};
use crate::update::MutationResult;

pub type Result<T> = std::result::Result<T, String>;

/// `null` and absent both decode to the default. (Serde's `default` alone
/// only covers absent.)
fn nullable<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(d)?.unwrap_or_default())
}

/// A GraphQL connection's `nodes`. A node can be `null` (for one we may not
/// see) or `{}` (an inline fragment on a type it is not); both are skipped by
/// the decoders below.
#[derive(Deserialize)]
#[serde(bound = "T: Deserialize<'de>")]
struct Nodes<T> {
    #[serde(default, deserialize_with = "nullable")]
    nodes: Vec<Option<T>>,
}

impl<T> Default for Nodes<T> {
    fn default() -> Self {
        Nodes { nodes: Vec::new() }
    }
}

impl<T> Nodes<T> {
    fn into_iter(self) -> impl Iterator<Item = T> {
        self.nodes.into_iter().flatten()
    }
}

#[derive(Deserialize)]
struct RawUser {
    id: String,
    login: String,
    name: Option<String>,
}

impl From<RawUser> for User {
    fn from(u: RawUser) -> Self {
        User {
            id: u.id,
            login: u.login,
            name: u.name.filter(|n| !n.is_empty()),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRepo {
    id: String,
    name_with_owner: String,
}

#[derive(Deserialize)]
struct RawLabel {
    name: String,
}

#[derive(Deserialize)]
struct RawId {
    id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFieldValue {
    option_id: Option<String>,
    field: Option<RawId>,
}

/// The `CardIssue` fragment.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIssue {
    id: String,
    number: Option<u64>,
    title: Option<String>,
    url: Option<String>,
    state: Option<String>,
    state_reason: Option<String>,
    repository: Option<RawRepo>,
    #[serde(default, deserialize_with = "nullable")]
    assignees: Nodes<RawUser>,
    #[serde(default, deserialize_with = "nullable")]
    labels: Nodes<RawLabel>,
    #[serde(default, deserialize_with = "nullable")]
    issue_field_values: Nodes<RawFieldValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOptionId {
    option_id: Option<String>,
}

/// The `CardItem` fragment. `content` stays raw until we know it is an issue.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    id: String,
    field_value_by_name: Option<RawOptionId>,
    content: Option<Value>,
}

#[derive(Deserialize)]
struct RawOption {
    id: String,
    name: String,
}

/// A single-select field. `{}` for fields of other types.
#[derive(Deserialize)]
struct RawSelectField {
    id: Option<String>,
    name: Option<String>,
    options: Option<Vec<RawOption>>,
}

impl RawSelectField {
    fn into_field(self) -> Option<Field> {
        Some(Field {
            id: self.id?,
            options: self
                .options?
                .into_iter()
                .map(|o| FieldOption {
                    id: o.id,
                    name: o.name,
                })
                .collect(),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBoardPage {
    viewer: Option<RawUser>,
    organization: Option<RawOrg>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOrg {
    #[serde(default, deserialize_with = "nullable")]
    issue_fields: Nodes<RawSelectField>,
    #[serde(rename = "projectV2")]
    project: Option<RawProject>,
}

#[derive(Deserialize)]
struct RawProject {
    id: String,
    title: Option<String>,
    field: Option<RawSelectField>,
    #[serde(default, deserialize_with = "nullable")]
    items: Nodes<RawItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComment {
    id: String,
    body: Option<String>,
    created_at: Option<String>,
    author: Option<RawLogin>,
}

#[derive(Deserialize)]
struct RawLogin {
    login: String,
}

impl From<RawComment> for Comment {
    fn from(c: RawComment) -> Self {
        Comment {
            id: c.id,
            // A deleted account's comments have no author.
            author: c.author.map_or_else(|| "ghost".into(), |a| a.login),
            body: c.body.unwrap_or_default(),
            created_at: c.created_at.unwrap_or_default(),
        }
    }
}

fn parse<T: DeserializeOwned>(v: &Value, what: &str) -> Result<T> {
    T::deserialize(v).map_err(|e| format!("unexpected {what}: {e}"))
}

/// Follow a path of object keys; `None` if any step is missing or `null`.
fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .filter(|v| !v.is_null())
}

pub fn issue_state(s: &str) -> Option<IssueState> {
    match s {
        "OPEN" => Some(IssueState::Open),
        "CLOSED" => Some(IssueState::Closed),
        _ => None,
    }
}

pub fn state_reason(s: &str) -> Option<StateReason> {
    match s {
        "COMPLETED" => Some(StateReason::Completed),
        "NOT_PLANNED" => Some(StateReason::NotPlanned),
        "DUPLICATE" => Some(StateReason::Duplicate),
        "REOPENED" => Some(StateReason::Reopened),
        _ => None,
    }
}

fn card_from_issue(raw: RawIssue, priority_field_id: Option<&str>) -> Card {
    // Matched by field id: another field (say, Effort) may have an option
    // with the same name.
    let priority_id = priority_field_id.and_then(|pf| {
        raw.issue_field_values
            .into_iter()
            .filter(|v| v.field.as_ref().and_then(|f| f.id.as_deref()) == Some(pf))
            .find_map(|v| v.option_id)
    });
    Card {
        item_id: String::new(),
        issue_id: Some(raw.id),
        number: raw.number,
        title: raw.title.unwrap_or_default(),
        url: raw.url,
        state: raw.state.as_deref().and_then(issue_state),
        state_reason: raw.state_reason.as_deref().and_then(state_reason),
        repo_id: raw.repository.as_ref().map(|r| r.id.clone()),
        repo: raw.repository.map(|r| r.name_with_owner),
        priority_id,
        assignees: raw.assignees.into_iter().map(User::from).collect(),
        labels: raw.labels.into_iter().map(|l| l.name).collect(),
        ..Card::default()
    }
}

/// The issue fields of a card (everything except item id and status), from a
/// `CardIssue` node.
pub fn issue(node: &Value, priority_field_id: Option<&str>) -> Result<Card> {
    Ok(card_from_issue(parse(node, "issue")?, priority_field_id))
}

fn card_from_item(raw: RawItem, priority_field_id: Option<&str>) -> Result<Option<Card>> {
    let Some(content) = raw
        .content
        .filter(|c| c.get("__typename").and_then(Value::as_str) == Some("Issue"))
    else {
        return Ok(None);
    };
    let mut card = issue(&content, priority_field_id)?;
    card.item_id = raw.id;
    card.status_id = raw.field_value_by_name.and_then(|v| v.option_id);
    Ok(Some(card))
}

/// A project item as a card, from a `CardItem` node. `None` when its content
/// is not an issue (a draft, a pull request).
pub fn item(node: &Value, priority_field_id: Option<&str>) -> Result<Option<Card>> {
    card_from_item(parse(node, "project item")?, priority_field_id)
}

/// Assemble every page of a board query into one board. Also returns how many
/// items were skipped for not being issues.
pub fn board(pages: &[Value], config: &Config) -> Result<(Board, usize)> {
    let mut pages = pages.iter().map(|p| parse::<RawBoardPage>(p, "board page"));
    let first = pages.next().ok_or("no pages")??;
    let org = first
        .organization
        .ok_or("organization not found (is the owner an organization you can see?)")?;
    let project = org.project.ok_or("project not found")?;
    let status_field = project
        .field
        .and_then(RawSelectField::into_field)
        .ok_or_else(|| {
            format!(
                "project has no single-select field named \"{}\"",
                config.status_field
            )
        })?;
    let priority_field = org
        .issue_fields
        .into_iter()
        .filter(|f| f.name.as_deref() == Some(config.priority_field.as_str()))
        .find_map(RawSelectField::into_field);
    let pf_id = priority_field.as_ref().map(|f| f.id.clone());

    let mut cards = Cards::default();
    let mut skipped = 0;
    let mut add_items = |items: Nodes<RawItem>| -> Result<()> {
        for node in items.into_iter() {
            match card_from_item(node, pf_id.as_deref())? {
                Some(card) => cards.put(card),
                None => skipped += 1,
            }
        }
        Ok(())
    };
    add_items(project.items)?;
    for page in pages {
        if let Some(p) = page?.organization.and_then(|o| o.project) {
            add_items(p.items)?;
        }
    }

    let viewer = first.viewer.map(User::from).unwrap_or(User {
        id: String::new(),
        login: String::new(),
        name: None,
    });
    let meta = Meta {
        viewer: User {
            name: None,
            ..viewer
        },
        project_id: project.id,
        title: project.title.unwrap_or_default(),
        status_field,
        priority_field,
    };
    Ok((Board { meta, cards }, skipped))
}

pub fn comment(node: &Value) -> Result<Comment> {
    Ok(parse::<RawComment>(node, "comment")?.into())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDetail {
    body: Option<String>,
    comments: Option<RawComments>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComments {
    total_count: Option<u64>,
    #[serde(default, deserialize_with = "nullable")]
    nodes: Vec<Option<RawComment>>,
}

/// An issue's body and latest comments.
pub fn detail(data: &Value) -> Result<DetailData> {
    let node = at(data, &["node"]).ok_or("issue not found")?;
    let raw: RawDetail = parse(node, "issue detail")?;
    let (total, comments) = match raw.comments {
        None => (None, Vec::new()),
        Some(c) => (
            c.total_count,
            c.nodes
                .into_iter()
                .flatten()
                .map(Comment::from)
                .collect::<Vec<_>>(),
        ),
    };
    Ok(DetailData {
        body: raw.body.unwrap_or_default(),
        total: total.unwrap_or(comments.len() as u64),
        comments,
    })
}

/// The users who may be assigned issues in a repository, sorted by login
/// ignoring case.
pub fn assignable(data: &Value) -> Result<Vec<User>> {
    let repo = at(data, &["repository"]).ok_or("repository not found")?;
    let users = at(repo, &["assignableUsers"]).map_or(Ok(Nodes::default()), |u| {
        parse::<Nodes<RawUser>>(u, "assignable users")
    })?;
    let mut users: Vec<User> = users.into_iter().map(User::from).collect();
    users.sort_by_cached_key(|u| u.login.to_lowercase());
    Ok(users)
}

/// The issue created by `createIssue`: its id and link.
pub fn created_issue(data: &Value) -> Result<(String, Option<String>)> {
    let issue = at(data, &["createIssue", "issue"]).ok_or("no issue in response")?;
    let id = issue
        .get("id")
        .and_then(Value::as_str)
        .ok_or("no issue id in response")?;
    let url = issue.get("url").and_then(Value::as_str);
    Ok((id.into(), url.map(String::from)))
}

/// The project item made by `addProjectV2ItemById`, as a card.
pub fn added_item(data: &Value, priority_field_id: Option<&str>) -> Result<Card> {
    let node = at(data, &["addProjectV2ItemById", "item"]).ok_or("no project item in response")?;
    item(node, priority_field_id)?.ok_or_else(|| "added item is not an issue".into())
}

/// A re-read of one project item (`queries::item`). `None` when it is gone or
/// is not an issue.
pub fn item_reread(data: &Value, priority_field_id: Option<&str>) -> Result<Option<Card>> {
    match at(data, &["node"]).filter(|n| n.get("id").is_some()) {
        None => Ok(None),
        Some(node) => item(node, priority_field_id),
    }
}

/// Decode a successful mutation for an op of the given kind.
///
/// - status: a full card (`CardItem`).
/// - create: a full card, from `{ "item": <CardItem> }` — the shell wraps the
///   project item from the last step it ran (add to project, or set status).
/// - priority, assign, state: issue fields (`CardIssue`; no item id or status).
/// - comment: the comment.
/// - body: the new description.
pub fn mutation(
    kind: &OpKind,
    data: &Value,
    priority_field_id: Option<&str>,
) -> Result<MutationResult> {
    let node = |path: &[&str], missing: &str| at(data, path).ok_or_else(|| missing.to_string());
    let full_card = |n: &Value| -> Result<MutationResult> {
        item(n, priority_field_id)?
            .map(MutationResult::Card)
            .ok_or_else(|| "item in response is not an issue".into())
    };
    let issue_card = |n: &Value| Ok(MutationResult::Card(issue(n, priority_field_id)?));
    match kind {
        OpKind::Status { .. } => full_card(node(
            &["updateProjectV2ItemFieldValue", "projectV2Item"],
            "no item in response",
        )?),
        OpKind::Create { .. } => full_card(node(&["item"], "no item in response")?),
        OpKind::Priority { .. } => issue_card(node(
            &["setIssueFieldValue", "issue"],
            "no issue in response",
        )?),
        OpKind::State { .. } => issue_card(
            at(data, &["closeIssue", "issue"])
                .or_else(|| at(data, &["reopenIssue", "issue"]))
                .ok_or("no issue in response")?,
        ),
        // Root mutation fields run in order, so whichever of the two ran last
        // has the issue with both changes.
        OpKind::Assign { .. } => issue_card(
            at(data, &["added", "assignable"])
                .or_else(|| at(data, &["removed", "assignable"]))
                .ok_or("no issue in response")?,
        ),
        OpKind::Body { .. } => {
            let issue = node(&["updateIssue", "issue"], "no issue in response")?;
            let body = issue
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(MutationResult::Body(body.into()))
        }
        OpKind::Comment { .. } => Ok(MutationResult::Comment(comment(node(
            &["addComment", "commentEdge", "node"],
            "no comment in response",
        )?)?)),
    }
}
