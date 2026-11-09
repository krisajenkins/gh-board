//! Display-width-aware text fitting. Pure.
//!
//! Widths are terminal cells (`unicode-width`, the same measure ratatui uses),
//! never bytes or chars: a CJK title or an emoji takes two cells per glyph.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const ELLIPSIS: &str = "…";

/// Styled pieces of one line, before fitting.
pub type Segments = Vec<(String, Style)>;

/// Display width in terminal cells.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// The longest prefix of `s` that fits in `w` cells.
fn take_width(s: &str, w: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices() {
        let cw = char_width(c);
        if used + cw > w {
            return &s[..i];
        }
        used += cw;
    }
    s
}

/// Fit styled segments into at most `w` cells, ending in `…` when they had
/// to be cut. Not padded.
pub fn compose(segments: Segments, w: usize) -> Line<'static> {
    let total: usize = segments.iter().map(|(t, _)| width(t)).sum();
    if total <= w {
        return Line::from(
            segments
                .into_iter()
                .map(|(t, s)| Span::styled(t, s))
                .collect::<Vec<_>>(),
        );
    }
    if w == 0 {
        return Line::default();
    }
    let mut budget = w - width(ELLIPSIS);
    let mut spans = Vec::new();
    for (text, style) in segments {
        let head = take_width(&text, budget);
        budget -= width(head);
        let cut = head.len() < text.len();
        if !head.is_empty() {
            spans.push(Span::styled(head.to_string(), style));
        }
        if cut {
            break;
        }
    }
    spans.push(Span::raw(ELLIPSIS));
    Line::from(spans)
}

/// Plain text fitted into at most `w` cells.
pub fn fit(s: &str, w: usize) -> String {
    compose(vec![(s.to_string(), Style::default())], w)
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect()
}

/// Split text at a word boundary: the longest run of whole words that fits in
/// `w` cells, and the rest (`None` when it all fits). A first word too long to
/// fit is broken mid-word.
pub fn split_words(s: &str, w: usize) -> (&str, Option<&str>) {
    if width(s) <= w {
        return (s, None);
    }
    let cut = s
        .char_indices()
        .filter(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .take_while(|&i| width(&s[..i]) <= w)
        .last()
        .filter(|&i| i > 0);
    match cut {
        Some(i) => (s[..i].trim_end(), Some(s[i..].trim_start())),
        None => {
            let head = take_width(s, w);
            (head, Some(&s[head.len()..]))
        }
    }
}

/// Word-wrap text into lines of at most `w` cells.
pub fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.lines() {
        let mut rest = para.trim_end();
        if rest.is_empty() {
            lines.push(String::new());
        }
        while !rest.is_empty() {
            let (head, _) = split_words(rest, w.max(1));
            // An empty head (w narrower than one glyph) would never progress.
            let head = if head.is_empty() {
                let n = rest.chars().next().map_or(rest.len(), char::len_utf8);
                &rest[..n]
            } else {
                head
            };
            lines.push(head.to_string());
            rest = rest[head.len()..].trim_start();
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn width_counts_cells_not_bytes_or_chars() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("…"), 1);
        assert_eq!(width("日本"), 4);
        assert_eq!(width("é"), 1);
    }

    #[test]
    fn compose_leaves_text_that_fits_alone() {
        let l = compose(vec![("ab".into(), Style::default())], 2);
        assert_eq!(plain(&l), "ab");
    }

    #[test]
    fn compose_truncates_with_an_ellipsis_across_segments() {
        let l = compose(
            vec![
                ("#12 ".into(), Style::default()),
                ("Hello world".into(), Style::default()),
            ],
            10,
        );
        assert_eq!(plain(&l), "#12 Hello…");
        assert_eq!(width(&plain(&l)), 10);
    }

    #[test]
    fn compose_never_splits_a_wide_glyph() {
        // 5 cells: "日本" is 4, then the ellipsis; "語" would not fit.
        let l = compose(vec![("日本語です".into(), Style::default())], 6);
        assert_eq!(plain(&l), "日本…");
        assert!(width(&plain(&l)) <= 6);
    }

    #[test]
    fn compose_into_nothing_is_empty() {
        assert_eq!(
            plain(&compose(vec![("abc".into(), Style::default())], 0)),
            ""
        );
    }

    #[test]
    fn split_words_breaks_at_the_last_space_that_fits() {
        assert_eq!(
            split_words("Write the launch blog post", 12),
            ("Write the", Some("launch blog post"))
        );
        assert_eq!(split_words("short", 12), ("short", None));
    }

    #[test]
    fn split_words_breaks_a_long_first_word_mid_word() {
        assert_eq!(
            split_words("Supercalifragilistic is long", 5),
            ("Super", Some("califragilistic is long"))
        );
    }

    #[test]
    fn split_words_measures_display_width() {
        // "日本 語" is 2+2 + 1 + 2 = 7 cells; in 5 only "日本" fits.
        assert_eq!(split_words("日本 語", 5), ("日本", Some("語")));
    }

    #[test]
    fn fit_truncates_plain_text() {
        assert_eq!(fit("In Progress (4)", 8), "In Prog…");
        assert_eq!(fit("Todo", 8), "Todo");
    }

    #[test]
    fn wrap_fills_lines_by_word() {
        assert_eq!(
            wrap("Set priority failed: rate limited", 12),
            vec!["Set priority", "failed: rate", "limited"]
        );
        assert_eq!(wrap("a\n\nb", 5), vec!["a", "", "b"]);
    }
}
