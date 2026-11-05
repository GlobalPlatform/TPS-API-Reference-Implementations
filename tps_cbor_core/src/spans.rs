/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The CBOR to EDN span table.
//!
//! [`SpanMap`] renders CBOR as EDN and keeps the table relating each decoded item's CBOR byte
//! range to the byte range of its rendered text. It is what lets a click on a hex byte highlight
//! the right EDN line, a click on an EDN line highlight the right hex bytes, and -- the feature
//! that justifies the whole mechanism -- a CDDL validation trace event highlight the same item in
//! both views at once.
//!
//! This is the subtlest code in the project, so both lookup directions are "smallest containing
//! range" searches rather than exact-match-only ones: the trace reports ranges that cover a map
//! entry's key and value together, which is a range no single rendered item owns.

use std::ops::Range;

use tps_minicbor::Decoder;

use crate::error::Result;

/// One entry of the table: a CBOR byte range and the EDN byte range that renders it.
pub type SpanEntry = (Range<usize>, Range<usize>);

/// EDN text for a CBOR buffer, together with the ranges relating the two.
#[derive(Debug, Clone)]
pub struct SpanMap {
    edn: String,
    entries: Vec<SpanEntry>,
}

impl SpanMap {
    /// Render `cbor` as pretty-printed EDN and record the span table.
    ///
    /// The text produced is byte-identical to [`crate::convert::cbor_to_edn`], so a document can
    /// be rendered once and the map built later without the displayed text shifting.
    pub fn build(cbor: &[u8]) -> Result<Self> {
        let decoder = Decoder::new(cbor);
        let mut edn = String::new();
        let entries = decoder
            .write_diagnostic_pretty_all_fmt_with_spans(&mut edn, Some(0))
            .map_err(|error| crate::convert::diagnostic_error(cbor, error))?;
        Ok(Self { edn, entries })
    }

    /// The rendered EDN text.
    pub fn edn(&self) -> &str {
        &self.edn
    }

    /// Take ownership of the rendered EDN text, discarding the table.
    pub fn into_edn(self) -> String {
        self.edn
    }

    /// The raw table, in the order the decoder emits it: **post-order**, so a container appears
    /// after everything it contains and the outermost item is last.
    ///
    /// Neither [`SpanMap::edn_range`] nor [`SpanMap::cbor_range`] relies on that -- both rank
    /// candidates by width -- but a caller walking the table directly needs to know, because the
    /// first entry is the innermost leaf, not the document.
    pub fn entries(&self) -> &[SpanEntry] {
        &self.entries
    }

    /// Find the EDN byte range that best corresponds to the CBOR byte range `target`.
    ///
    /// Prefers an exact match; failing that, the *smallest* mapped CBOR range that fully contains
    /// `target`. Ported from the old backend's `resolve_edn_range`
    /// (`tps_cbor_gui/src-tauri/src/main.rs:347-361`), whose behaviour the validation tracer
    /// depends on: a mismatch reported against a map entry resolves to the enclosing map, which is
    /// the smallest thing actually rendered.
    pub fn edn_range(&self, target: &Range<usize>) -> Option<Range<usize>> {
        self.entries
            .iter()
            .find(|(cbor, _)| cbor == target)
            .or_else(|| {
                self.entries
                    .iter()
                    .filter(|(cbor, _)| cbor.start <= target.start && cbor.end >= target.end)
                    .min_by_key(|(cbor, _)| cbor.end - cbor.start)
            })
            .map(|(_, edn)| edn.clone())
    }

    /// Find the CBOR byte range whose rendered text best corresponds to the EDN range `target`.
    ///
    /// The inverse of [`SpanMap::edn_range`], and the direction the old GUI never implemented --
    /// clicking in the EDN view highlighted nothing in the hex view. Prefers an exact match, then
    /// the smallest mapped EDN range containing `target`.
    pub fn cbor_range(&self, target: &Range<usize>) -> Option<Range<usize>> {
        self.entries
            .iter()
            .find(|(_, edn)| edn == target)
            .or_else(|| {
                self.entries
                    .iter()
                    .filter(|(_, edn)| edn.start <= target.start && edn.end >= target.end)
                    .min_by_key(|(_, edn)| edn.end - edn.start)
            })
            .map(|(cbor, _)| cbor.clone())
    }

    /// The CBOR range of the innermost item rendered at EDN byte `offset`.
    ///
    /// Convenience for a caret position, which has no width.
    pub fn cbor_at_edn_offset(&self, offset: usize) -> Option<Range<usize>> {
        self.cbor_range(&(offset..offset + 1))
    }

    /// The EDN range of the innermost item that covers CBOR byte `offset`.
    ///
    /// Convenience for a hex-view cursor, which selects one byte.
    pub fn edn_at_cbor_offset(&self, offset: usize) -> Option<Range<usize>> {
        self.edn_range(&(offset..offset + 1))
    }

    /// Resolve an EDN byte range to 1-based start and end line numbers.
    ///
    /// Line numbers, not positions, because every consumer of this wants to scroll a view to a
    /// line. Uses the one shared conversion; see [`crate::diag`].
    pub fn edn_lines(&self, range: &Range<usize>) -> (usize, usize) {
        (
            crate::diag::Position::at(&self.edn, range.start).line,
            crate::diag::Position::at(&self.edn, range.end).line,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    /// `{1: [2, 3], "k": h'ff'}`
    const NESTED: &[u8] = &[0xa2, 0x01, 0x82, 0x02, 0x03, 0x61, b'k', 0x41, 0xff];

    #[test]
    fn the_rendered_text_matches_the_plain_conversion() {
        let map = SpanMap::build(NESTED).expect("build");
        assert_eq!(map.edn(), crate::convert::cbor_to_edn(NESTED).expect("edn"));
    }

    /// The table is post-order: the document is the *last* entry, not the first.
    ///
    /// Worth pinning, because the obvious guess is the other way round and a caller that walks
    /// `entries()[0]` expecting the whole document gets a leaf instead.
    #[test]
    fn the_last_entry_covers_the_whole_buffer() {
        let map = SpanMap::build(NESTED).expect("build");
        let (cbor, edn) = map.entries().last().expect("at least one entry");
        assert_eq!(*cbor, 0..NESTED.len());
        assert_eq!(edn.start, 0);
        assert_eq!(edn.end, map.edn().len());

        // ... and every container really does follow its contents.
        for (i, (outer, _)) in map.entries().iter().enumerate() {
            for (inner, _) in &map.entries()[i + 1..] {
                assert!(
                    !(inner.start >= outer.start && inner.end <= outer.end && inner != outer),
                    "entry {i} {outer:?} is followed by the nested {inner:?}"
                );
            }
        }
    }

    #[test]
    fn an_exact_cbor_range_resolves_to_its_own_text() {
        let map = SpanMap::build(NESTED).expect("build");
        // The nested array occupies CBOR bytes 2..5. Its EDN range is a slice of the pretty-printed
        // document, so it carries the document's indentation rather than being re-rendered flat.
        let edn = map.edn_range(&(2..5)).expect("array maps");
        assert_eq!(&map.edn()[edn.clone()], "[\n    2,\n    3\n  ]");
    }

    /// The behaviour the tracer relies on: a range no single item owns falls back to the smallest
    /// container rather than returning nothing.
    #[test]
    fn a_range_spanning_a_map_entry_falls_back_to_the_container() {
        let map = SpanMap::build(NESTED).expect("build");
        // Bytes 1..5 are the key `1` and its value `[2, 3]` together -- not one rendered item.
        let edn = map.edn_range(&(1..5)).expect("falls back");
        let text = &map.edn()[edn];
        assert!(text.starts_with('{'), "expected the map, got {text:?}");
    }

    #[test]
    fn every_cbor_offset_resolves_into_the_edn() {
        let map = SpanMap::build(NESTED).expect("build");
        for offset in 0..NESTED.len() {
            let edn = map
                .edn_at_cbor_offset(offset)
                .unwrap_or_else(|| panic!("byte {offset} resolved to nothing"));
            assert!(
                edn.end <= map.edn().len(),
                "byte {offset} mapped outside the text"
            );
            assert!(
                map.edn().is_char_boundary(edn.start) && map.edn().is_char_boundary(edn.end),
                "byte {offset} mapped to a non-boundary range {edn:?}"
            );
        }
    }

    #[test]
    fn the_reverse_direction_round_trips() {
        let map = SpanMap::build(NESTED).expect("build");
        for (cbor, edn) in map.entries() {
            // Each entry's own EDN range must resolve back to its own CBOR range.
            assert_eq!(map.cbor_range(edn).as_ref(), Some(cbor));
        }
    }

    #[test]
    fn a_caret_inside_a_token_finds_the_innermost_item() {
        let map = SpanMap::build(NESTED).expect("build");
        let offset = map.edn().find("h'ff'").expect("byte string is rendered");
        let cbor = map.cbor_at_edn_offset(offset).expect("resolves");
        assert_eq!(cbor, 7..9);
    }

    #[test]
    fn lines_are_one_based() {
        let map = SpanMap::build(NESTED).expect("build");
        assert_eq!(map.edn_lines(&(0..0)), (1, 1));
        let last = map.edn().len();
        let (start, _) = map.edn_lines(&(last..last));
        assert!(start > 1, "pretty EDN should span several lines");
    }

    #[test]
    fn a_scalar_has_exactly_one_entry() {
        let map = SpanMap::build(&[0x18, 0x2a]).expect("build");
        assert_eq!(map.edn(), "42");
        assert_eq!(map.entries(), [(0..2, 0..2)]);
    }

    #[test]
    fn malformed_cbor_reports_an_offset() {
        let error = SpanMap::build(&[0x83, 0x01]).expect_err("truncated");
        assert!(
            matches!(error, CoreError::CborDecode { .. }),
            "unexpected error: {error}"
        );
    }
}
