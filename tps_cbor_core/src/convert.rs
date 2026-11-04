/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! CBOR to EDN, EDN to CBOR, and hex dumping.
//!
//! These three are the workbench's bread and butter, and were three `#[tauri::command]`s in the
//! old backend. The conversions themselves are thin -- `tps_minicbor` does the work -- but the
//! error handling is not: both old commands collapsed their error to `format!("{:?}")`, which is
//! where the EDN byte offset went missing. Here the structured error survives into [`CoreError`].

use tps_minicbor::{DecodeError, Decoder, DiagnosticError, Item};

use crate::error::{CoreError, Result};

/// Bytes per row in a [`hex_dump`], and the group size within a row.
///
/// 16 bytes split 8 + 8 is the `hexdump -C` convention the old GUI followed, and the fixtures'
/// expected output assumes it.
pub const DEFAULT_BYTES_PER_ROW: usize = 16;

/// How many bytes share a group before an extra space is inserted.
const GROUP: usize = 8;

/// Render `bytes` as CBOR extended diagnostic notation, pretty-printed one item per line.
///
/// Trailing bytes after the top-level item are an error: a CBOR *file* holds exactly one item, and
/// silently ignoring the tail would hide a truncated or concatenated document.
pub fn cbor_to_edn(bytes: &[u8]) -> Result<String> {
    let decoder = Decoder::new(bytes);
    let mut edn = String::new();
    decoder
        .write_diagnostic_pretty_all_fmt(&mut edn)
        .map_err(|error| diagnostic_error(bytes, error))?;
    Ok(edn)
}

/// Parse CBOR extended diagnostic notation into CBOR binary.
pub fn edn_to_cbor(edn: &str) -> Result<Vec<u8>> {
    tps_minicbor::parse_edn(edn).map_err(CoreError::edn)
}

/// Check that `bytes` is exactly one well-formed CBOR item, with nothing after it.
///
/// On failure the byte offset at which the decoder stopped is reported, which is what lets the
/// hex view highlight the offending range rather than just colouring the status bar red.
pub fn check_well_formed(bytes: &[u8]) -> Result<()> {
    let mut decoder = Decoder::new(bytes);
    match decoder.decode() {
        Ok(_) => Ok(()),
        Err(source) => Err(CoreError::CborDecode {
            offset: decoder.pos(),
            source,
        }),
    }
}

/// Check the CBOR document a user is editing, which may still be empty.
///
/// The byte counterpart of [`check_edn`], and what a frontend should run after every edit to the
/// CBOR buffer rather than calling [`check_well_formed`] directly.
///
/// **An empty buffer is `Ok(())`.** [`check_well_formed`] calls it
/// [`DecodeError::UnexpectedEof`], which is the right answer for a *file* being validated — an
/// empty file is not a CBOR document — but the wrong one for an editor buffer: with no CBOR open
/// at all, every fresh window reported "CBOR decode error at byte 0: unexpected end of CBOR input"
/// against a document that does not exist, and so did the EDN tab, which lists both slots.
///
/// [`check_well_formed`] keeps the strict behaviour, because a validator asked about a zero-byte
/// file should still say no. The two differ only here, and the split is what lets both answers be
/// correct for their own caller.
pub fn check_cbor(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    check_well_formed(bytes)
}

/// Check that `edn` is exactly one EDN value, discarding the encoding.
///
/// The parse counterpart of [`check_well_formed`], and the check both frontends run after every
/// keystroke on the EDN document.
///
/// **Empty or whitespace-only input is `Ok(())`.** `parse_edn` calls it
/// [`EdnError::UnexpectedEof`](tps_minicbor::EdnError::UnexpectedEof), which is true but useless: a
/// blank document has nothing wrong with it, and reporting it would open every fresh window with a
/// diagnostic already against it and raise one again the moment the user selected all and deleted.
/// The rule lives here rather than at a call site because both frontends have to agree about it.
pub fn check_edn(edn: &str) -> Result<()> {
    if edn.trim().is_empty() {
        return Ok(());
    }
    edn_to_cbor(edn).map(|_| ())
}

/// Convert a [`DiagnosticError`] into a [`CoreError`], recovering a byte offset where one exists.
///
/// [`DiagnosticError`] carries no position of its own, and `write_diagnostic_*` takes `&self`, so
/// the writing decoder's cursor is unobservable. When the cause was a decode error we therefore
/// re-walk the input with a mutable decoder purely to find out how far it gets. That is a second
/// pass over the buffer, but only on the error path, and it is the difference between a placed
/// marker and no marker at all.
pub(crate) fn diagnostic_error(bytes: &[u8], error: DiagnosticError) -> CoreError {
    match error {
        DiagnosticError::Decode(source) => {
            let mut decoder = Decoder::new(bytes);
            // The outcome is deliberately ignored: all that is wanted is how far the cursor got.
            // `decode` rejects trailing bytes just as diagnostic writing does, so it fails in the
            // same place, but even if it succeeded the cursor would still be the right offset.
            let _ = decoder.decode();
            CoreError::CborDecode {
                offset: decoder.pos(),
                source,
            }
        }
        // Neither of these is positional: OutOfSpace cannot occur when writing into a String, and
        // FmtError means the sink itself failed.
        other => CoreError::CborDiagnostic(other.to_string()),
    }
}

/// Classify a decode failure as "the input stops early" rather than "the input is wrong".
///
/// The IDE uses this to decide whether to report an error at all: a hex editor mid-edit is
/// routinely truncated, and flagging every keystroke as malformed is noise.
pub fn is_truncation(error: &CoreError) -> bool {
    matches!(
        error,
        CoreError::CborDecode {
            source: DecodeError::UnexpectedEof,
            ..
        }
    )
}

/// Classify an EDN parse failure as "the input stops early" rather than "the input is wrong".
///
/// The EDN counterpart of [`is_truncation`], and wanted for the same reason: an editor mid-edit
/// routinely holds an unclosed array, and painting every keystroke red is noise. Kept separate
/// because [`is_truncation`] matches only [`CoreError::CborDecode`] and so never fires for EDN.
pub fn is_edn_truncation(error: &CoreError) -> bool {
    matches!(
        error,
        CoreError::EdnParse {
            source: tps_minicbor::EdnError::UnexpectedEof,
            ..
        }
    )
}

/// Parse one EDN value that may be a *fragment* of a larger document.
///
/// Distinct from [`edn_to_cbor`] only in trimming first. `parse_edn` rejects trailing input, and a
/// selection dragged out of an editor almost always carries surrounding whitespace or a newline,
/// which would otherwise be reported as [`tps_minicbor::EdnError::TrailingInput`] -- an error about
/// the selection's edges rather than about anything the user did wrong.
///
/// Offsets in the returned error are relative to the *trimmed* fragment, so a caller placing a
/// marker in the whole document has to rebase them. [`decode_edn_selection`] is the usual entry
/// point and sidesteps that by not reporting a position into the original text at all.
pub fn edn_fragment_to_cbor(text: &str) -> Result<Vec<u8>> {
    edn_to_cbor(text.trim())
}

/// The payload of `cbor`, when `cbor` is exactly one byte string.
///
/// `None` covers both "decodes to something that is not a byte string" and "does not decode",
/// because the one caller treats them identically. [`tps_minicbor::Decoder::next_bytes`] exists but
/// reports a non-byte-string as `InvalidAdditionalInfo`, which would be a misleading error for a
/// case that is not an error here.
pub fn bstr_payload(cbor: &[u8]) -> Option<&[u8]> {
    match Decoder::new(cbor).next() {
        Ok(Item::Bytes(payload)) => Some(payload),
        _ => None,
    }
}

/// `UI_DESIGN.md:93`: the CBOR that a selected EDN fragment stands for.
///
/// A byte string is **unwrapped to its contents**, which is the whole point of the feature: the
/// spec's example is selecting the payload of a `COSE_Sign1`, "which are a BSTR containing CBOR",
/// so returning the `bstr`'s own encoding would leave the user to strip the header by hand.
///
/// Anything else is returned as it encodes. Refusing a non-`bstr` selection would be a gratuitous
/// restriction -- putting the encoding of a selected map or array into the hex view is useful and
/// harmless -- and it would also make the error message depend on the CBOR major type, which is
/// not something the user selected by.
///
/// The unwrapped payload is deliberately *not* checked for well-formedness. `Ide::recheck_cbor`
/// already runs when the result is loaded and marks a bad buffer exactly as it would for a
/// malformed file, so a check here would only duplicate that row in different words.
pub fn decode_edn_selection(text: &str) -> Result<Vec<u8>> {
    let cbor = edn_fragment_to_cbor(text)?;
    Ok(match bstr_payload(&cbor) {
        Some(payload) => payload.to_vec(),
        None => cbor,
    })
}

/// Render `bytes` as lowercase hex, one space between bytes: `a1 01 26`.
///
/// This is the *clipboard* form of a byte range, and deliberately not [`hex_dump`]'s form: a dump
/// carries offsets and an ASCII gutter, which do not survive a round trip. [`from_hex`] accepts
/// everything this produces, so a copy out of the hex grid and a paste back in is lossless.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Parse hex into bytes, tolerating what a human is likely to paste.
///
/// Whitespace anywhere is ignored, including newlines, so text copied out of a multi-line listing
/// works. A `0x` or `0X` prefix is accepted before any byte, since that is how hex appears in
/// source and in documentation. Case is immaterial.
///
/// Digits pair up left to right **after** whitespace is removed, so `a101` and `a1 01` and
/// `0xa1 0x01` are the same two bytes. An odd number of digits is an error rather than a leading
/// zero being assumed: the caller is about to overwrite a document, and guessing which end the
/// user dropped a digit from would corrupt it silently.
///
/// Errors carry the character position in the *original* text, counted in characters rather than
/// bytes so it matches what an editor would show.
pub fn from_hex(text: &str) -> Result<Vec<u8>> {
    let mut digits: Vec<(usize, u8)> = Vec::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    let mut position = 0usize;

    while let Some((_, ch)) = chars.next() {
        let here = position;
        position += 1;

        if ch.is_whitespace() {
            continue;
        }

        // A `0x` prefix: only when the `x` actually follows, so a bare `0` stays a digit.
        if ch == '0' && matches!(chars.peek(), Some((_, 'x' | 'X'))) {
            chars.next();
            position += 1;
            continue;
        }

        match ch.to_digit(16) {
            Some(value) => digits.push((here, value as u8)),
            None => {
                return Err(CoreError::NotHex {
                    position: here,
                    message: format!("`{ch}` is not a hex digit"),
                });
            }
        }
    }

    if digits.len() % 2 != 0 {
        let (position, _) = digits[digits.len() - 1];
        return Err(CoreError::NotHex {
            position,
            message: format!(
                "{} hex digits is an odd count, so the last byte is incomplete",
                digits.len()
            ),
        });
    }

    Ok(digits
        .chunks(2)
        .map(|pair| (pair[0].1 << 4) | pair[1].1)
        .collect())
}

/// Render `bytes` as a `hexdump -C`-style dump, 16 bytes per row.
pub fn hex_dump(bytes: &[u8]) -> String {
    hex_dump_with_width(bytes, DEFAULT_BYTES_PER_ROW)
}

/// Render `bytes` as a hex dump with `bytes_per_row` bytes per row.
///
/// Each row is `oooooooo  hh hh .. hh  hh .. hh  |ascii`, with an extra space every
/// [`GROUP`] bytes and unprintable bytes shown as `.` in the ASCII gutter. A short final row is
/// padded so the gutter stays aligned.
///
/// `bytes_per_row` of zero yields an empty string rather than looping forever.
pub fn hex_dump_with_width(bytes: &[u8], bytes_per_row: usize) -> String {
    if bytes_per_row == 0 {
        return String::new();
    }

    // Width of a full row's hex field: two characters per byte, one separator between bytes, plus
    // one extra separator per completed group boundary that is not the end of the row.
    let hex_width = bytes_per_row * 3 - 1 + (bytes_per_row - 1) / GROUP;

    let mut out = String::new();
    for (row, chunk) in bytes.chunks(bytes_per_row).enumerate() {
        let offset = row * bytes_per_row;
        out.push_str(&format!("{offset:08x}  "));

        let mut written = 0usize;
        for (index, byte) in chunk.iter().enumerate() {
            out.push_str(&format!("{byte:02x}"));
            written += 2;
            if index + 1 < chunk.len() {
                out.push(' ');
                written += 1;
                if index % GROUP == GROUP - 1 {
                    out.push(' ');
                    written += 1;
                }
            }
        }

        for _ in written..hex_width {
            out.push(' ');
        }
        out.push_str("  ");

        for &byte in chunk {
            out.push(if (32..127).contains(&byte) {
                byte as char
            } else {
                '.'
            });
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_edn() {
        // [1, "two", h'03']
        let cbor = [0x83, 0x01, 0x63, b't', b'w', b'o', 0x41, 0x03];
        let edn = cbor_to_edn(&cbor).expect("render EDN");
        assert!(edn.contains("\"two\""), "unexpected EDN: {edn}");
        assert_eq!(edn_to_cbor(&edn).expect("parse EDN"), cbor);
    }

    /// The regression test for the lost offset: the old backend's `format!("{:?}")` meant the
    /// frontend could not place a marker on an EDN syntax error.
    #[test]
    fn an_edn_syntax_error_keeps_its_offset() {
        let error = edn_to_cbor("[1, 2, %]").expect_err("should not parse");
        match error {
            CoreError::EdnParse {
                offset: Some(offset),
                ..
            } => assert_eq!(offset, 7),
            other => panic!("offset was lost: {other}"),
        }
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        // Two top-level items: 1 followed by 2.
        let error = cbor_to_edn(&[0x01, 0x02]).expect_err("should reject the tail");
        assert!(
            matches!(error, CoreError::CborDecode { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_truncated_item_reports_a_byte_offset() {
        // A 3-element array with only one element present.
        let error = check_well_formed(&[0x83, 0x01]).expect_err("should be truncated");
        match error {
            CoreError::CborDecode { offset, .. } => assert!(offset <= 2, "offset was {offset}"),
            other => panic!("unexpected error: {other}"),
        }
        assert!(is_truncation(&error_for(&[0x83, 0x01])));
    }

    #[test]
    fn a_well_formed_item_passes() {
        check_well_formed(&[0x83, 0x01, 0x02, 0x03]).expect("well formed");
    }

    fn error_for(bytes: &[u8]) -> CoreError {
        check_well_formed(bytes).expect_err("should fail")
    }

    #[test]
    fn hex_dump_matches_the_classic_layout() {
        let bytes: Vec<u8> = (0u8..20).collect();
        let dump = hex_dump(&bytes);
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "00000000  00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f  ................"
        );
        // The short row pads its hex field so the gutter stays in the same column.
        assert_eq!(
            lines[1],
            "00000010  10 11 12 13                                       ...."
        );
        let gutter = |line: &str| line.find("  ..").map(|i| i + 2);
        assert!(gutter(lines[0]).is_some());
        assert_eq!(lines[0].len(), lines[1].len() + 12);
    }

    #[test]
    fn printable_bytes_appear_in_the_gutter() {
        let dump = hex_dump(b"Hi\x00\x7f");
        assert!(dump.ends_with("  Hi..\n"), "unexpected dump: {dump:?}");
    }

    #[test]
    fn a_wider_row_keeps_its_groups_aligned() {
        let bytes: Vec<u8> = (0u8..32).collect();
        let dump = hex_dump_with_width(&bytes, 32);
        let line = dump.lines().next().expect("one row");
        // Three extra group separators at bytes 8, 16 and 24.
        assert_eq!(line.len(), 10 + (32 * 3 - 1 + 3) + 2 + 32);
        assert!(line.contains("07  08"), "missing group gap: {line}");
    }

    #[test]
    fn a_zero_width_dump_is_empty_rather_than_hanging() {
        assert_eq!(hex_dump_with_width(&[1, 2, 3], 0), "");
    }

    #[test]
    fn an_empty_input_dumps_to_nothing() {
        assert_eq!(hex_dump(&[]), "");
    }

    /// The point of "decode selection as CBOR": a `bstr` must yield its *contents*, not its own
    /// encoding. Returning `43 a1 01 26` where `a1 01 26` was meant would leave the user stripping
    /// the byte-string header by hand, which is the manual step the feature exists to remove.
    #[test]
    fn a_selected_bstr_decodes_to_its_payload_not_its_encoding() {
        // h'a10126' -- the protected header of a COSE_Sign1, a bstr wrapping {1: -7}.
        let bytes = decode_edn_selection("h'a10126'").expect("decode the selection");
        assert_eq!(bytes, [0xa1, 0x01, 0x26]);
        // The bstr's own encoding would have been four bytes with a 0x43 header.
        assert_eq!(
            edn_fragment_to_cbor("h'a10126'").expect("parse"),
            [0x43, 0xa1, 0x01, 0x26]
        );
    }

    /// The non-`bstr` fallback: selecting a map gives its encoding rather than an error, so the
    /// feature does not refuse a selection on the grounds of its CBOR major type.
    #[test]
    fn a_selected_map_decodes_to_its_own_encoding() {
        let bytes = decode_edn_selection("{1: 2}").expect("decode the selection");
        assert_eq!(bytes, [0xa1, 0x01, 0x02]);
    }

    /// A selection dragged out of an editor carries whitespace and newlines at its edges, and
    /// `parse_edn` rejects trailing input -- so without the trim this reported `TrailingInput`,
    /// an error about the selection's edges rather than about the user's EDN.
    #[test]
    fn a_fragment_parse_tolerates_surrounding_whitespace() {
        assert_eq!(
            decode_edn_selection("\n  h'0102'  \n").expect("decode"),
            [0x01, 0x02]
        );
    }

    #[test]
    fn a_selection_that_is_not_edn_is_an_error() {
        let error = decode_edn_selection("h'nothex'").expect_err("should not parse");
        assert!(
            matches!(error, CoreError::EdnParse { .. }),
            "unexpected error: {error}"
        );
    }

    /// The trim in [`edn_fragment_to_cbor`] leaves an empty string behind for a whitespace-only
    /// selection, and the parser is then asked to read a value out of nothing. It must come back
    /// with an error rather than indexing past the end.
    #[test]
    fn an_empty_selection_is_an_error_and_not_a_panic() {
        assert!(decode_edn_selection("").is_err());
        assert!(decode_edn_selection("  \n\t ").is_err());
    }

    /// Exactly **one** level of unwrapping. A `bstr` holding the encoding of another `bstr` must
    /// yield that inner encoding, not peel on to its contents -- an unbounded peel would show the
    /// user bytes they did not select, from a depth they never asked about.
    #[test]
    fn a_selected_bstr_is_unwrapped_exactly_one_level() {
        // h'43a10126' is a bstr whose four payload bytes are themselves `43 a1 01 26`, the
        // encoding of h'a10126'.
        let bytes = decode_edn_selection("h'43a10126'").expect("decode the selection");
        assert_eq!(bytes, [0x43, 0xa1, 0x01, 0x26]);
    }

    /// Only major type 2 is unwrapped. A *text* string shares the "string" idea and nothing else,
    /// so an arm matching it too would silently hand back the UTF-8 of `"abc"` where the user
    /// selected a CBOR text string.
    #[test]
    fn a_selected_text_string_is_not_mistaken_for_a_byte_string() {
        let bytes = decode_edn_selection("\"abc\"").expect("decode the selection");
        assert_eq!(bytes, [0x63, b'a', b'b', b'c']);
    }

    /// A tagged byte string keeps its tag. Tag 24 (`encoded-cbor`) is the obvious candidate for a
    /// second unwrap, and it is deliberately not taken: the tag is visible in the user's EDN, and
    /// stripping it would discard information they can see.
    #[test]
    fn a_tagged_byte_string_is_not_unwrapped() {
        let bytes = decode_edn_selection("24(h'a10126')").expect("decode the selection");
        assert_eq!(bytes, [0xd8, 0x18, 0x43, 0xa1, 0x01, 0x26]);
    }

    /// An empty document is not a parse failure. `parse_edn` calls `""` an `UnexpectedEof`, which
    /// would open every fresh window with a diagnostic against it and raise one again the instant
    /// the user selected all and deleted -- so the rule is pinned here, in the one place both
    /// frontends read it from.
    #[test]
    fn an_empty_edn_buffer_is_not_a_parse_failure() {
        assert!(check_edn("").is_ok());
        assert!(check_edn("  \n\t ").is_ok());
    }

    /// The byte-side half of the same rule. Before `check_cbor` existed the IDE called
    /// `check_well_formed` directly, so a window with no CBOR open reported "unexpected end of
    /// CBOR input" against a document that did not exist -- and showed it on the EDN tab too,
    /// which lists both diagnostic slots.
    #[test]
    fn an_empty_cbor_buffer_is_not_a_decode_failure() {
        assert!(check_cbor(&[]).is_ok());
    }

    /// The split is only worth having if the strict side stays strict: a validator asked about a
    /// zero-byte *file* must still refuse it. Asserted together so that collapsing the two
    /// functions into one breaks a test rather than quietly changing an answer.
    #[test]
    fn an_empty_buffer_is_still_malformed_to_the_strict_check() {
        assert!(check_well_formed(&[]).is_err());
    }

    /// The other side of the empty rule for bytes: a non-empty buffer is checked as before, and a
    /// truncated one still reports the offset the hex view marks.
    #[test]
    fn check_cbor_still_reports_a_real_decode_error() {
        assert!(check_cbor(&[0xa1, 0x01, 0x26]).is_ok());
        let error =
            check_cbor(&[0x59, 0x01]).expect_err("a truncated bstr header should not decode");
        assert!(
            is_truncation(&error),
            "a half-written length should read as truncation, not corruption: {error}"
        );
    }

    /// The other side of the empty rule: real input is still checked, and the encoding is
    /// discarded rather than returned, so a caller cannot come to depend on it.
    #[test]
    fn check_edn_still_reports_a_real_parse_error() {
        assert!(check_edn("{1: 2}").is_ok());
        let error = check_edn("[1, 2, %]").expect_err("should not parse");
        assert!(
            matches!(
                error,
                CoreError::EdnParse {
                    offset: Some(7),
                    ..
                }
            ),
            "unexpected error: {error}"
        );
    }

    /// An unclosed array is the editor's normal mid-typing state, so it must be distinguishable
    /// from input that is actually wrong. [`is_truncation`] matches only `CborDecode` and so is
    /// no help here, which is why [`is_edn_truncation`] exists at all.
    #[test]
    fn an_unterminated_edn_value_is_a_truncation_and_not_a_syntax_error() {
        let error = edn_to_cbor("[1, 2").expect_err("should not parse");
        assert!(is_edn_truncation(&error), "unexpected error: {error}");
        assert!(
            !is_truncation(&error),
            "the CBOR predicate must not claim an EDN error"
        );

        let wrong = edn_to_cbor("[1, 2, %]").expect_err("should not parse");
        assert!(!is_edn_truncation(&wrong), "a bad character is not an EOF");
    }

    // ─── to_hex / from_hex ────────────────────────────────────────────────────

    /// The property the hex grid's Copy and Paste rest on.
    #[test]
    fn to_hex_and_from_hex_round_trip() {
        for case in [
            &[][..],
            &[0x00],
            &[0xff],
            &[0xa1, 0x01, 0x26],
            &(0u8..=255).collect::<Vec<u8>>()[..],
        ] {
            let text = to_hex(case);
            assert_eq!(
                from_hex(&text).expect("to_hex output parses"),
                case,
                "round trip failed for {text}"
            );
        }
        assert_eq!(to_hex(&[0xa1, 0x01, 0x26]), "a1 01 26");
    }

    /// Hex a human assembled, rather than hex this module produced.
    #[test]
    fn from_hex_accepts_what_a_human_pastes() {
        let expected = vec![0xa1u8, 0x01, 0x26];
        for text in [
            "a10126",
            "a1 01 26",
            "A1 01 26",
            "0xa1 0x01 0x26",
            "a1\n01\t26\r\n",
            "  a1 0126  ",
            "0xA10126",
        ] {
            assert_eq!(
                from_hex(text).expect("parses"),
                expected,
                "failed on {text:?}"
            );
        }
        assert_eq!(from_hex("").expect("empty parses"), Vec::<u8>::new());
        // A bare `0` is a digit, not a truncated prefix.
        assert_eq!(from_hex("00").expect("parses"), vec![0x00]);
    }

    /// An odd digit count is refused rather than zero-extended: the caller is about to overwrite a
    /// document, and either guess could corrupt it.
    #[test]
    fn from_hex_refuses_an_odd_digit_count_and_says_where() {
        match from_hex("a1 01 2") {
            Err(CoreError::NotHex { position, message }) => {
                assert_eq!(position, 6, "the position of the last digit");
                assert!(message.contains("odd"), "unhelpful message: {message}");
            }
            other => panic!("expected NotHex, got {other:?}"),
        }
    }

    #[test]
    fn from_hex_names_the_offending_character_and_its_position() {
        match from_hex("a1 0g 26") {
            Err(CoreError::NotHex { position, message }) => {
                assert_eq!(position, 4);
                assert!(message.contains('g'), "unhelpful message: {message}");
            }
            other => panic!("expected NotHex, got {other:?}"),
        }
    }

    /// The position counts characters, not bytes, so it matches what an editor would show.
    #[test]
    fn from_hex_counts_the_position_in_characters() {
        match from_hex("é g") {
            Err(CoreError::NotHex { position, .. }) => assert_eq!(position, 0, "é is not hex"),
            other => panic!("expected NotHex, got {other:?}"),
        }
        match from_hex("a1 é") {
            Err(CoreError::NotHex { position, .. }) => assert_eq!(position, 3),
            other => panic!("expected NotHex, got {other:?}"),
        }
    }

    /// A byte string whose contents are not well-formed CBOR still decodes, by design: the
    /// caller's own well-formedness check reports it, so this must not pre-empt it with a second
    /// error in different words.
    #[test]
    fn a_bstr_payload_that_is_not_well_formed_cbor_still_decodes() {
        let bytes = decode_edn_selection("h'ff'").expect("unwrap without judging the contents");
        assert_eq!(bytes, [0xff]);
        assert!(
            check_well_formed(&bytes).is_err(),
            "0xff is not well formed"
        );
    }
}
