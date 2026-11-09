//! UI-only state: what the terminal shows on top of the model. Kept apart from
//! `model::State`, which knows nothing about modes, prompts or scrolling.
//!
//! Pure. Time is passed in (`now`), never read.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;

use crate::model::{ColumnKey, Op, State, board};
use crate::ui::layout::{column_layout, scroll_into_view, scroll_to_card};
use crate::ui::picker::Picker;
use crate::ui::{card, detail};
use crate::update::{Effect, Level};

/// What keys currently do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Board,
    /// `/`: the board narrows as `input` changes; Esc puts `before` back.
    Search { before: String, input: String },
    /// `n`: a one-line title prompt.
    NewIssue { input: String },
    /// `?`: the key help overlay.
    Help,
    /// A list popup over the detail view.
    Picker(Picker),
    /// `q` while changes are still being sent: quit anyway?
    ConfirmQuit { pending: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub message: String,
    pub level: Level,
    /// When it goes away on its own (any key dismisses it sooner).
    pub until: Instant,
}

/// How long a notice stays up when no key is pressed.
pub fn notice_lifetime(level: Level) -> Duration {
    match level {
        Level::Info => Duration::from_secs(4),
        Level::Warn | Level::Error => Duration::from_secs(10),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    /// The leftmost column shown, when they do not all fit.
    pub first_col: usize,
    /// Per column, the first card shown.
    pub first_card: HashMap<ColumnKey, usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiState {
    pub mode: Mode,
    /// A `g` was pressed; a second makes `gg`.
    pub pending_g: bool,
    /// The failed op being asked about (Retry / Abandon / dismiss).
    pub prompt: Option<Op>,
    pub notice: Option<Notice>,
    pub scroll: Scroll,
    /// The detail view's first visible line.
    pub detail_scroll: usize,
    /// Lines the detail view showed at the last settle (for half-page keys).
    pub detail_height: usize,
}

/// The screen's regions: column headers, the board, a message line (prompts
/// and notices) and the status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    pub header: Rect,
    pub board: Rect,
    pub message: Rect,
    pub status: Rect,
}

impl Areas {
    pub fn new(area: Rect) -> Self {
        let h = area.height;
        let row = |y: u16| Rect::new(area.x, area.y + y.min(h), area.width, u16::from(y < h));
        Areas {
            header: row(0),
            board: Rect::new(area.x, area.y + 1.min(h), area.width, h.saturating_sub(3)),
            message: row(h.saturating_sub(2)),
            status: row(h.saturating_sub(1)),
        }
    }
}

impl UiState {
    /// Handle a UI effect from the reducer. `SaveText` needs the clipboard, so
    /// the shell does that and reports back with a notice.
    pub fn apply_effect(self, effect: Effect, now: Instant) -> UiState {
        match effect {
            Effect::Notify { message, level } => self.notify(message, level, now),
            Effect::PromptFailure { op } => UiState {
                prompt: Some(op),
                ..self
            },
            _ => self,
        }
    }

    pub fn notify(self, message: String, level: Level, now: Instant) -> UiState {
        UiState {
            notice: Some(Notice {
                message,
                level,
                until: now + notice_lifetime(level),
            }),
            ..self
        }
    }

    /// Drop a notice whose time is up. `None` when nothing changed.
    pub fn expire(&self, now: Instant) -> Option<UiState> {
        let notice = self.notice.as_ref()?;
        (now >= notice.until).then(|| UiState {
            notice: None,
            ..self.clone()
        })
    }

    /// Scroll so the focused card is on screen in a terminal of this size.
    /// Call before each draw; rendering uses the offsets as they are.
    pub fn settle(&self, state: &State, area: Rect) -> UiState {
        let v = board(state);
        let areas = Areas::new(area);
        let n = v.columns.len();
        let width = usize::from(areas.board.width);
        let (w, visible) = column_layout(n, width);
        let first_col = scroll_into_view(self.scroll.first_col, v.col.unwrap_or(0), visible, n);
        let meta = state.meta.as_ref();
        let inner = w.saturating_sub(2);
        let first_card = v
            .columns
            .iter()
            .enumerate()
            .map(|(ci, col)| {
                let heights: Vec<usize> = col
                    .cards
                    .iter()
                    .map(|c| card::height(c, meta, inner))
                    .collect();
                let focus = if Some(ci) == v.col { v.row } else { None };
                let prev = self.scroll.first_card.get(&col.key).copied().unwrap_or(0);
                let first = scroll_to_card(&heights, focus, prev, usize::from(areas.board.height));
                (col.key.clone(), first)
            })
            .collect();
        let (detail_scroll, detail_height) = if state.detail.is_some() {
            let rect = detail::rect(area);
            let height = detail::inner_height(rect);
            let len = detail::lines(state, detail::inner_width(rect)).len();
            (self.detail_scroll.min(len.saturating_sub(height)), height)
        } else {
            (0, self.detail_height)
        };
        UiState {
            scroll: Scroll {
                first_col,
                first_card,
            },
            detail_scroll,
            detail_height,
            ..self.clone()
        }
    }
}
