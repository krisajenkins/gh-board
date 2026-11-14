//! "Agent context" for `Y`: a Markdown brief on the focused card and its
//! project, to paste into a coding agent that has `gh`. Pure.

use crate::model::{self, State};
use crate::types::Card;
use crate::ui::detail::state_label;

const PREAMBLE: &str = "\
You are helping manage a GitHub project. The `gh` CLI is installed and logged \
in with access to it. Act on the card below as asked: for example, research it \
and add your notes as a comment (`gh issue comment`), or update its fields \
(`gh issue edit`, `gh project item-edit`). Use `gh` for every change, and keep \
to this card unless asked otherwise.";

/// The agent context for `card`: the preamble, the project, and the card. When
/// the detail view is open on this card, its description and loaded comments
/// too.
pub fn agent_context(state: &State, card: &Card) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    line(PREAMBLE.into());

    let config = &state.config;
    let meta = state.meta.as_ref();
    line(String::new());
    line("## Project".into());
    line(String::new());
    line(format!("- Owner: {}", config.owner));
    match meta {
        Some(m) => line(format!("- Project: {} ({})", config.project, m.title)),
        None => line(format!("- Project: {}", config.project)),
    }
    line(format!(
        "- URL: https://github.com/orgs/{}/projects/{}",
        config.owner, config.project
    ));
    if let Some(m) = meta {
        line(format!("- Project ID: {}", m.project_id));
    }
    if let Some(repo) = new_issue_repo(state) {
        line(format!("- New issues go into: {repo}"));
    }
    if let Some(m) = meta {
        let names: Vec<&str> = m
            .status_field
            .options
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        line(format!(
            "- Columns (the project's \"{}\" field): {}",
            config.status_field,
            names.join(", ")
        ));
        if let Some(pf) = &m.priority_field {
            let names: Vec<&str> = pf.options.iter().map(|o| o.name.as_str()).collect();
            line(format!(
                "- Priorities (the org-level issue field \"{}\", set on the issue): {}",
                config.priority_field,
                names.join(", ")
            ));
        }
    }

    line(String::new());
    match card.number {
        Some(n) => line(format!("## Card: #{n} {}", card.title)),
        None => line(format!("## Card: {}", card.title)),
    }
    line(String::new());
    if card.issue_id.is_none() && !card.temp {
        line("- A draft: it has no issue yet".into());
    }
    if card.temp {
        line("- Still being created".into());
    }
    if let Some(url) = &card.url {
        line(format!("- URL: {url}"));
    }
    if let Some(repo) = &card.repo {
        line(format!("- Repo: {repo}"));
    }
    if !card.temp {
        line(format!("- Project item ID: {}", card.item_id));
    }
    if card.state.is_some() {
        line(format!("- State: {}", state_label(card)));
    }
    if let Some(m) = meta {
        let status = card
            .status_id
            .as_deref()
            .and_then(|id| m.status_field.option_name(id))
            .unwrap_or("No Status");
        line(format!("- Status: {status}"));
        if let Some(pf) = &m.priority_field {
            let priority = card
                .priority_id
                .as_deref()
                .and_then(|id| pf.option_name(id))
                .unwrap_or("none");
            line(format!("- Priority: {priority}"));
        }
    }
    let who: Vec<String> = card
        .assignees
        .iter()
        .map(|u| format!("@{}", u.login))
        .collect();
    line(format!("- Assignees: {}", or_none(&who.join(" "))));
    let labels: Vec<&str> = card.labels.iter().map(|l| l.name.as_str()).collect();
    line(format!("- Labels: {}", or_none(&labels.join(", "))));

    let detail = state.detail.as_ref().filter(|d| d.item_id == card.item_id);
    let Some(detail) = detail else {
        return out;
    };
    if let Some((body, _)) = model::description(Some(detail), &state.ops, &card.item_id) {
        let body = body.replace('\r', "");
        line(String::new());
        line("### Description".into());
        line(String::new());
        line(match body.trim() {
            "" => "(no description)".into(),
            b => b.into(),
        });
    }
    if let Some(data) = &detail.data {
        let shown = data.comments.len();
        let total = (data.total as usize).max(shown);
        line(String::new());
        if total > shown {
            line(format!(
                "### Comments (the latest {shown} of {total}, oldest first)"
            ));
        } else {
            line(format!("### Comments ({total})"));
        }
        for c in &data.comments {
            let date: String = c.created_at.chars().take(10).collect();
            line(String::new());
            line(format!("**@{}** ({date}):", c.author));
            line(String::new());
            line(c.body.replace('\r', "").trim_end().into());
        }
    }
    out
}

fn or_none(s: &str) -> &str {
    if s.is_empty() { "none" } else { s }
}

/// The name of the repository new issues go into, if there is one.
fn new_issue_repo(state: &State) -> Option<String> {
    let id = model::target_repo(state).ok()?;
    state
        .base
        .cards
        .values()
        .find(|c| c.repo_id.as_deref() == Some(id.as_str()))
        .and_then(|c| c.repo.clone())
}

/// The notice's name for what `Y` copied.
pub fn what(card: &Card) -> String {
    format!("Agent context for {}", card.label())
}
