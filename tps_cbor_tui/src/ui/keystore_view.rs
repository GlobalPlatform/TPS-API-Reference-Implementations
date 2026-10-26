/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Pure rendering helpers for the key store browser.

use ratatui::text::Line;

use crate::keystore::KeyMeta;

/// Render the key list, marking the cursor row and the active key (if any).
pub fn render_key_list(keys: &[KeyMeta], selected: usize, active_alias: Option<&str>) -> Vec<Line<'static>> {
    if keys.is_empty() {
        return vec![Line::from("(no keys — press 'g' to generate or 'i' to import)")];
    }

    keys.iter()
        .enumerate()
        .map(|(i, k)| {
            let cursor = if i == selected { ">" } else { " " };
            let active = if active_alias == Some(k.alias.as_str()) { "*" } else { " " };
            let alg = k
                .algorithm
                .map(|a| a.to_string())
                .unwrap_or_else(|| "-".to_owned());
            let curve = k.curve.clone().unwrap_or_else(|| "-".to_owned());
            Line::from(format!(
                "{cursor}{active} {:<16} {:<10} alg={alg:<6} curve={curve:<8} private={}",
                k.alias, k.key_type, k.has_private
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(alias: &str) -> KeyMeta {
        KeyMeta {
            alias: alias.to_owned(),
            key_type: "Symmetric".to_owned(),
            algorithm: Some(1),
            curve: None,
            has_private: false,
            created_at: "0".to_owned(),
        }
    }

    #[test]
    fn empty_list_shows_hint() {
        let lines = render_key_list(&[], 0, None);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn marks_cursor_and_active_rows() {
        let keys = vec![key("a"), key("b")];
        let lines = render_key_list(&keys, 1, Some("a"));
        let rendered: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        assert!(rendered[0].starts_with(" * a"));
        assert!(rendered[1].starts_with(">  b"));
    }
}
