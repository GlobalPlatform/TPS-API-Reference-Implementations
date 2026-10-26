/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Pure rendering helpers for the COSE encrypt/decrypt screen.

use ratatui::text::Line;

/// Render the encrypt/decrypt screen body: active key summary + last status.
pub fn render_encrypt_screen(
    active_alias: Option<&str>,
    artifact_len: usize,
    status: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!(
            "active key: {}",
            active_alias.unwrap_or("(none — select one in the key store)")
        )),
        Line::from(format!("artifact: {artifact_len} bytes")),
        Line::from(""),
        Line::from("e: encrypt artifact   d: decrypt artifact   Esc: back"),
    ];
    if let Some(s) = status {
        lines.push(Line::from(""));
        lines.push(Line::from(s.to_owned()));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_active_key_and_status() {
        let lines = render_encrypt_screen(Some("my-key"), 10, Some("encrypted"));
        let rendered: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        assert!(rendered[0].contains("my-key"));
        assert!(rendered.iter().any(|l| l == "encrypted"));
    }

    #[test]
    fn renders_hint_when_no_active_key() {
        let lines = render_encrypt_screen(None, 0, None);
        assert!(lines[0].to_string().contains("none"));
    }
}
