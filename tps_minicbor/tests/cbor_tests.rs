/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
mod support;

use support::{Sample, decode_all};
use tps_minicbor::{
    Decode, DecodeError, Decoder, EncodeError, EncodeItem, Encoder, Item, Sizer, encode,
};

#[test]
fn encode_decode_u64() {
    let value = 42u64;
    let mut buf = [0u8; 32];
    let len = encode(&value, &mut buf).expect("encode");
    let decoded = decode_all::<u64>(&buf[..len]).expect("decode");
    assert_eq!(decoded, value);
}

#[test]
fn encode_decode_negative_i64() {
    let value = -24i64;
    let mut buf = [0u8; 32];
    let len = encode(&value, &mut buf).expect("encode");
    let decoded = decode_all::<i64>(&buf[..len]).expect("decode");
    assert_eq!(decoded, value);
}

#[test]
fn encode_decode_i64_min() {
    let mut buf = [0u8; 16];
    let len = encode(&i64::MIN, &mut buf).expect("encode i64::MIN");
    let decoded = decode_all::<i64>(&buf[..len]).expect("decode i64::MIN");
    assert_eq!(decoded, i64::MIN);
}

#[test]
fn encode_decode_text_and_bytes() {
    let text = "cbor";
    let data: &[u8] = b"\x01\x02\x03";
    let mut buf = [0u8; 64];
    let mut s = Sizer::new();
    s.array_len(2).expect("array len");
    text.size(&mut s).expect("text size");
    data.size(&mut s).expect("bytes size");
    let total = s.len();
    let mut e = Encoder::<4>::new(&mut buf);
    let mut array = e.array_with_len(2).expect("array");
    array.insert_ref(text).expect("text");
    array.insert_ref(data).expect("bytes");
    array.end().expect("end array");
    assert_eq!(e.len(), total);

    let encoded_len = e.len();
    let mut d = Decoder::new(&buf[..encoded_len]);
    let len = d.array_len().expect("array len");
    assert_eq!(len, 2);
    let decoded_text = <&str>::decode(&mut d).expect("text");
    let decoded_data = <&[u8]>::decode(&mut d).expect("bytes");
    assert_eq!(decoded_text, text);
    assert_eq!(decoded_data, data);
    assert_eq!(d.pos(), encoded_len);
}

#[test]
fn encode_decode_sample() {
    let sample = Sample {
        id: 7,
        name: "node",
        data: b"\x10\x20",
    };
    let mut buf = [0u8; 64];
    let len = encode(&sample, &mut buf).expect("encode");
    let decoded = decode_all::<Sample>(&buf[..len]).expect("decode");
    assert_eq!(decoded.id, sample.id);
    assert_eq!(decoded.name, sample.name);
    assert_eq!(decoded.data, sample.data);
}

#[test]
fn decode_rejects_trailing_bytes() {
    let value = 1u64;
    let mut buf = [0u8; 16];
    let len = encode(&value, &mut buf).expect("encode");
    buf[len] = 0xff;
    let res = decode_all::<u64>(&buf[..len + 1]);
    assert!(res.is_err());
}

#[test]
fn encode_u64_boundaries() {
    fn assert_enc(value: u64, expected: &[u8]) {
        let mut buf = [0u8; 16];
        let len = encode(&value, &mut buf).expect("encode");
        assert_eq!(&buf[..len], expected);
    }

    assert_enc(23, &[0x17]);
    assert_enc(24, &[0x18, 0x18]);
    assert_enc(255, &[0x18, 0xff]);
    assert_enc(256, &[0x19, 0x01, 0x00]);
    assert_enc(65_535, &[0x19, 0xff, 0xff]);
    assert_enc(65_536, &[0x1a, 0x00, 0x01, 0x00, 0x00]);
    assert_enc(4_294_967_295, &[0x1a, 0xff, 0xff, 0xff, 0xff]);
    assert_enc(
        4_294_967_296,
        &[0x1b, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
    );
}

#[test]
fn encode_i64_negative_boundaries() {
    fn assert_enc(value: i64, expected: &[u8]) {
        let mut buf = [0u8; 16];
        let len = encode(&value, &mut buf).expect("encode");
        assert_eq!(&buf[..len], expected);
    }

    assert_enc(-1, &[0x20]);
    assert_enc(-24, &[0x37]);
    assert_enc(-25, &[0x38, 0x18]);
    assert_enc(-256, &[0x38, 0xff]);
    assert_enc(-257, &[0x39, 0x01, 0x00]);
    assert_enc(-65_536, &[0x39, 0xff, 0xff]);
    assert_enc(-65_537, &[0x3a, 0x00, 0x01, 0x00, 0x00]);
    // i64::MIN = -9223372036854775808; CBOR encodes as 0x3b + 8 bytes of 0x7fffffffffffffff
    assert_enc(
        i64::MIN,
        &[0x3b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
    );
}

#[test]
fn encode_length_headers_for_text_and_bytes() {
    fn assert_enc<T: EncodeItem>(value: T, expected: &[u8]) {
        let mut buf = [0u8; 80];
        let len = encode(&value, &mut buf).expect("encode");
        assert_eq!(&buf[..len], expected);
    }

    let short_bytes = [0x01, 0x02, 0x03];
    assert_enc(&short_bytes[..], &[0x43, 0x01, 0x02, 0x03]);
    let long_bytes = [0u8; 24];
    let mut expected = [0u8; 26];
    expected[0] = 0x58;
    expected[1] = 24;
    assert_enc(&long_bytes[..], &expected);

    assert_enc("hi", &[0x62, b'h', b'i']);
    let long_text = "abcdefghijklmnopqrstuvwx";
    let mut expected_text = [0u8; 26];
    expected_text[0] = 0x78;
    expected_text[1] = 24;
    expected_text[2..].copy_from_slice(long_text.as_bytes());
    assert_enc(long_text, &expected_text);
}

#[test]
fn decode_u64_header_widths() {
    let inputs: &[(&[u8], u64)] = &[
        (&[0x17], 23),
        (&[0x18, 0x18], 24),
        (&[0x18, 0xff], 255),
        (&[0x19, 0x01, 0x00], 256),
        (&[0x19, 0xff, 0xff], 65_535),
        (&[0x1a, 0x00, 0x01, 0x00, 0x00], 65_536),
        (&[0x1a, 0xff, 0xff, 0xff, 0xff], 4_294_967_295),
        (
            &[0x1b, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
            4_294_967_296,
        ),
    ];

    for (bytes, expected) in inputs {
        let value = decode_all::<u64>(bytes).expect("decode");
        assert_eq!(value, *expected);
    }
}

#[test]
fn decode_rejects_indefinite_length() {
    let data = [0x5f];
    let res = decode_all::<&[u8]>(&data);
    assert!(matches!(res, Err(DecodeError::IndefiniteLength)));
}

#[test]
fn encode_nested_array_builder() {
    let mut buf = [0u8; 64];
    let mut encoder = Encoder::<4>::new(&mut buf);
    let mut array = encoder.array().expect("start array");
    array.insert(32u8).expect("first item");
    let mut nested = array.array().expect("start nested");
    nested.insert("foo").expect("nested text");
    nested.insert(128u16).expect("nested int");
    nested.end().expect("end nested");
    array.insert(&[1u8, 2, 3, 4][..]).expect("bytes");
    array.end().expect("end array");

    let expected = [
        0x83, 0x18, 0x20, 0x82, 0x63, 0x66, 0x6f, 0x6f, 0x18, 0x80, 0x44, 0x01, 0x02, 0x03, 0x04,
    ];
    assert_eq!(encoder.encoded(), expected);
}

#[test]
fn encode_map_builder() {
    let mut buf = [0u8; 64];
    let mut encoder = Encoder::<4>::new(&mut buf);
    let mut map = encoder.map().expect("start map");
    map.insert_key_value("a", 1u8).expect("kv a");
    map.insert_key_value("b", "c").expect("kv b");
    map.end().expect("end map");

    let expected = [0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x61, 0x63];
    assert_eq!(encoder.encoded(), expected);
}

#[test]
fn encode_map_builder_bstr_key() {
    let mut buf = [0u8; 64];
    let mut encoder = Encoder::<4>::new(&mut buf);
    let mut map = encoder.map().expect("start map");
    map.insert_key_value(&b"k"[..], 1u8).expect("kv k");
    map.end().expect("end map");

    let expected = [0xa1, 0x41, 0x6b, 0x01];
    assert_eq!(encoder.encoded(), expected);
}

#[test]
fn encode_map_builder_mixed_key_types() {
    let mut buf = [0u8; 64];
    let mut encoder = Encoder::<4>::new(&mut buf);
    let mut map = encoder.map().expect("start map");
    map.insert_key_value(1u8, "one").expect("kv int");
    map.insert_key_value("two", 2u8).expect("kv tstr");
    map.insert_key_value(&b"three"[..], 3u8).expect("kv bstr");
    map.end().expect("end map");

    let expected = [
        0xa3, 0x01, 0x63, 0x6f, 0x6e, 0x65, 0x63, 0x74, 0x77, 0x6f, 0x02, 0x45, 0x74, 0x68, 0x72,
        0x65, 0x65, 0x03,
    ];
    assert_eq!(encoder.encoded(), expected);
}

#[test]
fn encode_map_builder_empty_bstr_key() {
    let mut buf = [0u8; 64];
    let mut encoder = Encoder::<4>::new(&mut buf);
    let mut map = encoder.map().expect("start map");
    map.insert_key_value(&b""[..], 1u8).expect("kv empty bstr");
    map.end().expect("end map");

    let expected = [0xa1, 0x40, 0x01];
    assert_eq!(encoder.encoded(), expected);
}

#[test]
fn decode_map_iter_bstr_key() {
    // {h'6b': 1} — map(1), bstr(1) "k", uint 1
    let data = [0xa1, 0x41, 0x6b, 0x01];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().expect("decode");
    match item {
        Item::Map(map) => {
            let pairs: Vec<(&[u8], u64)> = map
                .iter()
                .map(|res| {
                    let (k, v) = res.expect("pair");
                    let k = match k {
                        Item::Bytes(b) => b,
                        other => panic!("unexpected key {other:?}"),
                    };
                    let v = match v {
                        Item::Unsigned(v) => v,
                        other => panic!("unexpected val {other:?}"),
                    };
                    (k, v)
                })
                .collect();
            assert_eq!(pairs, vec![(&b"k"[..], 1)]);
        }
        other => panic!("unexpected item {other:?}"),
    }
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_map_builder_deterministic_order() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut encoder = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = encoder.map().expect("start map");
    map.insert_key_value("b", 2u8).expect("kv b");
    map.insert_key_value("aa", 3u8).expect("kv aa");
    map.insert_key_value("a", 1u8).expect("kv a");
    map.end().expect("end map");

    let expected = [
        0xa3, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x03,
    ];
    assert_eq!(encoder.encoded(), expected);
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_map_builder_deterministic_order_with_len() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut encoder = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = encoder.map_with_len(3).expect("start map");
    map.insert_key_value("b", 2u8).expect("kv b");
    map.insert_key_value("aa", 3u8).expect("kv aa");
    map.insert_key_value("a", 1u8).expect("kv a");
    map.end().expect("end map");

    let expected = [
        0xa3, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x03,
    ];
    assert_eq!(encoder.encoded(), expected);
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_map_builder_deterministic_order_bstr_keys() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut encoder = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = encoder.map().expect("start map");
    map.insert_key_value(&b"b"[..], 2u8).expect("kv b");
    map.insert_key_value(&b"aa"[..], 3u8).expect("kv aa");
    map.insert_key_value(&b"a"[..], 1u8).expect("kv a");
    map.end().expect("end map");

    // Pure bytewise-lex compares raw encoded bytes directly (no length-first
    // tie-break): b"a"(0x41 0x61) < b"b"(0x41 0x62) < b"aa"(0x42 0x61 0x61),
    // since the header byte 0x41 < 0x42 regardless of what follows.
    let expected = [
        0xa3, 0x41, 0x61, 0x01, 0x41, 0x62, 0x02, 0x42, 0x61, 0x61, 0x03,
    ];
    assert_eq!(encoder.encoded(), expected);
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_map_builder_deterministic_order_int_bstr_tstr_collision() {
    // Keys "a" (tstr), 1 (uint), and b"a" (bstr) all encode to a single byte,
    // so ordering among equal-length keys must fall back to bytewise-lex
    // comparison of the *encoded* bytes: 0x01 (uint 1) < 0x41 0x61 (bstr "a")
    // < 0x61 0x61 (tstr "a").
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut encoder = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = encoder.map().expect("start map");
    map.insert_key_value("a", 1u8).expect("kv tstr a");
    map.insert_key_value(1u8, 2u8).expect("kv int 1");
    map.insert_key_value(&b"a"[..], 3u8).expect("kv bstr a");
    map.end().expect("end map");

    let expected = [
        0xa3, 0x01, 0x02, 0x41, 0x61, 0x03, 0x61, 0x61, 0x01,
    ];
    assert_eq!(encoder.encoded(), expected);
}

#[test]
fn decode_item_array_iter() {
    let data = [0x83, 0x01, 0x02, 0x03];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().expect("decode");
    match item {
        Item::Array(arr) => {
            let values: Vec<u64> = arr
                .iter()
                .map(|res| match res.expect("item") {
                    Item::Unsigned(v) => v,
                    other => panic!("unexpected item {other:?}"),
                })
                .collect();
            assert_eq!(values, vec![1, 2, 3]);
        }
        other => panic!("unexpected item {other:?}"),
    }
}

#[test]
fn decode_item_map_iter() {
    let data = [0xa2, 0x01, 0x02, 0x03, 0x04];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().expect("decode");
    match item {
        Item::Map(map) => {
            let pairs: Vec<(u64, u64)> = map
                .iter()
                .map(|res| {
                    let (k, v) = res.expect("pair");
                    let k = match k {
                        Item::Unsigned(v) => v,
                        other => panic!("unexpected key {other:?}"),
                    };
                    let v = match v {
                        Item::Unsigned(v) => v,
                        other => panic!("unexpected val {other:?}"),
                    };
                    (k, v)
                })
                .collect();
            assert_eq!(pairs, vec![(1, 2), (3, 4)]);
        }
        other => panic!("unexpected item {other:?}"),
    }
}

#[test]
fn decode_item_tag() {
    let data = [0xc1, 0x01];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().expect("decode");
    match item {
        Item::Tag(tagged) => {
            assert_eq!(tagged.tag, 1);
            let inner = tagged.item().expect("tag item");
            assert!(matches!(inner, Item::Unsigned(1)));
        }
        other => panic!("unexpected item {other:?}"),
    }
}

#[test]
fn decode_item_simple_undefined() {
    let data = [0xf0, 0xf7];
    let mut dec = Decoder::new(&data);
    let first = dec.next().expect("first");
    assert!(matches!(first, Item::Simple(16)));
    let second = dec.next().expect("second");
    assert!(matches!(second, Item::Undefined));
}

#[test]
fn decode_all_rejects_trailing_bytes_item() {
    let data = [0x01, 0x02];
    let mut dec = Decoder::new(&data);
    let res = dec.decode();
    assert!(matches!(res, Err(DecodeError::TrailingBytes)));
}

#[test]
fn encode_with_custom_max_depth() {
    let mut buf = [0u8; 64];
    let mut encoder = Encoder::<6>::new(&mut buf);
    let mut outer = encoder.array().expect("start array");
    let inner = outer.array().expect("start nested");
    inner.end().expect("end nested");
    outer.end().expect("end array");
    assert_eq!(encoder.encoded(), [0x81, 0x80]);
}

#[test]
fn encode_with_custom_max_depth_rejects_too_deep() {
    let mut buf = [0u8; 32];
    let mut encoder = Encoder::<1>::new(&mut buf);
    let mut array = encoder.array().expect("start array");
    let res = array.array();
    assert!(matches!(res, Err(EncodeError::NestingTooDeep)));
    array.end().expect("end array");
    assert_eq!(encoder.encoded(), [0x80]);
}

#[cfg(feature = "float")]
#[test]
fn encode_float_canonical_nan() {
    let mut buf = [0u8; 16];
    let mut encoder = Encoder::<4>::new(&mut buf);
    let val = half::f16::from_bits(0x7e01);
    encoder.insert(val).expect("encode f16");
    assert_eq!(encoder.encoded(), &[0xf9, 0x7e, 0x00]);

    let mut encoder = Encoder::<4>::new(&mut buf);
    let val = f32::from_bits(0x7fc0_0001);
    encoder.insert(val).expect("encode f32");
    assert_eq!(encoder.encoded(), &[0xf9, 0x7e, 0x00]);
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_deterministic_map_free_fn() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];

    // Build a value that contains a map with out-of-order keys using
    // the free encode_deterministic function and verify the keys are sorted.
    let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = enc.map().expect("map");
    map.insert_key_value("b", 2u8).expect("b");
    map.insert_key_value("aa", 3u8).expect("aa");
    map.insert_key_value("a", 1u8).expect("a");
    map.end().expect("end");

    // RFC 8949 bytewise-lex sorted: "a"(1 byte) < "b"(1 byte, lex) < "aa"(2 bytes)
    let expected = [
        0xa3, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x03,
    ];
    assert_eq!(enc.encoded(), expected);
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_deterministic_nested_map() {
    // Verify that nested maps are also sorted.
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut outer = enc.map().expect("outer map");
    let mut inner = outer.key(1u8).expect("key 1").map().expect("inner map");
    inner.insert_key_value("b", 2u8).expect("b");
    inner.insert_key_value("a", 1u8).expect("a");
    inner.end().expect("end inner");
    outer.end().expect("end outer");

    // outer map: {1: {"a": 1, "b": 2}} — inner map keys sorted
    let expected = [
        0xa1, 0x01, // map(1), key=1
        0xa2, // map(2)
        0x61, 0x61, 0x01, // "a": 1
        0x61, 0x62, 0x02, // "b": 2
    ];
    assert_eq!(enc.encoded(), expected);
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_deterministic_nested_map_bstr_keys() {
    // Verify that nested maps with bstr keys are also sorted.
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut outer = enc.map().expect("outer map");
    let mut inner = outer.key(1u8).expect("key 1").map().expect("inner map");
    inner.insert_key_value(&b"b"[..], 2u8).expect("b");
    inner.insert_key_value(&b"a"[..], 1u8).expect("a");
    inner.end().expect("end inner");
    outer.end().expect("end outer");

    // outer map: {1: {h'61': 1, h'62': 2}} — inner map keys sorted
    let expected = [
        0xa1, 0x01, // map(1), key=1
        0xa2, // map(2)
        0x41, 0x61, 0x01, // h'61': 1
        0x41, 0x62, 0x02, // h'62': 2
    ];
    assert_eq!(enc.encoded(), expected);
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_deterministic_rejects_duplicate_keys() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = enc.map().expect("map");
    map.insert_key_value("a", 1u8).expect("first a");
    map.insert_key_value("a", 2u8).expect("second a inserted");
    let result = map.end();
    assert!(matches!(result, Err(EncodeError::DuplicateKey)));
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_deterministic_rejects_duplicate_bstr_keys() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = enc.map().expect("map");
    map.insert_key_value(&b"a"[..], 1u8).expect("first a");
    map.insert_key_value(&b"a"[..], 2u8)
        .expect("second a inserted");
    let result = map.end();
    assert!(matches!(result, Err(EncodeError::DuplicateKey)));
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_deterministic_rejects_duplicate_integer_keys() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = enc.map().expect("map");
    map.insert_key_value(1u8, "first").expect("first 1");
    map.insert_key_value(1u8, "second")
        .expect("second 1 inserted");
    let result = map.end();
    assert!(matches!(result, Err(EncodeError::DuplicateKey)));
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn new_deterministic_rejects_undersized_scratch() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 32];
    let result = Encoder::<4>::new_deterministic(&mut buf, &mut scratch);
    assert!(matches!(result, Err(EncodeError::OutOfSpace(64))));
}

#[test]
fn new_with_overflow_rescues_header_shrink() {
    // Header reservation (9 bytes) plus 3 one-byte items pushes the logical position to 12,
    // beyond the 10-byte buffer. Once the array header shrinks to 1 byte (count == 3), the
    // logical position drops back to 4, which fits. With overflow_scratch supplied, the real
    // byte values pushed past `buf` are preserved so the shrink can recover them correctly.
    let mut buf = [0u8; 10];
    let mut overflow = [0u8; 8];
    let mut encoder = Encoder::<4>::new_with_overflow(&mut buf, &mut overflow);
    let mut array = encoder.array().expect("start array");
    array.insert(1u8).expect("first item");
    array.insert(2u8).expect("second item");
    array.insert(3u8).expect("third item");
    array.end().expect("rescued end array");
    assert_eq!(encoder.encoded(), [0x83, 0x01, 0x02, 0x03]);
}

#[test]
fn new_without_overflow_cannot_rescue_header_shrink() {
    // Same shape as `new_with_overflow_rescues_header_shrink`, but without a scratch buffer to
    // preserve the real byte values pushed past `buf`. The eventual shrink would bring the
    // logical position back to a value that fits, but the lost bytes make the rescue unsound,
    // so this must still fail — with the exact number of bytes the complete encode requires.
    let mut buf = [0u8; 10];
    let mut encoder = Encoder::<4>::new(&mut buf);
    let mut array = encoder.array().expect("start array");
    array.insert(1u8).expect("first item");
    array.insert(2u8).expect("second item");
    array.insert(3u8).expect("third item");
    let result = array.end();
    assert!(matches!(result, Err(EncodeError::OutOfSpace(4))));
}

#[test]
fn encode_map_rejects_duplicate_keys_always() {
    // Duplicate detection must fire even without deterministic_encoding feature.
    let mut buf = [0u8; 32];
    let mut enc = Encoder::<4>::new(&mut buf);
    let mut map = enc.map().expect("map");
    map.insert_key_value(1u8, "first").expect("first");
    map.insert_key_value(1u8, "second")
        .expect("second inserted");
    let result = map.end();
    assert!(matches!(result, Err(EncodeError::DuplicateKey)));
}

#[test]
fn encode_map_rejects_duplicate_bstr_keys_always() {
    // Duplicate detection for bstr keys must fire even without
    // deterministic_encoding feature.
    let mut buf = [0u8; 32];
    let mut enc = Encoder::<4>::new(&mut buf);
    let mut map = enc.map().expect("map");
    map.insert_key_value(&b"a"[..], "first").expect("first");
    map.insert_key_value(&b"a"[..], "second")
        .expect("second inserted");
    let result = map.end();
    assert!(matches!(result, Err(EncodeError::DuplicateKey)));
}

#[cfg(feature = "float")]
#[test]
fn decode_f64_round_trip() {
    use tps_minicbor::encode;
    let value = 1.5f64;
    let mut buf = [0u8; 16];
    let len = encode(&value, &mut buf).expect("encode");
    let decoded = decode_all::<f64>(&buf[..len]).expect("decode");
    assert_eq!(decoded, value);
}

#[cfg(feature = "float")]
#[test]
fn decode_f32_round_trip() {
    use tps_minicbor::encode;
    let value = 1.5f32;
    let mut buf = [0u8; 16];
    let len = encode(&value, &mut buf).expect("encode");
    let decoded = decode_all::<f32>(&buf[..len]).expect("decode");
    assert_eq!(decoded, value);
}

#[test]
fn encode_tag_builder() {
    let mut buf = [0u8; 32];
    let mut encoder = Encoder::<4>::new(&mut buf);
    encoder.tag(1).expect("tag").item(1u8).expect("tag item");
    assert_eq!(encoder.encoded(), [0xc1, 0x01]);
}

// ── RFC 8949 §3.3 / Appendix F: reserved simple-value encodings ────────────────
//
// `Simple::new` is the only public constructor for the `Simple` wrapper type,
// and it must reject the entire reserved range 20-31 (20-23 are the dedicated
// false/true/null/undefined values; 24-31 are reserved two-byte encodings that
// RFC 8949 Appendix F explicitly calls out as not well-formed, e.g. `f8 18`,
// `f8 1f`). The crate-internal `Encoder::simple` helper also rejects the full
// 20..=31 range directly in its own match arms (Stage 1b Finding 7 fix), so
// this invariant holds independent of the `Simple::new` guard.
#[test]
fn simple_new_rejects_entire_reserved_range() {
    for v in 20u8..=31 {
        assert!(
            matches!(tps_minicbor::Simple::new(v), Err(EncodeError::InvalidValue)),
            "Simple::new({v}) should be rejected (RFC 8949 Appendix F reserved range)"
        );
    }
    // Boundary check: values just outside the reserved range are accepted.
    assert!(tps_minicbor::Simple::new(19).is_ok());
    assert!(tps_minicbor::Simple::new(32).is_ok());
}

// ── RFC 8949 Appendix F, Subkind 2: reserved two-byte simple-value encodings ───
//
// RFC 8949 §3.3: "An encoder MUST NOT issue two-byte sequences that start
// with 0xf8 ... and continue with a byte less than 0x20 (32 decimal). Such
// sequences are not well-formed." Appendix F's example list of non-well-formed
// inputs is explicit: "f8 00, f8 01, f8 18, f8 1f" — i.e. the *entire* 0..=31
// range, not just the false/true/null/undefined sub-range 20..=23.
//
// Stage 1b fix: `Decoder::next`/`Decoder::skip_item` now reject the whole
// 0..=31 range for the two-byte simple-value form, per Stage 1 finding
// "decoder accepts non-well-formed 0xf8 0x00..0x13 two-byte simple values".
#[test]
fn decode_rejects_non_well_formed_two_byte_simple_values_0_to_19() {
    for v in 0u8..=19 {
        let data = [0xf8, v];
        let mut dec = Decoder::new(&data);
        let result = dec.next();
        assert!(
            matches!(result, Err(DecodeError::InvalidAdditionalInfo)),
            "0xf8 {v:#04x} should be rejected as non-well-formed, got {result:?}"
        );
    }
}

#[test]
fn decode_rejects_reserved_two_byte_simple_values_20_to_31() {
    for v in 20u8..=31 {
        let data = [0xf8, v];
        let mut dec = Decoder::new(&data);
        let result = dec.next();
        assert!(
            matches!(result, Err(DecodeError::InvalidAdditionalInfo)),
            "0xf8 {v:#04x} should be rejected as non-well-formed, got {result:?}"
        );
    }
    // Values >= 32 in the two-byte form are well-formed (unassigned simple values).
    let data = [0xf8, 32];
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Ok(Item::Simple(32))));
    let data = [0xf8, 255];
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Ok(Item::Simple(255))));
}

// ── RFC 8949 Appendix F, Subkind 1: reserved additional-information values ─────
//
// Additional info values 28, 29, and 30 are reserved for all major types and
// must always be rejected, independent of which major type they're paired
// with (major type 0 unsigned, 1 negative, 2 bytes, 3 text, 4 array, 5 map,
// 7 simple/float all use the same header-parsing logic).
#[test]
fn decode_rejects_reserved_additional_info_across_major_types() {
    for major in [0u8, 1, 2, 3, 4, 5, 7] {
        for addl in [28u8, 29, 30] {
            let first_byte = (major << 5) | addl;
            let data = [first_byte];
            let mut dec = Decoder::new(&data);
            let result = dec.next();
            assert!(
                matches!(result, Err(DecodeError::InvalidAdditionalInfo)),
                "major type {major} with reserved additional info {addl} ({first_byte:#04x}) \
                 should be rejected, got {result:?}"
            );
        }
    }
}

// ── RFC 8949 §4.2.1: preferred float serialization must select the shortest
// width that round-trips exactly, including for subnormal values ─────────────
#[cfg(feature = "float")]
#[test]
fn encode_decode_f64_subnormal_round_trip() {
    use tps_minicbor::encode;
    // Smallest positive subnormal f64 (not representable exactly in f16 or
    // f32, so must be encoded as a full 8-byte float).
    let value = f64::from_bits(1);
    let mut buf = [0u8; 16];
    let len = encode(&value, &mut buf).expect("encode");
    assert_eq!(buf[0], 0xfb, "subnormal f64 should use the 8-byte float form");
    let decoded = decode_all::<f64>(&buf[..len]).expect("decode");
    assert_eq!(decoded.to_bits(), value.to_bits());
}

#[cfg(feature = "float")]
#[test]
fn encode_f16_smallest_subnormal_round_trip() {
    // Smallest positive subnormal in half-precision: bit pattern 0x0001.
    // RFC 8949 §4.2.1's shortest-form rule means this value, which is
    // exactly representable in f16, must be encoded in the 3-byte f16 form.
    use tps_minicbor::encode;
    let value = half::f16::from_bits(1).to_f64();
    let mut buf = [0u8; 16];
    let len = encode(&value, &mut buf).expect("encode");
    assert_eq!(&buf[..len], &[0xf9, 0x00, 0x01]);
    let decoded = decode_all::<f64>(&buf[..len]).expect("decode");
    assert_eq!(decoded.to_bits(), value.to_bits());
}

// ── RFC 8949 §4.2.3: length-first map key ordering (RFC 7049-compatible) ───────
//
// `deterministic_encoding` implements the *length-first* ordering of §4.2.3,
// not the pure bytewise-lexicographic "core deterministic encoding
// requirements" ordering of §4.2.1. This is disclosed in the crate's top
// level documentation. This test pins the behaviour using the RFC's own
// §4.2.3 example key set {10, -1, false, 100, "z"}, whose correctly
// length-first-sorted order (10, -1, false, 100, "z") differs from the pure
// bytewise-lex order given in §4.2.1 (10, 100, -1, "z", "aa") for an
// overlapping example set. Here all five keys are one byte when encoded
// except 100 (0x1864, two bytes), so length-first vs. pure bytewise-lex
// agree on relative order except that both orderings happen to agree for
// this particular key set on final placement of the two-byte key last;
// what this test actually pins down is that 1-byte keys are sorted purely
// by encoded-byte value (0x0a < 0x20 < 0xf4) before the 2-byte key (0x1864).
// ── RFC 8949 §4.2.1: pure bytewise-lexicographic map key ordering ─────────────
//
// Per human ruling on Stage 1 Finding 4, `deterministic_encoding` implements
// the pure bytewise-lexicographic "core deterministic encoding requirements"
// ordering of §4.2.1, not the alternative length-first ordering of §4.2.3.
// This test uses the RFC's own key set {10, -1, false, 100, "z"} (from the
// §4.2.3 length-first example) and pins the pure bytewise-lex order instead:
// comparing raw encoded bytes byte-by-byte gives 10 (0x0a) < 100 (0x18 0x64)
// < -1 (0x20) < "z" (0x61 0x7a) < false (0xf4), since the first byte alone
// (0x0a < 0x18 < 0x20 < 0x61 < 0xf4) already orders every key here.
#[cfg(feature = "deterministic_encoding")]
#[test]
fn encode_deterministic_bytewise_lex_ordering_rfc_example() {
    let mut buf = [0u8; 64];
    let mut scratch = [0u8; 64];
    let mut encoder = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = encoder.map().expect("start map");
    // Insert out of order.
    map.insert_key_value(100u8, "hundred").expect("100");
    map.insert_key_value("z", "letter").expect("z");
    map.insert_key_value(false, "bool").expect("false");
    map.insert_key_value(-1i8, "neg one").expect("-1");
    map.insert_key_value(10u8, "ten").expect("10");
    map.end().expect("end map");

    // Expected pure bytewise-lex order: 10 (0x0a), 100 (0x1864), -1 (0x20),
    // "z" (0x617a), false (0xf4).
    let mut dec = Decoder::new(encoder.encoded());
    let map = dec.next().expect("decode map");
    let Item::Map(map) = map else {
        panic!("expected map");
    };
    let keys: std::vec::Vec<_> = map
        .iter()
        .map(|pair| pair.expect("pair").0)
        .collect();
    assert!(matches!(keys[0], Item::Unsigned(10)));
    assert!(matches!(keys[1], Item::Unsigned(100)));
    assert!(matches!(keys[2], Item::Negative(-1)));
    assert!(matches!(keys[3], Item::Text("z")));
    assert!(matches!(keys[4], Item::Bool(false)));
}

// ── `skip_item` also rejects the non-well-formed 0xf8 0x00..0x13 range ────────
//
// `Decoder::skip_item` (used e.g. when an array/map element is skipped while
// locating a later element, or while computing an array's span) shares the
// same `(0..=31)` range check as `next()`, fixed in Stage 1b. This test
// exercises that code path by embedding the non-well-formed sequence as the
// first element of a two-element array: `next()` bounds the array eagerly via
// `skip_items`/`skip_item`, so decoding the array itself now fails.
#[test]
fn skip_item_rejects_non_well_formed_two_byte_simple_value() {
    // Array [simple(0) as f8 00, 1] = 0x82 0xf8 0x00 0x01
    let data = [0x82, 0xf8, 0x00, 0x01];
    let mut dec = Decoder::new(&data);
    let result = dec.next();
    assert!(matches!(result, Err(DecodeError::InvalidAdditionalInfo)));
}

// ── Stage 3: a byte/text string header lying about its length must not
// overflow `pos + len` and must not be reinterpreted as an in-bounds length ──
//
// A CBOR byte/text string header can declare any 64-bit length. Before the
// Stage 3 fix, `self.pos + len > self.buf.len()` computed with a plain `+`
// could itself overflow (panicking on debug builds) or wrap around to a
// small value (silently treating the lying header as if it were a short,
// in-bounds string on release builds, returning unrelated buffer bytes as
// the "decoded" value instead of rejecting the input).

#[test]
fn decode_bytes_header_with_max_u64_length_returns_error() {
    // Byte string (major 2), 8-byte length field = u64::MAX.
    let data = [0x5b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Err(DecodeError::UnexpectedEof)));

    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.bytes(), Err(DecodeError::UnexpectedEof)));
}

#[test]
fn decode_text_header_with_max_u64_length_returns_error() {
    // Text string (major 3), 8-byte length field = u64::MAX.
    let data = [0x7b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Err(DecodeError::UnexpectedEof)));

    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.text(), Err(DecodeError::UnexpectedEof)));
}

#[test]
fn decode_nested_bytes_inside_array_with_max_u64_length_returns_error() {
    // Array of 1 element, whose element is a byte string with an 8-byte
    // length field of u64::MAX. Exercises `skip_bytes` via `skip_item`,
    // which `next()` uses to bound the array's span.
    let mut data = std::vec![0x81u8, 0x5b];
    data.extend_from_slice(&[0xff; 8]);
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Err(DecodeError::UnexpectedEof)));
}

#[test]
fn decode_crafted_length_that_wraps_to_in_bounds_offset_is_rejected() {
    // Byte string header whose 8-byte length, added to `pos` (9, the header
    // size), wraps back around to a small in-bounds value if computed with
    // plain unchecked `usize` addition. Must still be rejected outright,
    // never accepted as a short in-bounds string.
    let pos_after_header = 9u64;
    let len = 0u64.wrapping_sub(pos_after_header).wrapping_add(6);
    let mut data = std::vec![0x5bu8];
    data.extend_from_slice(&len.to_be_bytes());
    data.extend_from_slice(&[0xaa; 11]);
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Err(DecodeError::UnexpectedEof)));
}

// ── Stage 3: unbounded array/map/tag nesting must not overflow the stack ──
//
// `next()`'s `MAJOR_ARRAY`/`MAJOR_MAP`/`MAJOR_TAG` arms eagerly walk the full
// nested structure via `skip_items`/`skip_item` before ever returning to the
// caller. Before the Stage 3 fix this recursion had no depth limit, so a
// few-hundred-byte payload of deeply nested containers could exhaust the
// call stack. `MAX_DECODE_DEPTH` bounds this recursion, returning
// `DecodeError::NestingTooDeep` once exceeded.

#[test]
fn decode_deeply_nested_tags_returns_nesting_too_deep() {
    // 1000 nested tag headers (tag 0 = 0xc0), terminated by an unsigned 0.
    let mut data = std::vec![0xc0u8; 1000];
    data.push(0x00);
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Err(DecodeError::NestingTooDeep)));
}

#[test]
fn decode_deeply_nested_arrays_returns_nesting_too_deep() {
    // 1000 nested single-element arrays (0x81), terminated by an unsigned 0.
    let mut data = std::vec![0x81u8; 1000];
    data.push(0x00);
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Err(DecodeError::NestingTooDeep)));
}

#[test]
fn decode_deeply_nested_maps_returns_nesting_too_deep() {
    // 1000 nested single-entry maps (0xa1) whose value is the next map,
    // terminated by a key/value pair of unsigned integers.
    let mut data = std::vec![0xa1u8; 1000];
    data.push(0x00);
    data.push(0x00);
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.next(), Err(DecodeError::NestingTooDeep)));
}

#[test]
fn decode_moderately_nested_tags_within_limit_succeeds() {
    // Well within MAX_DECODE_DEPTH: nesting this shallow must keep working.
    let mut data = std::vec![0xc0u8; 10];
    data.push(0x00);
    let mut dec = Decoder::new(&data);
    assert!(dec.next().is_ok());
}
