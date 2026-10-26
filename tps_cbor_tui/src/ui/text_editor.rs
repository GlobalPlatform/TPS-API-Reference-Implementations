/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Minimal hand-rolled line-buffer text editor.
//!
//! ratatui ships no text-editing widget; this is built once here and reused
//! unmodified by the Phase B CDDL editor.

/// Editable text buffer with a byte-offset cursor.
#[derive(Debug, Clone, Default)]
pub struct TextEditor {
    text: String,
    cursor: usize,
}

impl TextEditor {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    #[cfg(test)]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, offset: usize) {
        self.cursor = self.clamp_to_char_boundary(offset);
    }

    /// 0-based `(row, col)` of the cursor, counting characters (not bytes)
    /// since the last newline. Used to place the terminal's visible cursor
    /// over a rendered `Paragraph`, which lays out one character per column
    /// since none of this app's panes wrap text.
    pub fn cursor_row_col(&self) -> (u16, u16) {
        let mut row = 0u16;
        let mut col = 0u16;
        for c in self.text[..self.cursor].chars() {
            if c == '\n' {
                row += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (row, col)
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    #[cfg(test)]
    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the character before the cursor (backspace). No-op at start.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.prev_char_boundary(self.cursor);
        self.text.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// Delete the character at the cursor (forward delete). No-op at end.
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next = self.next_char_boundary(self.cursor);
        self.text.drain(self.cursor..next);
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary(self.cursor);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.next_char_boundary(self.cursor);
        }
    }

    fn clamp_to_char_boundary(&self, mut offset: usize) -> usize {
        offset = offset.min(self.text.len());
        while offset > 0 && !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn prev_char_boundary(&self, from: usize) -> usize {
        let mut i = from - 1;
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn next_char_boundary(&self, from: usize) -> usize {
        let mut i = from + 1;
        while i < self.text.len() && !self.text.is_char_boundary(i) {
            i += 1;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_backspace() {
        let mut e = TextEditor::new("");
        e.insert_str("hello");
        assert_eq!(e.text(), "hello");
        e.backspace();
        assert_eq!(e.text(), "hell");
        assert_eq!(e.cursor(), 4);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut e = TextEditor::new("abc");
        e.set_cursor(0);
        e.backspace();
        assert_eq!(e.text(), "abc");
        assert_eq!(e.cursor(), 0);
    }

    #[test]
    fn move_left_right_respect_utf8_boundaries() {
        let mut e = TextEditor::new("a\u{00e9}b"); // a, e-acute (2 bytes), b
        e.set_cursor(e.text().len());
        e.move_left();
        assert!(e.text().is_char_boundary(e.cursor()));
        e.move_left();
        assert!(e.text().is_char_boundary(e.cursor()));
        e.move_right();
        assert!(e.text().is_char_boundary(e.cursor()));
    }

    #[test]
    fn delete_forward_at_end_is_noop() {
        let mut e = TextEditor::new("abc");
        e.set_cursor(3);
        e.delete_forward();
        assert_eq!(e.text(), "abc");
    }

    #[test]
    fn cursor_row_col_tracks_newlines() {
        let mut e = TextEditor::new("ab\ncde");
        e.set_cursor(0);
        assert_eq!(e.cursor_row_col(), (0, 0));
        e.set_cursor(2);
        assert_eq!(e.cursor_row_col(), (0, 2));
        e.set_cursor(3);
        assert_eq!(e.cursor_row_col(), (1, 0));
        e.set_cursor(6);
        assert_eq!(e.cursor_row_col(), (1, 3));
    }
}
