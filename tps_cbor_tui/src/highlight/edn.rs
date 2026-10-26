/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! EDN syntax highlighting: `tree-sitter-edn` -> ratatui [`Line`]s.
//!
//! Mirrors `tps_ts_highlight_tool/src/main.rs`'s `HighlightEvent` walk, but
//! produces ratatui [`Style`]/[`Span`] output instead of ANSI escape codes.
//! The capture-name -> style table is intentionally duplicated rather than
//! shared with `tps_ts_highlight_tool` (see `tps_cbor_tui/PLAN.md`).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

const HIGHLIGHT_NAMES: &[&str] = &[
    "type",
    "type.parameter",
    "type.builtin",
    "property",
    "operator",
    "keyword.operator",
    "number",
    "string",
    "string.special",
    "punctuation.bracket",
    "punctuation.delimiter",
    "comment",
    "comment.documentation",
    "boolean",
    "constant.builtin",
    "constant.numeric.float",
];

fn style_for(name: &str) -> Style {
    match name {
        "type" | "type.builtin" => Style::default().fg(Color::Cyan),
        "type.parameter" => Style::default().fg(Color::LightCyan),
        "property" => Style::default().fg(Color::Blue),
        "operator" | "keyword.operator" => Style::default().fg(Color::Magenta),
        "number" | "constant.numeric.float" => Style::default().fg(Color::Yellow),
        "string" | "string.special" => Style::default().fg(Color::Green),
        "punctuation.bracket" | "punctuation.delimiter" => Style::default().fg(Color::White),
        "comment" => Style::default().fg(Color::DarkGray),
        "comment.documentation" => Style::default().fg(Color::LightBlue),
        "boolean" | "constant.builtin" => Style::default().fg(Color::LightRed),
        _ => Style::default(),
    }
}

fn edn_highlight_config() -> HighlightConfiguration {
    let mut config = HighlightConfiguration::new(
        tree_sitter_edn::LANGUAGE.into(),
        "edn",
        tree_sitter_edn::HIGHLIGHTS_QUERY,
        "",
        "",
    )
    .expect("build EDN highlight query");
    config.configure(HIGHLIGHT_NAMES);
    config
}

/// Highlight `source` (EDN text) into ratatui [`Line`]s, one per input line.
pub fn highlight_edn(source: &str) -> Vec<Line<'static>> {
    let config = edn_highlight_config();
    let mut highlighter = Highlighter::new();
    let events = highlighter
        // Args after `config`/`source` are `encoding` and tree-sitter-highlight 0.27's
        // `cancellation_flag`: highlighting a single editor buffer is not worth cancelling.
        .highlight(&config, source.as_bytes(), None, None, |_| None)
        .expect("run EDN highlighter");

    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut style_stack: Vec<Style> = vec![Style::default()];

    let push_text = |text: &str, lines: &mut Vec<Vec<Span<'static>>>, style: Style| {
        let mut first = true;
        for segment in text.split('\n') {
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
    };

    for event in events {
        match event.expect("EDN highlight event") {
            HighlightEvent::Source { start, end } => {
                let text = &source[start..end];
                let style = *style_stack.last().unwrap();
                push_text(text, &mut lines, style);
            }
            HighlightEvent::HighlightStart(idx) => {
                style_stack.push(style_for(HIGHLIGHT_NAMES[idx.0]));
            }
            HighlightEvent::HighlightEnd => {
                style_stack.pop();
            }
        }
    }

    lines.into_iter().map(Line::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_a_simple_map() {
        let lines = highlight_edn(r#"{1: "a", 2: [true, false, null]}"#);
        assert_eq!(lines.len(), 1);
        let rendered: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(rendered, r#"{1: "a", 2: [true, false, null]}"#);
    }

    #[test]
    fn splits_into_multiple_lines() {
        let lines = highlight_edn("1\n2\n3");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn empty_source_produces_one_empty_line() {
        let lines = highlight_edn("");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.is_empty());
    }
}
