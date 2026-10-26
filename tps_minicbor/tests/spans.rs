/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![cfg(feature = "spans")]

use tps_minicbor::{Decoder, Item};

#[test]
fn next_with_span_scalar() {
    // 0x18 0x2a = uint(42), two bytes.
    let data = [0x18, 0x2a];
    let mut dec = Decoder::new(&data);
    let (item, span) = dec.next_with_span().unwrap();
    assert!(matches!(item, Item::Unsigned(42)));
    assert_eq!(span, 0..2);
}

#[test]
fn next_with_span_second_item_offset() {
    let data = [0x01, 0x18, 0x2a];
    let mut dec = Decoder::new(&data);
    let _ = dec.next_with_span().unwrap();
    let (item, span) = dec.next_with_span().unwrap();
    assert!(matches!(item, Item::Unsigned(42)));
    assert_eq!(span, 1..3);
}

#[test]
fn array_iter_with_span() {
    // [1, 2] => 0x82 0x01 0x02
    let data = [0x82, 0x01, 0x02];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().unwrap();
    let arr = match item {
        Item::Array(a) => a,
        _ => panic!("expected array"),
    };
    let spans: Vec<_> = arr
        .iter_with_span()
        .map(|r| r.unwrap().1)
        .collect();
    // Spans are relative to the array's own data (starts right after 0x82).
    assert_eq!(spans, vec![0..1, 1..2]);
}

#[test]
fn map_iter_with_span() {
    // {1: 2} => 0xa1 0x01 0x02
    let data = [0xa1, 0x01, 0x02];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().unwrap();
    let map = match item {
        Item::Map(m) => m,
        _ => panic!("expected map"),
    };
    let pairs: Vec<_> = map
        .iter_with_span()
        .map(|r| {
            let ((_, kspan), (_, vspan)) = r.unwrap();
            (kspan, vspan)
        })
        .collect();
    assert_eq!(pairs, vec![(0..1, 1..2)]);
}

#[test]
fn tagged_item_with_span() {
    // #6.1(42) => 0xc1 0x18 0x2a
    let data = [0xc1, 0x18, 0x2a];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().unwrap();
    let tagged = match item {
        Item::Tag(t) => t,
        _ => panic!("expected tag"),
    };
    let (inner, span) = tagged.item_with_span().unwrap();
    assert!(matches!(inner, Item::Unsigned(42)));
    assert_eq!(span, 0..2);
}

#[test]
fn nested_array_span_composes_with_absolute_start() {
    // [10, [20, 21]] => 0x82 0x0a 0x82 0x14 0x15
    let data = [0x82, 0x0a, 0x82, 0x14, 0x15];
    let mut dec = Decoder::new(&data);
    let (outer, outer_span) = dec.next_with_span().unwrap();
    assert_eq!(outer_span, 0..5);
    let outer_arr = match outer {
        Item::Array(a) => a,
        _ => panic!("expected array"),
    };
    let items: Vec<_> = outer_arr.iter_with_span().map(|r| r.unwrap()).collect();
    // First item (10) relative span within the outer array's data.
    assert_eq!(items[0].1, 0..1);
    // Second item is the nested array itself, occupying the remaining bytes.
    assert_eq!(items[1].1, 1..4);
    let inner_arr = match &items[1].0 {
        Item::Array(a) => a,
        _ => panic!("expected inner array"),
    };
    let inner_items: Vec<_> = inner_arr.iter_with_span().map(|r| r.unwrap()).collect();
    assert_eq!(inner_items[0].1, 0..1);
    assert_eq!(inner_items[1].1, 1..2);
    // Composing: outer_span.start (0) + items[1].1.start (1) + inner_items[0].1.start (0) == 1,
    // which is exactly where the nested array's header byte (0x82) sits in `data`.
}
