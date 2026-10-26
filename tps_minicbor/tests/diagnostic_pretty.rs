/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![cfg(feature = "diagnostic")]

mod support;

use support::hex_to_bytes;
use tps_minicbor::Decoder;

fn pretty(hex: &str) -> String {
    let bytes = hex_to_bytes(hex);
    let dec = Decoder::new(&bytes);
    let mut out = String::new();
    dec.write_diagnostic_pretty_all_fmt(&mut out).unwrap();
    out
}

#[test]
fn scalar_is_unaffected_by_pretty_printing() {
    assert_eq!(pretty("0x00"), "0");
    assert_eq!(pretty("0x6161"), "\"a\"");
}

#[test]
fn empty_array_and_map_stay_on_one_line() {
    assert_eq!(pretty("0x80"), "[]");
    assert_eq!(pretty("0xa0"), "{}");
}

#[test]
fn flat_array_is_indented_one_item_per_line() {
    // [1, 2, 3]
    assert_eq!(pretty("0x83010203"), "[\n  1,\n  2,\n  3\n]");
}

#[test]
fn flat_map_is_indented_one_pair_per_line() {
    // {1: 2, 3: 4}
    assert_eq!(pretty("0xa201020304"), "{\n  1: 2,\n  3: 4\n}");
}

#[test]
fn nested_structure_indents_by_depth() {
    // [1, [2, 3]]
    assert_eq!(pretty("0x8201820203"), "[\n  1,\n  [\n    2,\n    3\n  ]\n]");
}

#[test]
fn map_with_nested_array_value_indents_correctly() {
    // {1: [2, 3]}
    assert_eq!(pretty("0xa1018202 03"), "{\n  1: [\n    2,\n    3\n  ]\n}");
}

#[test]
fn pretty_output_round_trips_through_edn_parser() {
    #[cfg(feature = "edn")]
    {
        let bytes = hex_to_bytes("0x8201820203");
        let dec = Decoder::new(&bytes);
        let mut out = String::new();
        dec.write_diagnostic_pretty_all_fmt(&mut out).unwrap();
        let reparsed = tps_minicbor::parse_edn(&out).unwrap();
        assert_eq!(reparsed, bytes);
    }
}

#[test]
fn pretty_slice_and_io_variants_match_fmt_variant() {
    let bytes = hex_to_bytes("0x8201820203");
    let dec = Decoder::new(&bytes);

    let mut expected = String::new();
    dec.write_diagnostic_pretty_all_fmt(&mut expected).unwrap();

    let mut buf = vec![0u8; expected.len() + 16];
    let len = dec.write_diagnostic_pretty_all(&mut buf).unwrap();
    assert_eq!(core::str::from_utf8(&buf[..len]).unwrap(), expected);

    let mut io_buf: Vec<u8> = Vec::new();
    dec.write_diagnostic_pretty_all_io(&mut io_buf).unwrap();
    assert_eq!(core::str::from_utf8(&io_buf).unwrap(), expected);
}

#[test]
fn pretty_all_rejects_trailing_bytes() {
    // A single uint (0x00) followed by a trailing byte.
    let bytes = hex_to_bytes("0x0000");
    let dec = Decoder::new(&bytes);
    let mut out = String::new();
    assert!(dec.write_diagnostic_pretty_all_fmt(&mut out).is_err());
}
