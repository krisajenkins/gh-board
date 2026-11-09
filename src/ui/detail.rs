//! The card detail view's contents, already wrapped to a width so scrolling
//! counts screen lines. Pure.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::model::{self, Detail, OpKind, OpState, State};
use crate::types::{Card, IssueState, Meta};
use crate::ui::text::{width, wrap};
use crate::ui::theme;

/// The detail view's keys, for the help overlay and the README.
pub const HELP: &[(&str, &str)] = &[
    ("j k", "scroll (arrows too); C-d C-u half a page"),
    ("gg G", "top / bottom"),
    ("e", "edit the description in $EDITOR"),
    ("c", "comment, in $EDITOR"),
    ("s", "status picker"),
    ("p", "priority picker"),
    ("a", "assignee picker (toggles; Esc closes)"),
    ("m", "assign me"),
    ("u", "unassign everyone"),
    ("x", "close (completed / not planned) or reopen"),
    ("< >", "move to the previous / next status"),
    ("!", "review failed changes"),
    ("o", "open in the browser"),
    ("r", "reload"),
    ("?", "this help"),
    ("q Esc", "close the detail view"),
];

/// The hint in the detail box's bottom border.
pub const HINT: &str =
    " e edit · c comment · s p a x m u · o browser · r reload · ? help · q close ";

/// Width of a field's `Key:` column.
const KEY_WIDTH: usize = 11;

/// Where the detail box goes: centred over the board, leaving the message and
/// status lines visible.
pub fn rect(area: Rect) -> Rect {
    let avail = Rect {
        height: area.height.saturating_sub(2),
        ..area
    };
    let w = avail
        .width
        .saturating_sub(4)
        .clamp(avail.width.min(20), 100);
    let h = avail.height.saturating_sub(2).max(avail.height.min(5));
    Rect::new(
        avail.x + (avail.width - w) / 2,
        avail.y + (avail.height - h) / 2,
        w,
        h,
    )
}

/// The width text is wrapped to inside the box (borders and a space of
/// padding each side).
pub fn inner_width(rect: Rect) -> usize {
    usize::from(rect.width).saturating_sub(4)
}

/// Lines visible at once inside the box.
pub fn inner_height(rect: Rect) -> usize {
    usize::from(rect.height).saturating_sub(2)
}

/// The card the detail view shows, as currently displayed (ops applied).
pub fn card(state: &State) -> Option<Card> {
    let detail = state.detail.as_ref()?;
    model::view(state).cards.remove(&detail.item_id)
}

/// "Open", or "Closed (completed)" and similar.
pub fn state_label(card: &Card) -> String {
    match card.state {
        Some(IssueState::Open) => "Open".into(),
        Some(IssueState::Closed) => match card.state_reason {
            Some(r) if r != crate::types::StateReason::Reopened => {
                format!("Closed ({})", r.describe())
            }
            _ => "Closed".into(),
        },
        None => "?".into(),
    }
}

struct Out {
    width: usize,
    lines: Vec<Line<'static>>,
}

impl Out {
    fn blank(&mut self) {
        self.lines.push(Line::default());
    }

    /// Text wrapped to the width, every line in one style.
    fn text(&mut self, s: &str, style: Style) {
        for l in wrap(s, self.width) {
            self.lines.push(Line::styled(l, style));
        }
    }

    /// A `Key:  value` line, the value wrapped under itself.
    fn field(&mut self, key: &str, value: &str, style: Style) {
        let room = self.width.saturating_sub(KEY_WIDTH).max(1);
        for (i, l) in wrap(value, room).into_iter().enumerate() {
            let prefix = if i == 0 {
                format!("{:<KEY_WIDTH$}", format!("{key}:"))
            } else {
                " ".repeat(KEY_WIDTH)
            };
            self.lines.push(Line::from(vec![
                Span::styled(prefix, theme::DIM),
                Span::styled(l, style),
            ]));
        }
    }

    /// A section rule: `── Title ─────`.
    fn rule(&mut self, title: &str) {
        let head = format!("── {title} ");
        let fill = self.width.saturating_sub(width(&head));
        self.lines.push(Line::styled(
            format!("{head}{}", "─".repeat(fill)),
            theme::DIM,
        ));
    }
}

/// The detail view's lines at this width. Empty when no detail is open.
pub fn lines(state: &State, width: usize) -> Vec<Line<'static>> {
    let mut out = Out {
        width: width.max(1),
        lines: Vec::new(),
    };
    let (Some(detail), Some(meta)) = (state.detail.as_ref(), state.meta.as_ref()) else {
        return out.lines;
    };
    let Some(card) = card(state) else {
        out.text("This card is no longer on the board.", theme::DIM);
        return out.lines;
    };
    header(&mut out, &card, meta);
    description(&mut out, state, detail, &card);
    comments(&mut out, state, detail, &card, meta);
    out.lines
}

fn header(out: &mut Out, card: &Card, meta: &Meta) {
    let heading = match card.number {
        Some(n) => format!("#{n} {}", card.title),
        None => card.title.clone(),
    };
    out.text(&heading, theme::HEADER.add_modifier(Modifier::BOLD));
    if let Some(url) = &card.url {
        out.text(url, theme::DIM);
    }
    if card.pending {
        out.text("~ changes pending", theme::PENDING);
    }
    if card.failed {
        out.text("! a change failed (! to review)", theme::FAILED);
    }
    out.blank();

    let status = card
        .status_id
        .as_deref()
        .and_then(|id| meta.status_field.option_name(id))
        .unwrap_or("No Status");
    out.field("Status", status, theme::TITLE);
    let pf = meta.priority_field.as_ref();
    match pf
        .zip(card.priority_id.as_deref())
        .and_then(|(f, id)| Some((f.option_name(id)?, f.option_rank(id)?)))
    {
        Some((name, rank)) => out.field("Priority", name, theme::priority(rank)),
        None => out.field("Priority", "none", theme::DIM),
    }
    if card.assignees.is_empty() {
        out.field("Assignees", "nobody", theme::DIM);
    } else {
        let who: Vec<String> = card
            .assignees
            .iter()
            .map(|u| format!("@{}", u.login))
            .collect();
        out.field("Assignees", &who.join(" "), theme::ASSIGNEE);
    }
    if card.labels.is_empty() {
        out.field("Labels", "none", theme::DIM);
    } else {
        out.field("Labels", &card.labels.join(", "), theme::LABEL);
    }
    let closed = card.state == Some(IssueState::Closed);
    out.field(
        "State",
        &state_label(card),
        if closed { theme::DIM } else { theme::TITLE },
    );
    if let Some(repo) = &card.repo {
        out.field("Repo", repo, theme::TITLE);
    }
    out.blank();
}

fn description(out: &mut Out, state: &State, detail: &Detail, card: &Card) {
    out.rule("Description");
    let body = model::description(Some(detail), &state.ops, &card.item_id);
    match body.and_then(|(_, op)| op) {
        Some(op) if op.state == OpState::Failed => out.text(
            &format!("! not saved: {}", op.err.as_deref().unwrap_or("?")),
            theme::FAILED,
        ),
        Some(_) => out.text("~ saving…", theme::PENDING),
        None => {}
    }
    match body {
        Some((text, _)) if text.trim().is_empty() => out.text("(no description)", theme::DIM),
        Some((text, _)) => out.text(&text.replace('\r', ""), theme::TITLE),
        None if detail.loading => out.text("loading…", theme::PENDING),
        None => {
            if let Some(err) = &detail.err {
                out.text(&format!("Failed to load: {err}"), theme::FAILED);
            }
        }
    }
    out.blank();
}

fn comments(out: &mut Out, state: &State, detail: &Detail, card: &Card, meta: &Meta) {
    let data = detail.data.as_ref();
    let loaded = data.map(|d| d.comments.as_slice()).unwrap_or_default();
    let unsent: Vec<_> = state
        .ops
        .iter()
        .filter(|op| op.item_id == card.item_id)
        .filter_map(|op| match &op.kind {
            OpKind::Comment { body } => Some((op, body)),
            _ => None,
        })
        .collect();
    let total = data.map_or(0, |d| d.total as usize) + unsent.len();
    out.rule(&format!("Comments ({total})"));
    if let Some(d) = data
        && d.total as usize > loaded.len()
    {
        let n = d.total as usize - loaded.len();
        out.text(&format!("({n} earlier comments not shown)"), theme::DIM);
    }
    for c in loaded {
        out.blank();
        let date: String = c.created_at.chars().take(10).collect();
        out.text(&format!("@{} · {date}", c.author), theme::ASSIGNEE);
        out.text(&c.body.replace('\r', ""), theme::TITLE);
    }
    let me = &meta.viewer.login;
    for (op, body) in unsent {
        out.blank();
        if op.state == OpState::Failed {
            let err = op.err.as_deref().unwrap_or("?");
            out.text(&format!("@{me} · failed: {err}"), theme::FAILED);
        } else {
            out.text(&format!("@{me} · sending…"), theme::PENDING);
        }
        out.text(body, theme::TITLE);
    }
    if detail.loading && data.is_some() {
        out.blank();
        out.text("reloading…", theme::PENDING);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_is_centred_and_at_most_100_wide() {
        let r = rect(Rect::new(0, 0, 200, 50));
        assert_eq!((r.width, r.height), (100, 46));
        assert_eq!(r.x, 50);
        let r = rect(Rect::new(0, 0, 60, 20));
        assert_eq!((r.width, r.height), (56, 16));
        assert_eq!((inner_width(r), inner_height(r)), (52, 14));
    }

    #[test]
    fn a_tiny_terminal_still_gets_a_box() {
        let r = rect(Rect::new(0, 0, 10, 4));
        assert!(r.width <= 10 && r.height <= 2);
    }
}
