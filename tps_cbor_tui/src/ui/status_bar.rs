/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Status bar: renders the artifact's provenance chain, current mode, and
//! any pending status/error message — the visible proof that an artifact
//! moves between workflows correctly.

/// Render the status bar text: `mode-hint | provenance-chain` or, when a
/// transient status/error message is set, that message takes priority.
pub fn render_status_line(mode_hint: &str, provenance: &[String], status: Option<&str>) -> String {
    if let Some(s) = status {
        return format!("{s}  |  {}", mode_hint);
    }
    if provenance.is_empty() {
        return mode_hint.to_owned();
    }
    format!("{}  |  {}", mode_hint, provenance.join(" -> "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_mode_hint_alone_when_no_provenance() {
        let line = render_status_line("q: quit", &[], None);
        assert_eq!(line, "q: quit");
    }

    #[test]
    fn joins_provenance_chain() {
        let provenance = vec!["loaded foo.cbor".to_owned(), "signed with 'k1'".to_owned()];
        let line = render_status_line("q: quit", &provenance, None);
        assert_eq!(line, "q: quit  |  loaded foo.cbor -> signed with 'k1'");
    }

    #[test]
    fn status_message_takes_priority_over_provenance() {
        let provenance = vec!["loaded foo.cbor".to_owned()];
        let line = render_status_line("q: quit", &provenance, Some("sign error: invalid key"));
        assert_eq!(line, "sign error: invalid key  |  q: quit");
    }
}
