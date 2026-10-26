/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![cfg(all(feature = "diagnostic", feature = "spans"))]

mod support;

use support::hex_to_bytes;
use tps_minicbor::Decoder;

#[test]
fn output_is_byte_identical_to_unspanned_method() {
    let bytes = hex_to_bytes("0xa1018202 03");
    let dec = Decoder::new(&bytes);

    let mut expected = String::new();
    dec.write_diagnostic_pretty_all_fmt(&mut expected).unwrap();

    let mut actual = String::new();
    let _spans = dec
        .write_diagnostic_pretty_all_fmt_with_spans(&mut actual, Some(0))
        .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn scalar_span_covers_whole_item() {
    // uint(42) => 0x18 0x2a, EDN text "42"
    let bytes = hex_to_bytes("0x182a");
    let dec = Decoder::new(&bytes);
    let mut out = String::new();
    let spans = dec
        .write_diagnostic_pretty_all_fmt_with_spans(&mut out, Some(0))
        .unwrap();

    assert_eq!(out, "42");
    assert_eq!(spans, vec![(0..2, 0..2)]);
}

#[test]
fn flat_array_spans_map_each_element_to_its_own_edn_text() {
    // [1, 2, 3] pretty-printed as "[\n  1,\n  2,\n  3\n]"
    let bytes = hex_to_bytes("0x83010203");
    let dec = Decoder::new(&bytes);
    let mut out = String::new();
    let spans = dec
        .write_diagnostic_pretty_all_fmt_with_spans(&mut out, Some(0))
        .unwrap();

    assert_eq!(out, "[\n  1,\n  2,\n  3\n]");

    // Element spans come before the whole-array span (post-order push).
    let elem_spans: Vec<_> = spans[..3].to_vec();
    for (cbor, edn) in &elem_spans {
        assert_eq!(cbor.end - cbor.start, 1);
        assert_eq!(&out[edn.clone()], match cbor.start {
            1 => "1",
            2 => "2",
            3 => "3",
            _ => panic!("unexpected cbor offset"),
        });
    }

    // The whole array is the last span recorded, covering the full text.
    let (whole_cbor, whole_edn) = spans.last().unwrap();
    assert_eq!(*whole_cbor, 0..4);
    assert_eq!(&out[whole_edn.clone()], out.as_str());
}

#[test]
fn nested_structure_spans_compose_with_edn_text() {
    // {1: [2, 3]} pretty-printed as "{\n  1: [\n    2,\n    3\n  ]\n}"
    let bytes = hex_to_bytes("0xa1018202 03");
    let dec = Decoder::new(&bytes);
    let mut out = String::new();
    let spans = dec
        .write_diagnostic_pretty_all_fmt_with_spans(&mut out, Some(0))
        .unwrap();

    assert_eq!(out, "{\n  1: [\n    2,\n    3\n  ]\n}");

    // Every recorded EDN span, when sliced out of the text, must round-trip
    // to a valid, self-consistent chunk of the printed EDN.
    for (_, edn) in &spans {
        assert!(edn.end <= out.len());
        assert!(out.is_char_boundary(edn.start));
        assert!(out.is_char_boundary(edn.end));
    }

    // The nested array [2, 3] should map to its own EDN slice "[\n    2,\n    3\n  ]".
    let nested_array = spans
        .iter()
        .find(|(cbor, _)| *cbor == (2..5))
        .expect("nested array span not found");
    assert_eq!(&out[nested_array.1.clone()], "[\n    2,\n    3\n  ]");
}
