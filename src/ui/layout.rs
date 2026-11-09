//! Board geometry: column widths, which columns fit, and scrolling so the
//! focused card stays visible. Pure arithmetic on sizes.

/// Columns are never narrower than this; when they do not all fit, the board
/// scrolls sideways.
pub const MIN_COL_WIDTH: usize = 18;
/// Cells between columns.
pub const GAP: usize = 1;

/// The width of each of `n` columns sharing `width` cells.
pub fn column_width(n: usize, width: usize) -> usize {
    if n == 0 {
        return width;
    }
    (width.saturating_sub((n - 1) * GAP) / n).max(MIN_COL_WIDTH)
}

/// The column width and how many columns show: as many as fit at
/// [`column_width`], then widened to fill the space between them.
pub fn column_layout(n: usize, width: usize) -> (usize, usize) {
    let visible = visible_columns(n, column_width(n, width), width);
    (column_width(visible, width), visible)
}

/// How many columns of width `w` fit side by side in `width` (at least one).
pub fn visible_columns(n: usize, w: usize, width: usize) -> usize {
    ((width + GAP) / (w + GAP)).clamp(1, n.max(1))
}

/// The first of `visible` items to show so that `focus` is among them,
/// moving as little as possible from `first`, and never leaving a gap at the
/// end.
pub fn scroll_into_view(first: usize, focus: usize, visible: usize, n: usize) -> usize {
    let visible = visible.max(1);
    let first = if focus < first {
        focus
    } else if focus >= first + visible {
        focus + 1 - visible
    } else {
        first
    };
    first.min(n.saturating_sub(visible))
}

/// How many of `heights`, starting at `first`, fit whole in `height` lines
/// (at least one, so a card taller than the column still shows).
pub fn fitting(heights: &[usize], first: usize, height: usize) -> usize {
    let mut used = 0;
    let mut n = 0;
    for &h in heights.iter().skip(first) {
        if used + h > height && n > 0 {
            break;
        }
        used += h;
        n += 1;
    }
    n
}

/// The first card to show in a column of cards of these heights, `height`
/// lines tall, so that card `focus` is whole on screen. Moves as little as
/// possible from `first`, and pulls back when cards below have gone, so the
/// column does not end in blank space while cards above are hidden.
pub fn scroll_to_card(
    heights: &[usize],
    focus: Option<usize>,
    first: usize,
    height: usize,
) -> usize {
    let n = heights.len();
    let mut first = first.min(n.saturating_sub(1));
    if let Some(f) = focus.filter(|&f| f < n) {
        if f < first {
            first = f;
        }
        while first < f && first + fitting(heights, first, height) <= f {
            first += 1;
        }
    }
    // Pull back while the card above would also fit.
    while first > 0 {
        let below: usize = heights[first - 1..].iter().sum();
        if below > height {
            break;
        }
        first -= 1;
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_share_the_width_less_the_gaps() {
        assert_eq!(column_width(4, 100), 24); // (100 - 3) / 4
        assert_eq!(column_width(1, 80), 80);
    }

    #[test]
    fn columns_have_a_minimum_width() {
        assert_eq!(column_width(10, 100), MIN_COL_WIDTH);
    }

    #[test]
    fn as_many_columns_show_as_fit() {
        assert_eq!(visible_columns(4, 24, 100), 4);
        assert_eq!(visible_columns(10, 18, 100), 5); // 5 * 18 + 4 = 94
        assert_eq!(visible_columns(10, 18, 10), 1);
        assert_eq!(visible_columns(2, 18, 200), 2);
    }

    #[test]
    fn columns_that_do_not_all_fit_widen_to_fill() {
        assert_eq!(column_layout(4, 100), (24, 4));
        assert_eq!(column_layout(4, 40), (19, 2)); // 19 + 1 + 19 + 1 spare
        assert_eq!(column_layout(10, 100), (19, 5));
        assert_eq!(column_layout(0, 50), (50, 1));
    }

    #[test]
    fn horizontal_scroll_follows_focus_minimally() {
        assert_eq!(scroll_into_view(0, 2, 3, 6), 0);
        assert_eq!(scroll_into_view(0, 3, 3, 6), 1);
        assert_eq!(scroll_into_view(2, 1, 3, 6), 1);
        assert_eq!(scroll_into_view(5, 5, 3, 6), 3, "no gap at the end");
    }

    #[test]
    fn fitting_counts_whole_cards() {
        assert_eq!(fitting(&[4, 4, 5, 4], 0, 12), 2);
        assert_eq!(fitting(&[4, 4, 5, 4], 1, 13), 3);
        assert_eq!(fitting(&[20], 0, 5), 1, "a too-tall card still shows");
        assert_eq!(fitting(&[], 0, 5), 0);
    }

    #[test]
    fn a_visible_focus_does_not_scroll() {
        let h = [4; 10];
        assert_eq!(scroll_to_card(&h, Some(1), 0, 12), 0);
        assert_eq!(scroll_to_card(&h, Some(5), 4, 12), 4);
    }

    #[test]
    fn focus_below_scrolls_until_it_is_the_last_whole_card() {
        let h = [4; 10];
        // 3 cards fit in 12 lines: focus 5 -> first 3 (cards 3, 4, 5).
        assert_eq!(scroll_to_card(&h, Some(5), 0, 12), 3);
        // Mixed heights: card 3 (5 tall) needs 4 + 5 to fit in 9.
        assert_eq!(scroll_to_card(&[4, 4, 4, 5], Some(3), 0, 9), 2);
    }

    #[test]
    fn focus_above_scrolls_up_to_it() {
        assert_eq!(scroll_to_card(&[4; 10], Some(2), 6, 12), 2);
    }

    #[test]
    fn scroll_pulls_back_when_the_end_would_leave_space() {
        // Four cards, three fit: starting at 3 would show one card and blank.
        assert_eq!(scroll_to_card(&[4; 4], Some(3), 3, 12), 1);
        assert_eq!(scroll_to_card(&[4; 4], None, 9, 12), 1);
        assert_eq!(scroll_to_card(&[4; 2], Some(0), 1, 12), 0);
    }

    #[test]
    fn empty_columns_do_not_scroll() {
        assert_eq!(scroll_to_card(&[], None, 3, 12), 0);
    }

    #[test]
    fn focus_reaches_the_bottom_of_a_long_column() {
        let h = [4; 56];
        let first = scroll_to_card(&h, Some(55), 0, 20);
        assert_eq!(first, 51);
        assert!(first + fitting(&h, first, 20) > 55);
    }
}
