/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! CDDL syntax highlighting: `tps_cddl::tokenize` -> ratatui [`Line`]s.
//!
//! Simpler than EDN highlighting: `tokenize()` already returns a flat,
//! sorted, gap-free `Token{start,end,kind}` list, so this only needs to map
//! `TokenKind` -> `Style` and split on `\n`.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use tps_cddl::tokenize::{TokenKind, tokenize};

fn style_for(kind: TokenKind) -> Style {
    match kind {
        TokenKind::RuleName => Style::default().fg(Color::Cyan),
        TokenKind::Literal => Style::default().fg(Color::Green),
        TokenKind::Operator => Style::default().fg(Color::Magenta),
        TokenKind::Comment => Style::default().fg(Color::DarkGray),
        TokenKind::Default => Style::default(),
    }
}

/// Highlight `source` (CDDL text) into ratatui [`Line`]s, one per input line.
pub fn highlight_cddl(source: &str) -> Vec<Line<'static>> {
    let tokens = tokenize(source);

    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    for token in tokens {
        let style = style_for(token.kind);
        let mut first = true;
        for segment in token.text.split('\n') {
            if !first {
                lines.push(Vec::new());
            }
            if !segment.is_empty() {
                lines
                    .last_mut()
                    .unwrap()
                    .push(Span::styled(segment.to_owned(), style));
            }
            first = false;
        }
    }

    lines.into_iter().map(Line::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_a_simple_rule() {
        let lines = highlight_cddl("x = uint");
        assert_eq!(lines.len(), 1);
        let rendered: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "x = uint");
    }

    #[test]
    fn splits_into_multiple_lines() {
        let lines = highlight_cddl("a = uint\nb = tstr");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn empty_source_produces_one_empty_line() {
        let lines = highlight_cddl("");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.is_empty());
    }

    #[test]
    fn invalid_source_falls_back_to_default_styling() {
        let lines = highlight_cddl("@@@ not valid");
        assert_eq!(lines.len(), 1);
        let rendered: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "@@@ not valid");
    }
}
