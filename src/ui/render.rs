//! Drawing: a pure function of `(&State, &UiState)` onto a ratatui `Frame`.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::model::{self, BoardView, Column, State};
use crate::types::Meta;
use crate::ui::keys::HELP;
use crate::ui::layout::{GAP, column_layout, fitting};
use crate::ui::picker::{self, Menu};
use crate::ui::state::{Areas, Mode, UiState};
use crate::ui::text::{Segments, compose, fit, width, wrap};
use crate::ui::theme;
use crate::ui::{card, detail};
use crate::update::Level;

/// Draw everything. Scroll offsets are used as they are: call
/// [`UiState::settle`] first so the focused card is in view.
pub fn render(frame: &mut Frame, state: &State, ui: &UiState) {
    let areas = Areas::new(frame.area());
    let v = model::board(state);
    if v.columns.is_empty() {
        render_message(frame, state, areas.board);
    } else {
        render_board(frame, state, ui, &v, areas);
    }
    render_message_line(frame, ui, areas.message);
    render_status(frame, state, areas.status);
    if state.detail.is_some() {
        render_detail(frame, state, ui);
    }
    if let Mode::Picker(picker) = &ui.mode
        && let Some(menu) = picker::menu(state, picker)
    {
        render_picker(frame, &menu, picker.cursor);
    }
    let typing = matches!(
        ui.mode,
        Mode::Search { .. }
            | Mode::NewIssue { .. }
            | Mode::Feedback { .. }
            | Mode::ConfirmQuit { .. }
    );
    if let Some(op) = ui.prompt.as_ref().filter(|_| !typing) {
        let err = op.err.as_deref().unwrap_or("?");
        render_failure(frame, &format!("{} failed: {err}", op.desc));
    } else if ui.mode == Mode::Help {
        let (title, keys) = if state.detail.is_some() {
            (" detail keys ", detail::HELP)
        } else {
            (" gh-board keys ", HELP)
        };
        render_help(frame, title, keys);
    }
}

fn render_detail(frame: &mut Frame, state: &State, ui: &UiState) {
    let rect = detail::rect(frame.area());
    let lines = detail::lines(state, detail::inner_width(rect));
    let height = detail::inner_height(rect);
    let first = ui.detail_scroll.min(lines.len().saturating_sub(height));
    let title = match detail::card(state).and_then(|c| c.number) {
        Some(n) => format!(" #{n} "),
        None => " issue ".into(),
    };
    let position = if lines.len() > height {
        let last = (first + height).min(lines.len());
        format!(" {}-{last}/{} ", first + 1, lines.len())
    } else {
        String::new()
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::CARD_FOCUS)
        .title(Line::from(title).centered())
        .title_bottom(Line::styled(
            fit(
                detail::HINT,
                usize::from(rect.width).saturating_sub(position.len() + 4),
            ),
            theme::DIM,
        ))
        .title_bottom(Line::from(position).right_aligned());
    let shown: Vec<Line> = lines.into_iter().skip(first).take(height).collect();
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    let inner = Rect {
        x: rect.x + 2,
        y: rect.y + 1,
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    };
    frame.render_widget(Paragraph::new(shown), inner);
}

fn render_picker(frame: &mut Frame, menu: &Menu, cursor: usize) {
    let area = frame.area();
    let max_h = usize::from(area.height).saturating_sub(4).max(1);
    let rows: Vec<(String, bool)> = if menu.loading {
        vec![("loading…".into(), false)]
    } else if menu.choices.is_empty() {
        vec![("(nothing to pick)".into(), false)]
    } else {
        menu.choices
            .iter()
            .map(|c| {
                let mark = if c.marked { menu.mark } else { " " };
                (format!(" {mark} {} ", c.label), true)
            })
            .collect()
    };
    let visible = rows.len().min(max_h.saturating_sub(2)).max(1);
    let first = if cursor < visible {
        0
    } else {
        cursor + 1 - visible
    };
    let hint = if menu.stays_open {
        " Enter toggles · Esc done "
    } else {
        " Enter picks · Esc cancels "
    };
    let title = format!(" {} ", menu.title);
    let w = rows
        .iter()
        .map(|(l, _)| width(l))
        .chain([width(&title), width(hint)])
        .max()
        .unwrap_or(0)
        + 4;
    let w = w.min(usize::from(area.width).saturating_sub(2));
    let lines: Vec<Line> = rows
        .into_iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .map(|(i, (text, pickable))| {
            let text = fit(&text, w.saturating_sub(2));
            if !pickable {
                Line::styled(format!(" {text}"), theme::DIM)
            } else if i == cursor {
                Line::styled(
                    format!("{text:<0$}", w.saturating_sub(2)),
                    theme::HEADER_FOCUS,
                )
            } else {
                Line::raw(text)
            }
        })
        .collect();
    let rect = centered(area, w, lines.len() + 2);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::CARD_FOCUS)
        .title(title)
        .title_bottom(Line::styled(hint, theme::DIM).right_aligned());
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

/// Before the board arrives (or when it has no columns).
fn render_message(frame: &mut Frame, state: &State, area: Rect) {
    let message = match (&state.error, state.loading) {
        (Some(e), _) => format!("Error: {e}"),
        (None, true) => "Loading…".into(),
        (None, false) => "No columns".into(),
    };
    let style = if state.error.is_some() {
        theme::ERROR
    } else {
        theme::DIM
    };
    let area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    }
    .intersection(area);
    let lines: Vec<Line> = wrap(&message, usize::from(area.width))
        .into_iter()
        .map(|l| Line::styled(l, style))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_board(frame: &mut Frame, state: &State, ui: &UiState, v: &BoardView, areas: Areas) {
    let n = v.columns.len();
    let total = usize::from(areas.board.width);
    let (w, visible) = column_layout(n, total);
    let first_col = ui.scroll.first_col.min(n - visible);
    let meta = state.meta.as_ref();
    for (i, ci) in (first_col..first_col + visible).enumerate() {
        let x = areas.board.x + to_u16(i * (w + GAP));
        let cw = to_u16(w).min(areas.board.right().saturating_sub(x));
        let col = &v.columns[ci];
        let focused = v.col == Some(ci);
        let first = ui.scroll.first_card.get(&col.key).copied().unwrap_or(0);
        let column = ColumnView {
            col,
            meta,
            focused,
            focused_row: v.row.filter(|_| focused),
            first,
            inner: w.saturating_sub(2),
        };
        let shown = column.render(frame, Rect::new(x, areas.board.y, cw, areas.board.height));
        let header = header_text(
            &col.name,
            col.cards.len(),
            column.first(),
            col.cards.len().saturating_sub(column.first() + shown),
            usize::from(cw),
        );
        let style = if focused {
            theme::HEADER_FOCUS
        } else {
            theme::HEADER
        };
        frame.render_widget(
            Paragraph::new(Line::styled(header, style)),
            Rect::new(x, areas.header.y, cw, areas.header.height),
        );
    }
}

/// A column header exactly `w` cells wide: ` Todo (12)`, and on the right how
/// many cards are scrolled off above and below.
pub fn header_text(name: &str, count: usize, above: usize, below: usize, w: usize) -> String {
    let right = match (above, below) {
        (0, 0) => String::new(),
        (a, 0) => format!("↑{a} "),
        (0, b) => format!("↓{b} "),
        (a, b) => format!("↑{a} ↓{b} "),
    };
    let left = fit(
        &format!(" {name} ({count})"),
        w.saturating_sub(width(&right) + 1),
    );
    let pad = w.saturating_sub(width(&left) + width(&right));
    fit(&format!("{left}{}{right}", " ".repeat(pad)), w)
}

struct ColumnView<'a> {
    col: &'a Column,
    meta: Option<&'a Meta>,
    focused: bool,
    focused_row: Option<usize>,
    first: usize,
    inner: usize,
}

impl ColumnView<'_> {
    fn first(&self) -> usize {
        self.first.min(self.col.cards.len().saturating_sub(1))
    }

    /// Draw the cards that fit whole (or the first, if even that does not);
    /// returns how many were drawn.
    fn render(&self, frame: &mut Frame, area: Rect) -> usize {
        let cards = &self.col.cards;
        if cards.is_empty() {
            let style = if self.focused {
                theme::CARD_FOCUS
            } else {
                theme::DIM
            };
            frame.render_widget(Paragraph::new(Line::styled("  (empty)", style)), area);
            return 0;
        }
        let first = self.first();
        let contents: Vec<_> = cards[first..]
            .iter()
            .map(|c| card::lines(c, self.meta, self.inner))
            .collect();
        let heights: Vec<usize> = contents.iter().map(|l| l.len() + 2).collect();
        let shown = fitting(&heights, 0, usize::from(area.height));
        let mut y = area.y;
        for (i, lines) in contents.into_iter().take(shown).enumerate() {
            let card = &cards[first + i];
            let h = to_u16(lines.len() + 2).min(area.bottom().saturating_sub(y));
            let rect = Rect::new(area.x, y, area.width, h);
            let focused = self.focused_row == Some(first + i);
            let block = Block::bordered()
                .border_type(card::border_type(focused))
                .border_style(card::border_style(card, focused));
            frame.render_widget(Paragraph::new(lines).block(block), rect);
            y += h;
        }
        shown
    }
}

fn render_message_line(frame: &mut Frame, ui: &UiState, area: Rect) {
    let w = usize::from(area.width);
    let (prompt, input) = match &ui.mode {
        Mode::Search { input, .. } => ("/", input),
        Mode::NewIssue { input } => ("New issue title: ", input),
        Mode::Feedback { input } => ("Feedback on gh-board: ", input),
        Mode::ConfirmQuit { pending } => {
            let what = if *pending == 1 {
                "1 change is".to_string()
            } else {
                format!("{pending} changes are")
            };
            let line = compose(
                vec![(
                    format!("{what} still being sent. Quit anyway? (y/n)"),
                    theme::WARN,
                )],
                w,
            );
            frame.render_widget(Paragraph::new(line), area);
            return;
        }
        _ => {
            if let Some(notice) = &ui.notice {
                let style = match notice.level {
                    Level::Info => theme::INFO,
                    Level::Warn => theme::WARN,
                    Level::Error => theme::ERROR,
                };
                let line = compose(vec![(notice.message.replace('\n', " "), style)], w);
                frame.render_widget(Paragraph::new(line), area);
            }
            return;
        }
    };
    // Keep the end of a long input in view, where the cursor is.
    let room = w.saturating_sub(width(prompt) + 1);
    let shown = tail(input, room);
    let line = Line::from(vec![
        Span::styled(prompt, theme::KEY),
        Span::raw(shown.to_string()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
    let x = area.x + to_u16(width(prompt) + width(shown));
    frame.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
}

/// The longest suffix of `s` that fits in `w` cells.
fn tail(s: &str, w: usize) -> &str {
    let mut start = s.len();
    for (i, _) in s.char_indices().rev() {
        if width(&s[i..]) > w {
            break;
        }
        start = i;
    }
    &s[start..]
}

/// The status line's text as styled segments: what the board is showing on
/// the left, what is happening on the right.
pub fn status_segments(state: &State) -> (Segments, Segments) {
    let meta = state.meta.as_ref();
    let title = meta.map_or("gh-board", |m| m.title.as_str());
    let options = meta
        .and_then(|m| m.priority_field.as_ref())
        .map_or(0, |f| f.options.len());
    let keys = if options > 0 {
        format!("1-{options} · 0 all · {} none", options + 1)
    } else {
        "no priority field".into()
    };
    let plain = Style::default();
    let mut left = vec![
        (format!(" {title} "), theme::HEADER_FOCUS),
        (" Priority: ".into(), plain),
        (model::filter_label(state), theme::KEY),
        (format!(" ({keys})"), plain),
    ];
    if !state.search.is_empty() {
        left.push((" │ Search: ".into(), plain));
        left.push((format!("/{}", state.search), theme::KEY));
        left.push((" (C-l clears)".into(), plain));
    }
    let hidden = &state.config.hidden_statuses;
    if !hidden.is_empty() {
        let what = hidden.join(", ");
        left.push((
            if state.show_hidden {
                format!(" │ showing {what}")
            } else {
                format!(" │ {what} hidden")
            },
            plain,
        ));
    }
    let mut right = Vec::new();
    if state.loading {
        right.push(("loading… ".into(), theme::PENDING));
    }
    if let Some(e) = &state.error {
        right.push((format!("{} ", e.replace('\n', " ")), theme::FAILED));
    }
    let (pending, failed) = model::op_counts(state);
    if pending > 0 {
        right.push((format!("{pending} pending "), theme::PENDING));
    }
    if failed > 0 {
        right.push((format!("{failed} failed (! to review) "), theme::FAILED));
    }
    right.push((" ? help ".into(), plain));
    (left, right)
}

fn render_status(frame: &mut Frame, state: &State, area: Rect) {
    let (left, right) = status_segments(state);
    let w = usize::from(area.width);
    let right = compose(right, w);
    let rw = right.width();
    let left = compose(left, w.saturating_sub(rw + 1));
    frame.render_widget(Paragraph::new(left).style(theme::STATUS_BAR), area);
    let rw = to_u16(rw).min(area.width);
    frame.render_widget(
        Paragraph::new(right).style(theme::STATUS_BAR),
        Rect::new(area.right() - rw, area.y, rw, area.height),
    );
}

/// A `w` x `h` box in the middle of `area`, clipped to it.
fn centered(area: Rect, w: usize, h: usize) -> Rect {
    let w = to_u16(w).min(area.width);
    let h = to_u16(h).min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn render_help(frame: &mut Frame, title: &str, keys: &[(&str, &str)]) {
    let lines: Vec<Line> = keys
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(format!(" {key:<8}"), theme::KEY),
                Span::raw(format!(" {desc} ")),
            ])
        })
        .collect();
    let w = lines.iter().map(Line::width).max().unwrap_or(0) + 2;
    let area = centered(frame.area(), w, lines.len() + 2);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::CARD_FOCUS)
        .title(title.to_string())
        .title_bottom(Line::from(" any key closes ").right_aligned());
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_failure(frame: &mut Frame, message: &str) {
    let w = usize::from(frame.area().width).saturating_sub(4).min(64);
    let inner = w.saturating_sub(4);
    let mut lines: Vec<Line> = wrap(message, inner)
        .into_iter()
        .map(|l| Line::raw(format!(" {l}")))
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled("r", theme::KEY),
        Span::raw(" Retry   "),
        Span::styled("a", theme::KEY),
        Span::raw(" Abandon   "),
        Span::styled("Esc", theme::KEY),
        Span::raw(" Later (! brings it back)"),
    ]));
    let area = centered(frame.area(), w, lines.len() + 2);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::FAILED)
        .title(" A change failed ");
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_shows_name_and_count() {
        assert_eq!(header_text("Todo", 12, 0, 0, 14), " Todo (12)    ");
    }

    #[test]
    fn header_shows_cards_scrolled_off() {
        assert_eq!(header_text("Todo", 56, 3, 40, 22), " Todo (56)     ↑3 ↓40 ");
        assert_eq!(width(&header_text("Todo", 56, 3, 40, 22)), 22);
        assert_eq!(header_text("Todo", 56, 0, 40, 16), " Todo (56)  ↓40 ");
    }

    #[test]
    fn header_truncates_a_long_name_before_the_counts() {
        let h = header_text("Waiting for review", 5, 0, 2, 16);
        assert_eq!(h, " Waiting fo… ↓2 ");
        assert_eq!(width(&h), 16);
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("hello world", 5), "world");
        assert_eq!(tail("日本語", 4), "本語");
        assert_eq!(tail("ab", 5), "ab");
    }
}
