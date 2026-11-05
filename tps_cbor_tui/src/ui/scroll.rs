/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Scroll offsets for the selectable list panes (diagnostics, key store,
//! check results) and for the single-line input prompt.
//!
//! These panes have no scroll offset of their own: the offset is derived from
//! the selection — or, in the prompt's case, the cursor — on every frame, so
//! there is no stored offset that can disagree with it or go stale across a
//! resize. The cost of that is that the offset is a pure function of the
//! selection and therefore cannot tell which way the selection moved — see
//! [`offset_for_selection`] and [`horizontal_window`].

use ratatui::text::Line;

/// Word-wrap each list entry to `width`, returning the rendered rows together
/// with the number of rows each entry occupies.
///
/// The counts are the wrap output's own lengths rather than a separate
/// calculation, so the rows a pane renders and the rows
/// [`offset_for_selection`] measures cannot disagree.
pub fn wrap_list(entries: Vec<Line<'static>>, width: usize) -> (Vec<Line<'static>>, Vec<usize>) {
    let mut rows = Vec::new();
    let mut rows_per_entry = Vec::with_capacity(entries.len());
    for entry in entries {
        let wrapped = super::wrap::wrap_lines(vec![entry], width);
        rows_per_entry.push(wrapped.len());
        rows.extend(wrapped);
    }
    (rows, rows_per_entry)
}

/// Scroll offset that keeps entry `selected` visible in a `height`-row
/// viewport, where entry `i` occupies `rows_per_entry[i]` rows.
///
/// The selection is pinned to the bottom edge of the pane once the list is
/// longer than the pane, and sits wherever it falls naturally while the whole
/// first screenful fits. Being a pure function of the selection, this cannot
/// distinguish moving down from moving up, so the selection rides the bottom
/// edge in both directions rather than switching to the top edge on the way
/// back up. It is monotone in `selected`, so stepping through the list moves
/// the view one entry at a time and never jumps.
///
/// An entry taller than the whole pane shows its *first* rows, not its last.
pub fn offset_for_selection(rows_per_entry: &[usize], selected: usize, height: u16) -> u16 {
    if height == 0 {
        return 0;
    }
    let rows_above: usize = rows_per_entry.iter().take(selected).sum();
    let selected_rows = rows_per_entry.get(selected).copied().unwrap_or(0);
    let offset = (rows_above + selected_rows)
        .saturating_sub(height as usize)
        .min(rows_above);
    offset.min(u16::MAX as usize) as u16
}

/// [`offset_for_selection`] for a list whose entries are one row each.
pub fn offset_for_row(selected: usize, len: usize, height: u16) -> u16 {
    if height == 0 || len == 0 {
        return 0;
    }
    let selected = selected.min(len - 1);
    let offset = (selected + 1).saturating_sub(height as usize).min(selected);
    offset.min(u16::MAX as usize) as u16
}

/// The slice of a single-line `text` visible in a `width`-column field whose
/// cursor is `cursor` characters from the start of it, together with the
/// cursor's column *within that field*.
///
/// Finding 5: the input prompt had no horizontal scrolling, so a path longer
/// than the box was cut off at its right edge and the cursor stopped moving
/// there. Nothing was lost — the whole path was in the buffer and still
/// loaded — but there was no way to see what had been typed or where the
/// cursor was in it.
///
/// Like the vertical offsets above this is derived from the cursor each frame
/// rather than stored, so it cannot tell which way the cursor last moved: the
/// window is the widest one *ending* at the cursor, which rides the right
/// edge of the field once the text is longer than it. One column is kept for
/// the cursor to sit in, and any columns left over past it are filled with
/// what follows.
///
/// Measured in columns throughout, so a path holding wide characters scrolls
/// by what is drawn rather than by how many `char`s it has.
pub fn horizontal_window(text: &str, cursor: usize, width: usize) -> (&str, u16) {
    let width = width.max(1);
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let cursor = cursor.min(chars.len());

    let mut start = cursor;
    let mut before = 0usize;
    while start > 0 {
        let w = super::wrap::char_columns(chars[start - 1].1);
        if before + w > width - 1 {
            break;
        }
        before += w;
        start -= 1;
    }

    let mut end = cursor;
    let mut remaining = width - before;
    while end < chars.len() {
        let w = super::wrap::char_columns(chars[end].1);
        if w > remaining {
            break;
        }
        remaining -= w;
        end += 1;
    }

    let from = chars.get(start).map_or(text.len(), |&(b, _)| b);
    let to = chars.get(end).map_or(text.len(), |&(b, _)| b);
    (&text[from..to], before as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Modifier, Style};

    #[test]
    fn a_selection_inside_the_first_screenful_does_not_scroll() {
        assert_eq!(offset_for_row(5, 30, 21), 0);
        assert_eq!(offset_for_row(20, 30, 21), 0);
    }

    #[test]
    fn a_selection_below_the_pane_is_pinned_to_the_bottom_edge() {
        // 31 entries in a 21-row pane: selecting the last one shows the last
        // 21, with the selection on the bottom row.
        assert_eq!(offset_for_row(30, 31, 21), 10);
        assert_eq!(offset_for_row(21, 31, 21), 1);
    }

    #[test]
    fn the_first_entry_is_always_at_the_top() {
        assert_eq!(offset_for_row(0, 31, 21), 0);
    }

    #[test]
    fn an_out_of_range_selection_is_clamped_to_the_last_entry() {
        assert_eq!(offset_for_row(99, 31, 21), 10);
    }

    #[test]
    fn a_zero_height_viewport_does_not_scroll() {
        assert_eq!(offset_for_row(30, 31, 0), 0);
        assert_eq!(offset_for_selection(&[1, 1, 1], 2, 0), 0);
    }

    #[test]
    fn wrapped_entries_above_the_selection_count_their_full_height() {
        // Three entries of 3 rows each in a 4-row pane: selecting the last
        // one must skip all 6 rows of the first two, not 2 of them.
        assert_eq!(offset_for_selection(&[3, 3, 3], 2, 4), 5);
        assert_eq!(offset_for_selection(&[3, 3, 3], 1, 4), 2);
    }

    #[test]
    fn an_entry_taller_than_the_pane_shows_its_first_rows() {
        assert_eq!(offset_for_selection(&[2, 10], 1, 4), 2);
    }

    #[test]
    fn a_text_shorter_than_the_field_is_not_scrolled() {
        assert_eq!(horizontal_window("short/path", 10, 31), ("short/path", 10));
        assert_eq!(horizontal_window("short/path", 3, 31), ("short/path", 3));
    }

    #[test]
    fn a_text_longer_than_the_field_rides_its_right_edge() {
        // The 74-character path of finding 5, in the 31-column field the
        // Input box leaves after its label. The tail is visible and the
        // cursor is on the last column rather than stuck mid-text.
        let path = "/home/jodonogh/Code/OpenSource/TPS-API-Reference-Implementations/long.cddl";
        assert_eq!(path.chars().count(), 74);
        let (visible, col) = horizontal_window(path, 74, 31);
        assert_eq!(col, 30);
        assert_eq!(visible, &path[74 - 30..]);
    }

    #[test]
    fn the_cursor_moved_back_brings_the_text_before_it_into_view() {
        let path = "/home/jodonogh/Code/OpenSource/TPS-API-Reference-Implementations/long.cddl";
        let (visible, col) = horizontal_window(path, 40, 31);
        assert_eq!(col, 30);
        // 30 columns of text before the cursor, and the character the cursor
        // is sitting on in the 31st.
        assert_eq!(visible, &path[10..41]);
    }

    #[test]
    fn the_start_of_the_text_is_not_scrolled_away() {
        let path = "/home/jodonogh/Code/OpenSource/long.cddl";
        assert_eq!(horizontal_window(path, 0, 31), (&path[..31], 0));
    }

    #[test]
    fn the_window_is_measured_in_columns_not_characters() {
        // 16 CJK characters are 32 columns, so a 10-column field holds five of
        // them: four before the cursor and one after it.
        let text = "漢字漢字漢字漢字漢字漢字漢字漢字";
        let (visible, col) = horizontal_window(text, 8, 10);
        assert_eq!(col, 8);
        assert_eq!(visible.chars().count(), 5);
        assert_eq!(super::super::wrap::str_columns(visible), 10);
    }

    #[test]
    fn a_field_with_no_room_left_still_returns_something_renderable() {
        let (visible, col) = horizontal_window("abc", 3, 0);
        assert_eq!(col, 0);
        assert!(visible.chars().count() <= 1);
    }

    #[test]
    fn wrap_list_counts_the_rows_it_produced() {
        let entries = vec![
            Line::from("short"),
            Line::from("hello world"),
            Line::from("tiny"),
        ];
        let (rows, per_entry) = wrap_list(entries, 6);
        assert_eq!(per_entry, vec![1, 2, 1]);
        assert_eq!(rows.len(), per_entry.iter().sum::<usize>());
    }

    #[test]
    fn wrap_list_keeps_the_selection_highlight_on_every_row() {
        let style = Style::default().add_modifier(Modifier::REVERSED);
        let entries = vec![Line::from("hello world").style(style)];
        let (rows, per_entry) = wrap_list(entries, 6);
        assert_eq!(per_entry, vec![2]);
        assert!(rows.iter().all(|r| r.style == style));
    }
}
