/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Tests for the EDN (CBOR diagnostic notation) parser — RFC 8949 §8.
//!
//! Values are cross-checked against the RFC 8949 Appendix A test vectors
//! where applicable, and against round-trips through `Decoder::write_diagnostic_all_fmt`.

#[cfg(feature = "edn")]
mod edn_tests {
    #[cfg(feature = "diagnostic")]
    use tps_minicbor::Decoder;
    use tps_minicbor::{EdnError, parse_edn};

    // ── Unsigned integers ─────────────────────────────────────────────────────

    #[test]
    fn uint_zero() {
        assert_eq!(parse_edn("0").unwrap(), vec![0x00]);
    }

    #[test]
    fn uint_small() {
        assert_eq!(parse_edn("1").unwrap(), vec![0x01]);
        assert_eq!(parse_edn("23").unwrap(), vec![0x17]);
    }

    #[test]
    fn uint_one_byte() {
        assert_eq!(parse_edn("24").unwrap(), vec![0x18, 0x18]);
        assert_eq!(parse_edn("255").unwrap(), vec![0x18, 0xff]);
    }

    #[test]
    fn uint_two_byte() {
        assert_eq!(parse_edn("256").unwrap(), vec![0x19, 0x01, 0x00]);
        assert_eq!(parse_edn("1000").unwrap(), vec![0x19, 0x03, 0xe8]);
    }

    #[test]
    fn uint_four_byte() {
        assert_eq!(
            parse_edn("1000000").unwrap(),
            vec![0x1a, 0x00, 0x0f, 0x42, 0x40]
        );
    }

    #[test]
    fn uint_eight_byte() {
        // 10000000000000000000 = 0x8AC7230489E80000
        assert_eq!(
            parse_edn("18446744073709551615").unwrap(),
            vec![0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
    }

    #[test]
    fn uint_hex_prefix() {
        // 0x1a = 26 decimal → preferred 2-byte encoding [0x18, 0x1a] = [24, 26]
        assert_eq!(parse_edn("0x1a").unwrap(), vec![0x18, 0x1a]);
        // 0xff = 255
        assert_eq!(parse_edn("0xFF").unwrap(), vec![0x18, 0xff]);
    }

    #[test]
    fn uint_binary_prefix() {
        assert_eq!(parse_edn("0b1010").unwrap(), vec![0x0a]);
    }

    #[test]
    fn uint_octal_prefix() {
        assert_eq!(parse_edn("0o17").unwrap(), vec![0x0f]);
    }

    // ── Negative integers ─────────────────────────────────────────────────────

    #[test]
    fn nint_small() {
        assert_eq!(parse_edn("-1").unwrap(), vec![0x20]);
        assert_eq!(parse_edn("-10").unwrap(), vec![0x29]);
        assert_eq!(parse_edn("-100").unwrap(), vec![0x38, 0x63]);
    }

    #[test]
    fn nint_large() {
        assert_eq!(parse_edn("-1000").unwrap(), vec![0x39, 0x03, 0xe7]);
    }

    #[test]
    fn nint_zero_is_zero() {
        // -0 in EDN equals 0
        assert_eq!(parse_edn("-0").unwrap(), vec![0x00]);
    }

    // ── Floats ────────────────────────────────────────────────────────────────

    #[test]
    #[cfg(feature = "float")]
    fn float_zero() {
        // 0.0 → half precision 0xf90000
        assert_eq!(parse_edn("0.0").unwrap(), vec![0xf9, 0x00, 0x00]);
    }

    #[test]
    #[cfg(feature = "float")]
    fn float_one() {
        // 1.0 → half precision 0xf93c00
        assert_eq!(parse_edn("1.0").unwrap(), vec![0xf9, 0x3c, 0x00]);
    }

    #[test]
    #[cfg(feature = "float")]
    fn float_one_point_five() {
        // 1.5 → half precision 0xf93e00
        assert_eq!(parse_edn("1.5").unwrap(), vec![0xf9, 0x3e, 0x00]);
    }

    #[test]
    #[cfg(feature = "float")]
    fn float_large() {
        // 100000.0 → single precision 0xfa47c35000
        assert_eq!(
            parse_edn("100000.0").unwrap(),
            vec![0xfa, 0x47, 0xc3, 0x50, 0x00]
        );
    }

    #[test]
    #[cfg(feature = "float")]
    fn float_nan() {
        assert_eq!(parse_edn("NaN").unwrap(), vec![0xf9, 0x7e, 0x00]);
    }

    #[test]
    #[cfg(feature = "float")]
    fn float_inf_pos() {
        assert_eq!(parse_edn("Infinity").unwrap(), vec![0xf9, 0x7c, 0x00]);
    }

    #[test]
    #[cfg(feature = "float")]
    fn float_inf_neg() {
        assert_eq!(parse_edn("-Infinity").unwrap(), vec![0xf9, 0xfc, 0x00]);
    }

    // ── Text strings ──────────────────────────────────────────────────────────

    #[test]
    fn tstr_empty() {
        assert_eq!(parse_edn(r#""""#).unwrap(), vec![0x60]);
    }

    #[test]
    fn tstr_ascii() {
        // "a" = 0x61 0x61
        assert_eq!(parse_edn(r#""a""#).unwrap(), vec![0x61, 0x61]);
    }

    #[test]
    fn tstr_longer() {
        // "IETF" = 0x64 0x49454446
        assert_eq!(
            parse_edn(r#""IETF""#).unwrap(),
            vec![0x64, 0x49, 0x45, 0x54, 0x46]
        );
    }

    #[test]
    fn tstr_escape_quote() {
        // "\"" → a 1-char string containing a double-quote: 0x61 0x22
        assert_eq!(parse_edn(r#""\"""#).unwrap(), vec![0x61, 0x22]);
    }

    #[test]
    fn tstr_unicode_escape() {
        // "ü" = 'ü' (U+00FC)
        let result = parse_edn(r#""ü""#).unwrap();
        // "ü" in UTF-8 is 0xc3 0xbc; tstr header 0x62 (2-byte string)
        assert_eq!(result, vec![0x62, 0xc3, 0xbc]);
    }

    // ── Byte strings ──────────────────────────────────────────────────────────

    #[test]
    fn bstr_empty() {
        assert_eq!(parse_edn("h''").unwrap(), vec![0x40]);
    }

    #[test]
    fn bstr_hex() {
        // h'01020304' = 0x44 01 02 03 04
        assert_eq!(
            parse_edn("h'01020304'").unwrap(),
            vec![0x44, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn bstr_hex_whitespace_ignored() {
        assert_eq!(
            parse_edn("h'01 02 03 04'").unwrap(),
            vec![0x44, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn bstr_base64() {
        // b64'aGVsbG8=' = "hello"
        assert_eq!(
            parse_edn("b64'aGVsbG8='").unwrap(),
            vec![0x45, b'h', b'e', b'l', b'l', b'o']
        );
    }

    #[test]
    fn bstr_raw() {
        // 'hello'
        assert_eq!(
            parse_edn("'hello'").unwrap(),
            vec![0x45, b'h', b'e', b'l', b'l', b'o']
        );
    }

    // ── Simple values ─────────────────────────────────────────────────────────

    #[test]
    fn simple_false() {
        assert_eq!(parse_edn("false").unwrap(), vec![0xf4]);
    }

    #[test]
    fn simple_true() {
        assert_eq!(parse_edn("true").unwrap(), vec![0xf5]);
    }

    #[test]
    fn simple_null() {
        assert_eq!(parse_edn("null").unwrap(), vec![0xf6]);
    }

    #[test]
    fn simple_undefined() {
        assert_eq!(parse_edn("undefined").unwrap(), vec![0xf7]);
    }

    #[test]
    fn simple_value_16() {
        // simple(16) = 0xf0
        assert_eq!(parse_edn("simple(16)").unwrap(), vec![0xf0]);
    }

    #[test]
    fn simple_value_255() {
        // simple(255) = 0xf8 0xff
        assert_eq!(parse_edn("simple(255)").unwrap(), vec![0xf8, 0xff]);
    }

    // ── Arrays ────────────────────────────────────────────────────────────────

    #[test]
    fn array_empty() {
        assert_eq!(parse_edn("[]").unwrap(), vec![0x80]);
    }

    #[test]
    fn array_one_two_three() {
        assert_eq!(
            parse_edn("[1, 2, 3]").unwrap(),
            vec![0x83, 0x01, 0x02, 0x03]
        );
    }

    #[test]
    fn array_nested() {
        // [[1], [2, 3], [4, 5]]
        assert_eq!(
            parse_edn("[[1], [2, 3], [4, 5]]").unwrap(),
            vec![0x83, 0x81, 0x01, 0x82, 0x02, 0x03, 0x82, 0x04, 0x05]
        );
    }

    #[test]
    fn array_mixed() {
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]
        let result = parse_edn("[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]").unwrap();
        assert_eq!(result[0], 0x98); // array, 1-byte length follows
        assert_eq!(result[1], 25); // 25 items
    }

    // ── Maps ─────────────────────────────────────────────────────────────────

    #[test]
    fn map_empty() {
        assert_eq!(parse_edn("{}").unwrap(), vec![0xa0]);
    }

    #[test]
    fn map_one_entry() {
        assert_eq!(parse_edn("{1: 2}").unwrap(), vec![0xa1, 0x01, 0x02]);
    }

    #[test]
    fn map_string_keys() {
        // {"a": 1, "b": [2, 3]} = 0xa2 0x61 0x61 0x01 0x61 0x62 0x82 0x02 0x03
        assert_eq!(
            parse_edn(r#"{"a": 1, "b": [2, 3]}"#).unwrap(),
            vec![0xa2, 0x61, b'a', 0x01, 0x61, b'b', 0x82, 0x02, 0x03]
        );
    }

    #[test]
    fn map_bstr_hex_key() {
        // {h'0102': 1} = 0xa1 0x42 0x01 0x02 0x01
        assert_eq!(
            parse_edn("{h'0102': 1}").unwrap(),
            vec![0xa1, 0x42, 0x01, 0x02, 0x01]
        );
    }

    #[test]
    fn map_bstr_raw_key() {
        // {'ab': 1} = 0xa1 0x42 0x61 0x62 0x01
        assert_eq!(
            parse_edn("{'ab': 1}").unwrap(),
            vec![0xa1, 0x42, b'a', b'b', 0x01]
        );
    }

    #[test]
    fn map_bstr_base64_key() {
        // {b64'aGk=': 1} — b64'aGk=' decodes to "hi" (2 bytes)
        // = 0xa1 0x42 0x68 0x69 0x01
        assert_eq!(
            parse_edn("{b64'aGk=': 1}").unwrap(),
            vec![0xa1, 0x42, b'h', b'i', 0x01]
        );
    }

    #[test]
    fn map_mixed_bstr_and_other_key_types() {
        // {1: "int", "s": "text", h'ff': "bstr"}
        let result = parse_edn(r#"{1: "int", "s": "text", h'ff': "bstr"}"#).unwrap();
        assert_eq!(result[0], 0xa3); // map(3)
        // Keys appear in source order (parse_map does not sort).
        assert_eq!(&result[1..2], &[0x01]); // key: uint 1
        assert_eq!(&result[6..8], &[0x61, b's']); // key: tstr "s"
        assert_eq!(&result[13..15], &[0x41, 0xff]); // key: bstr h'ff'
    }

    // ── Tags ─────────────────────────────────────────────────────────────────

    #[test]
    fn tag_datetime() {
        // 0("2013-03-21T20:04:00Z")
        let result = parse_edn(r#"0("2013-03-21T20:04:00Z")"#).unwrap();
        assert_eq!(result[0], 0xc0); // tag 0
        // Rest should be tstr
        assert_eq!(result[1], 0x74); // tstr of length 20
    }

    #[test]
    fn tag_epoch_time() {
        // 1(1363896240)
        assert_eq!(
            parse_edn("1(1363896240)").unwrap(),
            vec![0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0]
        );
    }

    #[test]
    fn tag_24_encoded_cbor() {
        // 24(h'6449455446') — tag 24 wrapping a bstr
        let result = parse_edn("24(h'6449455446')").unwrap();
        assert_eq!(result[0], 0xd8); // tag, 1-byte value follows
        assert_eq!(result[1], 24);
        assert_eq!(result[2], 0x45); // 5-byte bstr
    }

    // ── Comments (RFC 8610 Appendix G.6: delimited by a pair of "/") ───────────

    #[test]
    fn comment_ignored() {
        assert_eq!(
            parse_edn("/ this is a comment / 42").unwrap(),
            vec![0x18, 0x2a]
        );
    }

    #[test]
    fn comment_leading_no_space() {
        assert_eq!(parse_edn("/comment/42").unwrap(), vec![0x18, 0x2a]);
    }

    #[test]
    fn comment_trailing() {
        assert_eq!(parse_edn("42 /trailing comment/").unwrap(), vec![0x18, 0x2a]);
    }

    #[test]
    fn inline_comment() {
        assert_eq!(
            parse_edn("[1, /first item/ 2]").unwrap(),
            vec![0x82, 0x01, 0x02]
        );
    }

    #[test]
    fn comment_can_span_newlines() {
        assert_eq!(
            parse_edn("/ a comment\nspanning lines / 42").unwrap(),
            vec![0x18, 0x2a]
        );
    }

    #[test]
    fn comment_inside_hex_bstr() {
        // Example from RFC 8610 Appendix G.6.
        assert_eq!(
            parse_edn("h'68 65 6c /doubled l!/ 6c 6f /hello/\n  20 /space/\n  77 6f 72 6c 64' /world/").unwrap(),
            parse_edn("h'68656c6c6f20776f726c64'").unwrap()
        );
    }

    #[test]
    fn comment_annotated_array() {
        // Example from RFC 8610 Appendix G.6.
        assert_eq!(
            parse_edn(
                "/grasp-message/ [/M_DISCOVERY/ 1, /session-id/ 10584416, \
                 /objective/ [/objective-name/ \"opsonize\", /D, N, S/ 7, /loop-count/ 105]]"
            )
            .unwrap(),
            parse_edn(r#"[1, 10584416, ["opsonize", 7, 105]]"#).unwrap()
        );
    }

    #[test]
    fn error_unterminated_comment() {
        assert_eq!(parse_edn("/unterminated 42"), Err(EdnError::UnexpectedEof));
    }

    // ── Negative floats ───────────────────────────────────────────────────────

    #[test]
    #[cfg(feature = "float")]
    fn float_neg_one_point_five() {
        // -1.5 → half precision 0xf9be00
        assert_eq!(parse_edn("-1.5").unwrap(), vec![0xf9, 0xbe, 0x00]);
    }

    #[test]
    #[cfg(feature = "float")]
    fn float_neg_zero_point_five() {
        // -0.5 → half precision
        let result = parse_edn("-0.5").unwrap();
        assert_eq!(result[0], 0xf9); // half-precision
        let bits = u16::from_be_bytes([result[1], result[2]]);
        // Sign bit set, value is -0.5
        assert_ne!(bits & 0x8000, 0);
    }

    // ── Negative hex integers ─────────────────────────────────────────────────

    #[test]
    fn nint_hex() {
        // -0x1f = -31 → CBOR nint(30) = 0x38 0x1e
        assert_eq!(parse_edn("-0x1f").unwrap(), vec![0x38, 0x1e]);
    }

    // ── Tags with value > 23 ──────────────────────────────────────────────────

    #[test]
    fn tag_value_32() {
        // 32("http://example.com") — tag 32 = URI
        let result = parse_edn(r#"32("http://example.com")"#).unwrap();
        assert_eq!(result[0], 0xd8); // tag, 1-byte value follows
        assert_eq!(result[1], 32);
    }

    // ── simple(24) boundary ───────────────────────────────────────────────────

    #[test]
    fn simple_value_24_is_reserved() {
        // simple(24) = 0xf8 0x18 is a non-well-formed two-byte encoding per
        // RFC 8949 §3.3/Appendix F Subkind 2; rejected since the Stage 1b fix.
        assert_eq!(
            parse_edn("simple(24)"),
            Err(EdnError::ReservedSimpleValue(24))
        );
    }

    // ── RFC 8949 §3.3 / Appendix F: `simple(N)` for N in the reserved 24..=31
    // range is rejected as non-well-formed CBOR ─────────────────────────────────
    //
    // RFC 8949 Appendix F explicitly lists `f8 18` and `f8 1f` among its
    // examples of syntactically non-well-formed CBOR (subkind 2: reserved
    // two-byte simple-value encodings), because §3.3 states "An encoder MUST
    // NOT issue two-byte sequences that start with 0xf8 ... and continue with
    // a byte less than 0x20". `parse_simple_call` rejects N in 20..=31
    // (mirroring `Simple::new`'s validation) with `EdnError::ReservedSimpleValue`.
    #[test]
    fn simple_value_20_to_31_is_rejected() {
        for n in 20u8..=31 {
            assert_eq!(
                parse_edn(&std::format!("simple({n})")),
                Err(EdnError::ReservedSimpleValue(n)),
                "simple({n}) should be rejected as non-well-formed"
            );
        }
    }

    // ── Error cases ───────────────────────────────────────────────────────────

    #[test]
    fn error_trailing_input() {
        assert_eq!(parse_edn("1 2"), Err(EdnError::TrailingInput));
    }

    #[test]
    fn error_unexpected_eof() {
        assert_eq!(parse_edn("[1, 2"), Err(EdnError::UnexpectedEof));
    }

    #[test]
    fn error_invalid_hex() {
        assert!(matches!(parse_edn("h'gg'"), Err(EdnError::InvalidHex)));
    }

    #[test]
    fn error_invalid_base64() {
        assert!(matches!(parse_edn("b64'!!'"), Err(EdnError::InvalidBase64)));
    }

    // ── Round-trips via Decoder::write_diagnostic ─────────────────────────────

    #[cfg(feature = "diagnostic")]
    fn round_trip(edn: &str) -> String {
        let cbor = parse_edn(edn).unwrap();
        let dec = Decoder::new(&cbor);
        let mut s = String::new();
        dec.write_diagnostic_all_fmt(&mut s).unwrap();
        s
    }

    #[test]
    #[cfg(feature = "diagnostic")]
    fn round_trip_integer() {
        assert_eq!(round_trip("42"), "42");
    }

    #[test]
    #[cfg(feature = "diagnostic")]
    fn round_trip_negative() {
        assert_eq!(round_trip("-1"), "-1");
    }

    #[test]
    #[cfg(feature = "diagnostic")]
    fn round_trip_array() {
        // diagnostic formatter writes [1,2,3] (no spaces)
        assert_eq!(round_trip("[1, 2, 3]"), "[1,2,3]");
    }

    #[test]
    #[cfg(feature = "diagnostic")]
    fn round_trip_map() {
        // diagnostic formatter writes {1:2} (no spaces)
        assert_eq!(round_trip("{1: 2}"), "{1:2}");
    }

    #[test]
    #[cfg(all(feature = "diagnostic", feature = "float"))]
    fn round_trip_float() {
        assert_eq!(round_trip("1.5"), "1.5");
    }

    #[test]
    #[cfg(feature = "diagnostic")]
    fn round_trip_tstr() {
        assert_eq!(round_trip(r#""hello""#), r#""hello""#);
    }

    #[test]
    #[cfg(feature = "diagnostic")]
    fn round_trip_bstr_hex() {
        // h'aabb' round-trips to h'aabb'
        assert_eq!(round_trip("h'aabb'"), "h'aabb'");
    }

    #[test]
    #[cfg(feature = "diagnostic")]
    fn round_trip_true_false_null() {
        assert_eq!(round_trip("true"), "true");
        assert_eq!(round_trip("false"), "false");
        assert_eq!(round_trip("null"), "null");
    }
}
