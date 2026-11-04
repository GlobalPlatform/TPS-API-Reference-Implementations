/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Word-wrap for EDN/CDDL panes.
//!
//! ratatui's `Paragraph::wrap()` exists, but the line-count/line-width APIs
//! needed to keep scroll-clamping and cursor placement consistent with what
//! it renders are `#[unstable]` and not callable from outside the crate. So
//! wrapping is done by hand here, once, and reused for rendering
//! (`wrap_lines`), list layout (`super::scroll::wrap_list`) and cursor
//! placement (`visual_position`) — all three stay consistent by construction
//! since they all go through [`wrap_row_starts`].

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

/// Columns `c` occupies when ratatui draws it.
///
/// **Correction.** [`wrap_row_starts`] used to count `char`s and call them
/// columns, so a line holding CJK, wide emoji or combining marks wrapped at
/// the wrong place and put the cursor on the wrong cell. It is measured here
/// with the same crate — and deliberately the same major version — that
/// ratatui measures its own cells with, because the only thing that makes the
/// wrap correct is agreeing with the renderer.
///
/// A character with no width (a control character, and a tab) counts as zero
/// columns. That is also what ratatui draws for it: it skips a zero-width
/// grapheme rather than giving it a cell.
///
/// Residual limitation, recorded because the function reads as though it had
/// none: this measures `char`s where ratatui measures *graphemes*. The two
/// agree for a base character followed by combining marks — `1 + 0 + 0 …`,
/// which is the case a CDDL or EDN document reaches this code with — and
/// disagree for a ZWJ emoji sequence, which `unicode-width` counts per code
/// point and ratatui counts once.
pub fn char_columns(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// Columns `s` occupies when ratatui draws it. See [`char_columns`].
pub fn str_columns(s: &str) -> usize {
    s.chars().map(char_columns).sum()
}

/// Char-index offsets (into `line.chars()`) where each visual row of one
/// logical line begins under greedy word-wrap at `width` **columns**. Always
/// starts with `0`. A word wider than `width` is hard-broken so wrapping
/// still terminates.
///
/// The offsets are char indices because that is what every caller slices
/// with; the budget they are chosen against is columns, measured by
/// [`char_columns`].
pub fn wrap_row_starts(line: &str, width: usize) -> Vec<usize> {
    let width = width.max(1);
    let chars: Vec<char> = line.chars().collect();
    // Prefix sums of column widths, so "does the rest of the line still
    // overflow?" and "where does this row end?" are both lookups rather than
    // a re-scan of the tail once per row.
    let mut prefix = Vec::with_capacity(chars.len() + 1);
    prefix.push(0usize);
    for &c in &chars {
        prefix.push(prefix[prefix.len() - 1] + char_columns(c));
    }

    let mut starts = vec![0usize];
    let mut row_start = 0usize;

    while prefix[chars.len()] - prefix[row_start] > width {
        // First char index that does *not* fit in `width` columns from
        // `row_start`. A zero-width char always fits, so a combining mark
        // stays on the row with the character it modifies.
        let mut window_end = row_start;
        while window_end < chars.len() && prefix[window_end + 1] - prefix[row_start] <= width {
            window_end += 1;
        }
        // A single character wider than the whole pane fits nowhere; without
        // this it would make no progress and loop forever.
        let window_end = window_end.max(row_start + 1);
        let break_at = (row_start..window_end).rev().find(|&j| chars[j] == ' ');
        let next_start = match break_at {
            Some(space_idx) if space_idx + 1 > row_start => space_idx + 1,
            _ => window_end,
        };
        if next_start >= chars.len() {
            // Only reachable when the window was forced above: the last
            // character is wider than the pane, so the tail still "overflows"
            // with nothing left to move down. Pushing here would add an empty
            // row and inflate every row count taken from this table.
            break;
        }
        starts.push(next_start);
        row_start = next_start;
    }

    starts
}

/// Convert a logical `(row, col)` cursor position (as returned by
/// `TextEditor::cursor_row_col`, i.e. char-counts since the last `\n`) into
/// a visual `(row, col)` after word-wrapping `text` at `width` columns.
///
/// The column that comes back is a **column**, not a char count: the two
/// differ on any row holding a wide character or a combining mark, and the
/// caller is placing a terminal cursor.
///
/// The returned row can be **one past** the last row [`wrap_row_starts`]
/// reports for the line. A cursor at the end of a row that is exactly
/// `width` columns wide has no column left on that row to sit in, so it
/// belongs at column 0 of the row below — which holds no text yet, and so is
/// not a row that wrapping produces.
///
/// Reporting column `width` instead was finding 7: `cursor_position_in`
/// clamps a column to the pane, so the cursor sat on top of the last
/// character, exactly where it had been one keystroke earlier, and then
/// reappeared at column 1 of the next row having never been seen at column 0.
/// It looked like the cursor stalling for one keystroke at every exact
/// multiple of the pane width. The fix is here and not in
/// [`wrap_row_starts`], whose `>` is right: a line of exactly `width`
/// columns occupies one row, and making it two would put a blank row after
/// every full-width line and inflate every row count taken from it.
pub fn visual_position(text: &str, row: u16, col: u16, width: usize) -> (u16, u16) {
    let width = width.max(1);
    let mut visual_row = 0u16;
    for (i, line) in text.split('\n').enumerate() {
        let starts = wrap_row_starts(line, width);
        if i as u16 == row {
            let col = col as usize;
            let row_idx = starts.iter().rposition(|&s| col >= s).unwrap_or(0);
            let visual_col: usize = line
                .chars()
                .skip(starts[row_idx])
                .take(col.saturating_sub(starts[row_idx]))
                .map(char_columns)
                .sum();
            // Only reachable on the line's last row: anywhere else, the next
            // row's start is `<= col` and `rposition` would have chosen it.
            // `>=` rather than `==` because a character wider than the pane is
            // placed anyway (see `wrap_row_starts`), which can carry the cursor
            // past the edge rather than exactly onto it.
            if visual_col >= width {
                return (visual_row + row_idx as u16 + 1, 0);
            }
            return (visual_row + row_idx as u16, visual_col as u16);
        }
        visual_row += starts.len() as u16;
    }
    // `row` is past the last line of `text`, which `cursor_row_col` cannot
    // produce since it derives the row from the text itself.
    (visual_row, col)
}

/// Word-wrap already-highlighted `lines` at `width` columns, preserving span
/// styling across breaks. Each input [`Line`] may expand into several output
/// rows; an empty input line stays a single empty output line.
///
/// A line-level style is carried onto every row the line produces. The
/// highlighters only ever style spans, so this looks redundant — it is not:
/// the diagnostic and check-result lists mark their selected entry with
/// `Line::style(…REVERSED)`, and dropping it here deleted the highlight.
pub fn wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for line in lines {
        let mut chars_styles: Vec<(char, Style)> = Vec::new();
        for span in &line.spans {
            for c in span.content.chars() {
                chars_styles.push((c, span.style));
            }
        }
        let plain: String = chars_styles.iter().map(|(c, _)| *c).collect();
        let starts = wrap_row_starts(&plain, width);

        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(chars_styles.len());
            out.push(Line::from(spans_for(&chars_styles[start..end])).style(line.style));
        }
    }
    out
}

fn spans_for(chars_styles: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cur_style: Option<Style> = None;
    let mut cur_text = String::new();
    for &(c, style) in chars_styles {
        if cur_style == Some(style) {
            cur_text.push(c);
        } else {
            if !cur_text.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut cur_text),
                    cur_style.unwrap(),
                ));
            }
            cur_style = Some(style);
            cur_text.push(c);
        }
    }
    if !cur_text.is_empty() {
        spans.push(Span::styled(cur_text, cur_style.unwrap()));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn short_line_is_not_wrapped() {
        assert_eq!(wrap_row_starts("hello", 10), vec![0]);
    }

    #[test]
    fn wraps_at_a_space() {
        // "hello world" is 11 chars; width 6 -> "hello " fits in the window
        // (indices 0..6), the last space in that window is at index 5.
        assert_eq!(wrap_row_starts("hello world", 6), vec![0, 6]);
    }

    #[test]
    fn hard_breaks_a_word_longer_than_width() {
        assert_eq!(wrap_row_starts("abcdefgh", 3), vec![0, 3, 6]);
    }

    #[test]
    fn empty_line_has_a_single_row() {
        assert_eq!(wrap_row_starts("", 10), vec![0]);
    }

    #[test]
    fn wraps_wide_characters_by_column_not_by_character() {
        // Four CJK characters are eight columns, so a four-column pane takes
        // two of them per row. Counting chars put four on a row and let the
        // renderer cut the row in half at the pane edge.
        assert_eq!(wrap_row_starts("漢字漢字", 4), vec![0, 2]);
    }

    #[test]
    fn a_wide_character_is_not_split_across_the_pane_edge() {
        // "a漢" is exactly three columns; the second wide char has to go to
        // the next row, because half of it cannot be drawn.
        assert_eq!(wrap_row_starts("a漢字", 3), vec![0, 2]);
    }

    #[test]
    fn a_character_wider_than_the_pane_still_terminates() {
        // Nowhere for it to fit, so it is placed anyway, one per row, rather
        // than wrapping forever.
        assert_eq!(wrap_row_starts("漢字", 1), vec![0, 1]);
    }

    #[test]
    fn a_combining_mark_stays_with_the_character_it_modifies() {
        // "e" + combining acute is one column, so index 2 (the mark) must not
        // become a row start — that would draw the accent on the row below.
        let text = "ae\u{301}bc";
        let starts = wrap_row_starts(text, 2);
        assert!(!starts.contains(&2), "the mark starts a row: {starts:?}");
        assert_eq!(starts, vec![0, 3]);
    }

    #[test]
    fn visual_position_counts_columns_not_characters() {
        // Two CJK characters put the cursor in column 4, not column 2.
        assert_eq!(visual_position("漢字ab", 0, 2, 10), (0, 4));
    }

    #[test]
    fn visual_position_before_first_wrap_point_is_unchanged() {
        assert_eq!(visual_position("hello world", 0, 3, 6), (0, 3));
    }

    #[test]
    fn visual_position_after_wrap_point_moves_to_next_row() {
        // col 8 ('o' in "world") is past the wrap at char index 6.
        assert_eq!(visual_position("hello world", 0, 8, 6), (1, 2));
    }

    #[test]
    fn visual_position_at_the_end_of_a_full_width_row_moves_to_the_next_row() {
        // Finding 7. "abcdef" exactly fills a 6-column pane, so the cursor at
        // the end of it has no column left on that row: it belongs at column 0
        // of the row below. Reporting (0, 6) instead put it back on top of the
        // 'f', where it had been one keystroke earlier.
        assert_eq!(visual_position("abcdef", 0, 6, 6), (1, 0));
        // And the next keystroke continues from there, rather than jumping to
        // column 1 of a row the cursor was never seen on.
        assert_eq!(visual_position("abcdefg", 0, 7, 6), (1, 1));
    }

    #[test]
    fn a_full_width_line_still_renders_as_one_row() {
        // The other half of finding 7: the fix belongs in `visual_position`,
        // not in `wrap_row_starts`. A line of exactly `width` characters is
        // one row — making it two would put a blank row after every full-width
        // line and inflate every row count taken from it, including the scroll
        // clamp's.
        assert_eq!(wrap_row_starts("abcdef", 6), vec![0]);
        assert_eq!(wrap_lines(vec![Line::from("abcdef")], 6).len(), 1);
    }

    #[test]
    fn visual_position_accounts_for_earlier_logical_lines() {
        assert_eq!(visual_position("hello world\nhi", 1, 1, 6), (2, 1));
    }

    #[test]
    fn wrap_lines_is_noop_when_line_fits() {
        let lines = vec![Line::from(vec![Span::raw("hello")])];
        let wrapped = wrap_lines(lines, 10);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(render(&wrapped[0]), "hello");
    }

    #[test]
    fn wrap_lines_preserves_styling_across_a_break() {
        let style = Style::default().fg(Color::Green);
        let lines = vec![Line::from(vec![Span::styled("hello world", style)])];
        let wrapped = wrap_lines(lines, 6);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(render(&wrapped[0]), "hello ");
        assert_eq!(render(&wrapped[1]), "world");
        assert_eq!(wrapped[0].spans[0].style, style);
        assert_eq!(wrapped[1].spans[0].style, style);
    }

    #[test]
    fn wrap_lines_carries_a_line_level_style_onto_every_row() {
        // The selected diagnostic row is styled at the line level, not the
        // span level, so dropping `line.style` here loses the highlight on
        // every row of a wrapped selection.
        let style = Style::default().add_modifier(Modifier::REVERSED);
        let lines = vec![Line::from("hello world").style(style)];
        let wrapped = wrap_lines(lines, 6);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0].style, style);
        assert_eq!(wrapped[1].style, style);
    }

    #[test]
    fn wrap_lines_preserves_empty_lines() {
        let lines = vec![
            Line::from(vec![Span::raw("a")]),
            Line::from(Vec::<Span>::new()),
        ];
        let wrapped = wrap_lines(lines, 10);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(render(&wrapped[1]), "");
    }

    fn render(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }
}
