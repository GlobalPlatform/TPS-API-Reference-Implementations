/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use tps_minicbor::{Decode, DecodeError, Decoder, Encoder, EncoderStd, Item};

#[test]
fn readme_basic_encoder_std() {
    let mut buf = [0u8; 16];
    let mut enc = EncoderStd::new(&mut buf);
    enc.insert(1u8).unwrap();
    assert_eq!(enc.encoded(), &[0x01]);

    let mut buf = [0u8; 16];
    let mut enc = Encoder::<8>::new(&mut buf);
    enc.insert(2u8).unwrap();
    assert_eq!(enc.encoded(), &[0x02]);
}

#[test]
fn readme_array_encoding() {
    let mut buf = [0u8; 16];
    let mut enc = Encoder::<2>::new(&mut buf);
    let mut array = enc.array().unwrap();
    array.insert(1u8).unwrap();
    array.insert(2u8).unwrap();
    array.insert(3u8).unwrap();
    array.end().unwrap();
    assert_eq!(enc.encoded(), &[0x83, 0x01, 0x02, 0x03]);
}

#[test]
fn readme_map_insertion_order() {
    let mut buf = [0u8; 32];
    let mut enc = Encoder::<4>::new(&mut buf);
    let mut map = enc.map().unwrap();
    map.insert_key_value("b", 2u8).unwrap();
    map.insert_key_value("aa", 3u8).unwrap();
    map.insert_key_value("a", 1u8).unwrap();
    map.end().unwrap();
    assert_eq!(
        enc.encoded(),
        &[
            0xa3, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x03, 0x61, 0x61, 0x01
        ]
    );
}

#[cfg(feature = "deterministic_encoding")]
#[test]
fn readme_deterministic_map() {
    let mut buf = [0u8; 32];
    let mut scratch = [0u8; 32];
    let mut enc = EncoderStd::new_deterministic(&mut buf, &mut scratch).unwrap();
    let mut map = enc.map().unwrap();
    map.insert_key_value("b", 2u8).unwrap();
    map.insert_key_value("aa", 3u8).unwrap();
    map.insert_key_value("a", 1u8).unwrap();
    map.end().unwrap();
    assert_eq!(
        enc.encoded(),
        &[
            0xa3, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x03
        ]
    );
}

#[test]
fn readme_basic_decode() {
    let data = [0x01];
    let mut dec = Decoder::new(&data);
    let item = dec.decode().unwrap();
    assert!(matches!(item, Item::Unsigned(1)));
}

#[test]
fn readme_decode_impl() {
    struct Point {
        x: i32,
        y: i32,
    }
    impl<'a> Decode<'a> for Point {
        fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
            let len = d.array_len()?;
            if len != 2 {
                return Err(DecodeError::InvalidAdditionalInfo);
            }
            let x = i32::decode(d)?;
            let y = i32::decode(d)?;
            Ok(Point { x, y })
        }
    }
    // CBOR: [10, -3]  →  82 0a 22
    let data = [0x82, 0x0a, 0x22];
    let mut dec = Decoder::new(&data);
    let p = Point::decode(&mut dec).unwrap();
    assert_eq!(p.x, 10);
    assert_eq!(p.y, -3);
}

#[test]
fn readme_array_iteration() {
    use std::vec::Vec;
    let data = [0x82, 0x01, 0x02];
    let mut dec = Decoder::new(&data);
    let array = dec.next_array().unwrap();
    let items: Vec<u64> = array
        .iter()
        .map(|r| match r.unwrap() {
            Item::Unsigned(v) => v,
            _ => 0,
        })
        .collect();
    assert_eq!(items, vec![1, 2]);
}
