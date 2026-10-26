/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! CBOR hex-dump rendering.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Render `bytes` as a classic offset/hex/ASCII dump, `width` bytes per row.
pub fn render_hex_dump(bytes: &[u8], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = Vec::with_capacity(bytes.len() / width + 1);

    for (row, chunk) in bytes.chunks(width).enumerate() {
        let offset = row * width;
        let mut spans = vec![Span::styled(
            format!("{offset:08x}  "),
            Style::default().fg(Color::DarkGray),
        )];

        for i in 0..width {
            if let Some(b) = chunk.get(i) {
                spans.push(Span::styled(
                    format!("{b:02x} "),
                    Style::default().fg(Color::Cyan),
                ));
            } else {
                spans.push(Span::raw("   "));
            }
            if i + 1 == width / 2 {
                spans.push(Span::raw(" "));
            }
        }

        spans.push(Span::raw(" "));
        let ascii: String = chunk
            .iter()
            .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
            .collect();
        spans.push(Span::styled(ascii, Style::default().fg(Color::White)));

        lines.push(Line::from(spans));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_produces_no_lines() {
        assert!(render_hex_dump(&[], 16).is_empty());
    }

    #[test]
    fn single_row_contains_offset_and_ascii() {
        let lines = render_hex_dump(b"Hi!", 16);
        assert_eq!(lines.len(), 1);
        let rendered: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(rendered.starts_with("00000000"));
        assert!(rendered.contains("48 69 21"));
        assert!(rendered.contains("Hi!"));
    }

    #[test]
    fn wraps_at_configured_width() {
        let bytes: Vec<u8> = (0..20).collect();
        let lines = render_hex_dump(&bytes, 16);
        assert_eq!(lines.len(), 2);
        let second: String = lines[1]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(second.starts_with("00000010"));
    }
}
