/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Word-wrap for EDN/CDDL panes.
//!
//! ratatui's `Paragraph::wrap()` exists, but the line-count/line-width APIs
//! needed to keep scroll-clamping and cursor placement consistent with what
//! it renders are `#[unstable]` and not callable from outside the crate. So
//! wrapping is done by hand here, once, and reused for line counting
//! (`wrapped_line_count`), rendering (`wrap_lines`) and cursor placement
//! (`visual_position`) — all three stay consistent by construction since
//! they all go through [`wrap_row_starts`].

use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Char-index offsets (into `line.chars()`) where each visual row of one
/// logical line begins under greedy word-wrap at `width` columns. Always
/// starts with `0`. A word longer than `width` is hard-broken so wrapping
/// still terminates.
pub fn wrap_row_starts(line: &str, width: usize) -> Vec<usize> {
    let width = width.max(1);
    let chars: Vec<char> = line.chars().collect();
    let mut starts = vec![0usize];
    let mut row_start = 0usize;

    while chars.len() - row_start > width {
        let window_end = row_start + width;
        let break_at = (row_start..window_end).rev().find(|&j| chars[j] == ' ');
        let next_start = match break_at {
            Some(space_idx) if space_idx + 1 > row_start => space_idx + 1,
            _ => window_end,
        };
        starts.push(next_start);
        row_start = next_start;
    }

    starts
}

/// Total number of visual rows `text` occupies once each of its logical
/// (`\n`-separated) lines is word-wrapped at `width` columns.
pub fn wrapped_line_count(text: &str, width: usize) -> usize {
    text.split('\n').map(|line| wrap_row_starts(line, width).len()).sum()
}

/// Convert a logical `(row, col)` cursor position (as returned by
/// `TextEditor::cursor_row_col`, i.e. char-counts since the last `\n`) into
/// a visual `(row, col)` after word-wrapping `text` at `width` columns.
pub fn visual_position(text: &str, row: u16, col: u16, width: usize) -> (u16, u16) {
    let mut visual_row = 0u16;
    for (i, line) in text.split('\n').enumerate() {
        let starts = wrap_row_starts(line, width);
        if i as u16 == row {
            let col = col as usize;
            let row_idx = starts.iter().rposition(|&s| col >= s).unwrap_or(0);
            let visual_col = col - starts[row_idx];
            return (visual_row + row_idx as u16, visual_col as u16);
        }
        visual_row += starts.len() as u16;
    }
    (visual_row, col)
}

/// Word-wrap already-highlighted `lines` at `width` columns, preserving span
/// styling across breaks. Each input [`Line`] may expand into several output
/// rows; an empty input line stays a single empty output line.
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
            out.push(Line::from(spans_for(&chars_styles[start..end])));
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
                spans.push(Span::styled(std::mem::take(&mut cur_text), cur_style.unwrap()));
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
    use ratatui::style::Color;

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
    fn wrapped_line_count_sums_rows_across_logical_lines() {
        assert_eq!(wrapped_line_count("hello world\nhi", 6), 3);
    }

    #[test]
    fn wrapped_line_count_matches_split_for_empty_text() {
        assert_eq!(wrapped_line_count("", 80), 1);
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
    fn wrap_lines_preserves_empty_lines() {
        let lines = vec![Line::from(vec![Span::raw("a")]), Line::from(Vec::<Span>::new())];
        let wrapped = wrap_lines(lines, 10);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(render(&wrapped[1]), "");
    }

    fn render(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }
}
