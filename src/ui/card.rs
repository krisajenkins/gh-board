//! A card's contents and border style. Pure.

use ratatui::style::Style;
use ratatui::text::Line;

use crate::types::{Card, IssueState, Meta};
use crate::ui::text::{Segments, compose, split_words, width};
use crate::ui::theme;

/// The lines inside a card's box, each at most `inner` cells: the title
/// (wrapped onto a second line when long, truncated after that), then the
/// priority, assignees and labels.
pub fn lines(card: &Card, meta: Option<&Meta>, inner: usize) -> Vec<Line<'static>> {
    let mut first: Segments = vec![(" ".into(), Style::default())];
    if card.failed {
        first.push(("! ".into(), theme::FAILED));
    } else if card.pending {
        first.push(("~ ".into(), theme::PENDING));
    }
    match card.number {
        Some(n) => first.push((format!("#{n} "), theme::NUMBER)),
        None => first.push(("new ".into(), theme::PENDING)),
    }
    let indent: usize = first.iter().map(|(t, _)| width(t)).sum();
    let title_style = if card.state == Some(IssueState::Closed) {
        theme::DIM
    } else {
        theme::TITLE
    };
    let (head, rest) = split_words(&card.title, inner.saturating_sub(indent));
    first.push((head.into(), title_style));
    let mut out = vec![compose(first, inner)];
    if let Some(rest) = rest.filter(|r| !r.is_empty()) {
        // The continuation lines up under the title; compose truncates it.
        out.push(compose(
            vec![
                (" ".repeat(indent), Style::default()),
                (rest.into(), title_style),
            ],
            inner,
        ));
    }
    out.push(compose(meta_segments(card, meta), inner));
    out
}

fn meta_segments(card: &Card, meta: Option<&Meta>) -> Segments {
    let field = meta.and_then(|m| m.priority_field.as_ref());
    let priority = field
        .zip(card.priority_id.as_deref())
        .and_then(|(f, id)| Some((f.option_name(id)?, f.option_rank(id)?)))
        .map(|(name, rank)| (name.to_string(), theme::priority(rank)));
    let viewer = meta.map(|m| m.viewer.login.as_str());
    let assignees = card.assignees.iter().map(|u| {
        let style = if Some(u.login.as_str()) == viewer {
            theme::ASSIGNEE_ME
        } else {
            theme::ASSIGNEE
        };
        (format!("@{}", u.login), style)
    });
    let labels = card.labels.iter().map(|l| (l.clone(), theme::LABEL));
    let items: Vec<_> = priority
        .into_iter()
        .chain(assignees)
        .chain(labels)
        .collect();
    let mut out = vec![(" ".to_string(), Style::default())];
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            out.push((" ".into(), Style::default()));
        }
        out.push(item);
    }
    out
}

/// A card's height on screen, borders included.
pub fn height(card: &Card, meta: Option<&Meta>, inner: usize) -> usize {
    lines(card, meta, inner).len() + 2
}

/// The border colour carries the card's state. Focus wins; the `~`/`!` marker
/// still shows on a focused card.
pub fn border_style(card: &Card, focused: bool) -> Style {
    if focused {
        theme::CARD_FOCUS
    } else if card.failed {
        theme::FAILED
    } else if card.pending {
        theme::PENDING
    } else {
        theme::CARD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn card(title: &str) -> Card {
        Card {
            number: Some(41),
            title: title.into(),
            ..Card::default()
        }
    }

    #[test]
    fn a_short_title_is_one_line_then_the_meta_line() {
        assert_eq!(
            plain(&lines(&card("Vendor quote"), None, 20)),
            [" #41 Vendor quote", " "]
        );
    }

    #[test]
    fn a_long_title_wraps_under_itself_then_truncates() {
        let c = card("Write the launch blog post for the new release");
        assert_eq!(
            plain(&lines(&c, None, 20)),
            [" #41 Write the", "     launch blog po…", " "]
        );
        assert_eq!(height(&c, None, 20), 5);
    }

    #[test]
    fn markers_precede_the_number() {
        let pending = Card {
            pending: true,
            ..card("A")
        };
        let failed = Card {
            failed: true,
            pending: true,
            ..card("A")
        };
        assert_eq!(plain(&lines(&pending, None, 20))[0], " ~ #41 A");
        assert_eq!(plain(&lines(&failed, None, 20))[0], " ! #41 A");
    }

    #[test]
    fn a_card_being_created_says_new() {
        let c = Card {
            number: None,
            ..card("Fresh")
        };
        assert_eq!(plain(&lines(&c, None, 20))[0], " new Fresh");
    }

    #[test]
    fn border_reflects_focus_then_failure_then_pending() {
        let c = Card {
            failed: true,
            pending: true,
            ..card("A")
        };
        assert_eq!(border_style(&c, true), theme::CARD_FOCUS);
        assert_eq!(border_style(&c, false), theme::FAILED);
        let p = Card {
            pending: true,
            ..card("A")
        };
        assert_eq!(border_style(&p, false), theme::PENDING);
        assert_eq!(border_style(&card("A"), false), theme::CARD);
    }
}
