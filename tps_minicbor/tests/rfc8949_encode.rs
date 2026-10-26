/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use tps_minicbor::{EncodeError, Encoder, Simple, Tag, Undefined};

#[test]
fn rfc8949_encode_int() -> Result<(), EncodeError> {
    println!("<======================= rfc8949_encode_int =====================>");
    let mut bytes = [0u8; 32];
    let u1: &[u8] = &[0x00];
    let u2: &[u8] = &[0x01];
    let u3: &[u8] = &[0x0a];
    let u4: &[u8] = &[0x17];
    let u5: &[u8] = &[0x18, 0x18];
    let u6: &[u8] = &[0x18, 0x19];
    let u7: &[u8] = &[0x18, 0x64];
    let u8: &[u8] = &[0x19, 0x03, 0xe8];
    let u9: &[u8] = &[0x1a, 0x00, 0x0f, 0x42, 0x40];
    let u10: &[u8] = &[0x1b, 0x00, 0x00, 0x00, 0xe8, 0xd4, 0xa5, 0x10, 0x00];
    let u11: &[u8] = &[0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let s1: &[u8] = &[0x20];
    let s2: &[u8] = &[0x29];
    let s3: &[u8] = &[0x38, 0x63];
    let s4: &[u8] = &[0x39, 0x03, 0xe7];
    let s5: &[u8] = &[0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];

    // 64 bit encodings
    for &(val, expect) in [
        (0u64, u1),
        (1u64, u2),
        (10u64, u3),
        (23u64, u4),
        (24u64, u5),
        (25u64, u6),
        (100u64, u7),
        (1000u64, u8),
        (1000000u64, u9),
        (1000000000000u64, u10),
        (18446744073709551615, u11),
    ]
    .iter()
    {
        println!(
            "<======================= Encode u64 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // 32 bit encodings
    for &(val, expect) in [
        (0u32, u1),
        (1u32, u2),
        (10u32, u3),
        (23u32, u4),
        (24u32, u5),
        (25u32, u6),
        (100u32, u7),
        (1000u32, u8),
        (1000000u32, u9),
    ]
    .iter()
    {
        println!(
            "<======================= Encode u32 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // 16 bit encodings
    for &(val, expect) in [
        (0u16, u1),
        (1u16, u2),
        (10u16, u3),
        (23u16, u4),
        (24u16, u5),
        (25u16, u6),
        (100u16, u7),
        (1000u16, u8),
    ]
    .iter()
    {
        println!(
            "<======================= Encode u16 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // 8 bit encodings
    for &(val, expect) in [
        (0u8, u1),
        (1u8, u2),
        (10u8, u3),
        (23u8, u4),
        (24u8, u5),
        (25u8, u6),
        (100u8, u7),
    ]
    .iter()
    {
        println!(
            "<======================= Encode u8 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // Concatenations
    {
        println!("<======================= Concatenate 2 x u32 =====================>");
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(1000000u32)?;
        encoder.insert(1000001u32)?;
        assert_eq!(
            encoder.encoded(),
            &[0x1a, 0x00, 0x0f, 0x42, 0x40, 0x1a, 0x00, 0x0f, 0x42, 0x41]
        );
    }

    // i64 encodings
    for &(val, expect) in [
        (0i64, u1),
        (1i64, u2),
        (10i64, u3),
        (23i64, u4),
        (24i64, u5),
        (25i64, u6),
        (100i64, u7),
        (1000i64, u8),
        (1000000i64, u9),
        (1000000000000i64, u10),
        (-1i64, s1),
        (-10i64, s2),
        (-100i64, s3),
        (-1000i64, s4),
    ]
    .iter()
    {
        println!(
            "<======================= Encode i64 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // i32 encodings
    for &(val, expect) in [
        (0i32, u1),
        (1i32, u2),
        (10i32, u3),
        (23i32, u4),
        (24i32, u5),
        (25i32, u6),
        (100i32, u7),
        (1000i32, u8),
        (1000000i32, u9),
        (-1i32, s1),
        (-10i32, s2),
        (-100i32, s3),
        (-1000i32, s4),
    ]
    .iter()
    {
        println!(
            "<======================= Encode i32 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // i16 encodings
    for &(val, expect) in [
        (0i16, u1),
        (1i16, u2),
        (10i16, u3),
        (23i16, u4),
        (24i16, u5),
        (25i16, u6),
        (100i16, u7),
        (1000i16, u8),
        (-1i16, s1),
        (-10i16, s2),
        (-100i16, s3),
        (-1000i16, s4),
    ]
    .iter()
    {
        println!(
            "<======================= Encode i16 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // i8 encodings
    for &(val, expect) in [
        (0i16, u1),
        (1i16, u2),
        (10i16, u3),
        (23i16, u4),
        (24i16, u5),
        (25i16, u6),
        (100i16, u7),
        (-1i16, s1),
        (-10i16, s2),
        (-100i16, s3),
    ]
    .iter()
    {
        println!(
            "<======================= Encode i8 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    // i128 encodings
    for &(val, expect) in [
        (0i128, u1),
        (1i128, u2),
        (10i128, u3),
        (23i128, u4),
        (24i128, u5),
        (25i128, u6),
        (100i128, u7),
        (1000i128, u8),
        (1000000i128, u9),
        (1000000000000i128, u10),
        (-1i128, s1),
        (-10i128, s2),
        (-100i128, s3),
        (-1000i128, s4),
        (-18446744073709551616i128, s5),
    ]
    .iter()
    {
        println!(
            "<======================= Encode i64 {} =====================>",
            val
        );
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), expect);
    }

    Ok(())
}

#[test]
fn rfc8949_encode_tstr() -> Result<(), EncodeError> {
    println!("<======================= rfc8949_encode_tstr =====================>");
    let mut bytes = [0u8; 32];

    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert("")?;
        assert_eq!(encoder.encoded(), &[0x60]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert("a")?;
        assert_eq!(encoder.encoded(), &[0x61, 0x61]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert("IETF")?;
        assert_eq!(encoder.encoded(), &[0x64, 0x49, 0x45, 0x54, 0x46]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert("\"\\")?;
        assert_eq!(encoder.encoded(), &[0x62, 0x22, 0x5c]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert("\u{00fc}")?;
        assert_eq!(encoder.encoded(), &[0x62, 0xc3, 0xbc]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert("\u{6c34}")?;
        assert_eq!(encoder.encoded(), &[0x63, 0xe6, 0xb0, 0xb4]);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_bstr() -> Result<(), EncodeError> {
    println!("<======================= rfc8949_encode_tstr =====================>");
    let mut bytes = [0u8; 32];

    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val: &[u8] = &[];
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0x40]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val: &[u8] = &[1, 2, 3, 4];
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0x44, 0x01, 0x02, 0x03, 0x04]);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_simple() -> Result<(), EncodeError> {
    println!("<======================= rfc8949_encode_simple =====================>");
    let mut bytes = [0u8; 32];

    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(false)?;
        assert_eq!(encoder.encoded(), &[0xf4]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(true)?;
        assert_eq!(encoder.encoded(), &[0xf5]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(())?;
        assert_eq!(encoder.encoded(), &[0xf6]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(Undefined)?;
        assert_eq!(encoder.encoded(), &[0xf7]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(Simple::new(16)?)?;
        assert_eq!(encoder.encoded(), &[0xf0]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(Simple::new(255)?)?;
        assert_eq!(encoder.encoded(), &[0xf8, 0xff]);
    }
    Ok(())
}

#[test]
#[cfg(feature = "float")]
fn rfc8949_encode_float() -> Result<(), EncodeError> {
    println!("<======================= rfc8949_encode_float ======================>");
    let mut bytes = [0u8; 32];

    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 0.0;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x00, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = -0.0;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x80, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 1.0;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x3c, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 1.1;
        encoder.insert(val)?;
        assert_eq!(
            encoder.encoded(),
            &[0xfb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a]
        );
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 1.5;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x3e, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 65504.0;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x7b, 0xff]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 100000.0;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xfa, 0x47, 0xc3, 0x50, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 3.4028234663852886e+38;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xfa, 0x7f, 0x7f, 0xff, 0xff]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 1.0e+300;
        encoder.insert(val)?;
        assert_eq!(
            encoder.encoded(),
            &[0xfb, 0x7e, 0x37, 0xe4, 0x3c, 0x88, 0x00, 0x75, 0x9c]
        );
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 5.960464477539063e-8;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x00, 0x01]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = 0.00006103515625;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x04, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = -4.0;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0xc4, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = -4.1;
        encoder.insert(val)?;
        assert_eq!(
            encoder.encoded(),
            &[0xfb, 0xc0, 0x10, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66]
        );
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = half::f16::INFINITY;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x7c, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = half::f16::NAN;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0x7e, 0x00]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let val = half::f16::NEG_INFINITY;
        encoder.insert(val)?;
        assert_eq!(encoder.encoded(), &[0xf9, 0xfc, 0x00]);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_arrays() -> Result<(), EncodeError> {
    let mut bytes = [0u8; 128];

    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.array()?.end()?;
        assert_eq!(encoder.encoded(), &[0x80]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let mut array = encoder.array()?;
        array.insert(1u8)?;
        array.insert(2u8)?;
        array.insert(3u8)?;
        array.end()?;
        assert_eq!(encoder.encoded(), &[0x83, 0x01, 0x02, 0x03]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let mut array = encoder.array()?;
        array.insert(1u8)?;
        let mut nested = array.array()?;
        nested.insert(2u8)?;
        nested.insert(3u8)?;
        nested.end()?;
        let mut nested = array.array()?;
        nested.insert(4u8)?;
        nested.insert(5u8)?;
        nested.end()?;
        array.end()?;
        assert_eq!(
            encoder.encoded(),
            &[0x83, 0x01, 0x82, 0x02, 0x03, 0x82, 0x04, 0x05]
        );
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let mut arr = encoder.array()?;
        for value in 1u8..=25 {
            arr.insert(value)?;
        }
        arr.end()?;
        let expected = [
            0x98, 0x19, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
            0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x18, 0x18,
            0x19,
        ];
        assert_eq!(encoder.encoded(), expected);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let mut array = encoder.array()?;
        array.insert("a")?;
        let mut map = array.map()?;
        map.insert_key_value("b", "c")?;
        map.end()?;
        array.end()?;
        assert_eq!(
            encoder.encoded(),
            &[0x82, 0x61, 0x61, 0xa1, 0x61, 0x62, 0x61, 0x63]
        );
    }

    Ok(())
}

#[test]
fn rfc8949_encode_maps() -> Result<(), EncodeError> {
    let mut bytes = [0u8; 128];

    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.map()?.end()?;
        assert_eq!(encoder.encoded(), &[0xa0]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let mut map = encoder.map()?;
        map.insert_key_value(1u8, 2u8)?;
        map.insert_key_value(3u8, 4u8)?;
        map.end()?;
        assert_eq!(encoder.encoded(), &[0xa2, 0x01, 0x02, 0x03, 0x04]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let mut map = encoder.map()?;
        map.insert_key_value("a", 1u8)?;
        let mut array = map.key("b")?.array()?;
        array.insert(2u8)?;
        array.insert(3u8)?;
        array.end()?;
        map.end()?;
        assert_eq!(
            encoder.encoded(),
            &[0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x82, 0x02, 0x03]
        );
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        let mut map = encoder.map()?;
        map.insert_key_value("a", "A")?;
        map.insert_key_value("b", "B")?;
        map.insert_key_value("c", "C")?;
        map.insert_key_value("d", "D")?;
        map.insert_key_value("e", "E")?;
        map.end()?;
        let expected = [
            0xa5, 0x61, 0x61, 0x61, 0x41, 0x61, 0x62, 0x61, 0x42, 0x61, 0x63, 0x61, 0x43, 0x61,
            0x64, 0x61, 0x44, 0x61, 0x65, 0x61, 0x45,
        ];
        assert_eq!(encoder.encoded(), expected);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_tags() -> Result<(), EncodeError> {
    let mut bytes = [0u8; 128];

    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.tag(1)?.item(1_363_896_240u64)?;
        assert_eq!(encoder.encoded(), &[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.tag(23)?.item(&[1u8, 2u8, 3u8, 4u8][..])?;
        assert_eq!(encoder.encoded(), &[0xd7, 0x44, 0x01, 0x02, 0x03, 0x04]);
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(Tag {
            tag: 24,
            value: &b"dIETF"[..],
        })?;
        assert_eq!(
            encoder.encoded(),
            &[0xd8, 0x18, 0x45, 0x64, 0x49, 0x45, 0x54, 0x46]
        );
    }
    {
        let mut encoder = Encoder::<4>::new(&mut bytes);
        encoder.insert(Tag {
            tag: 32,
            value: "http://www.example.com",
        })?;
        let expected = [
            0xd8, 0x20, 0x76, 0x68, 0x74, 0x74, 0x70, 0x3a, 0x2f, 0x2f, 0x77, 0x77, 0x77, 0x2e,
            0x65, 0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x2e, 0x63, 0x6f, 0x6d,
        ];
        assert_eq!(encoder.encoded(), expected);
    }

    Ok(())
}

#[test]
#[cfg(feature = "float")]
fn rfc8949_encode_tags_float() -> Result<(), EncodeError> {
    let mut bytes = [0u8; 64];
    let mut encoder = Encoder::<4>::new(&mut bytes);
    encoder.tag(1)?.item(1_363_896_240.5f64)?;
    assert_eq!(
        encoder.encoded(),
        &[0xc1, 0xfb, 0x41, 0xd4, 0x52, 0xd9, 0xec, 0x20, 0x00, 0x00]
    );
    Ok(())
}

/*
#[test]
fn rfc8949_encode_tag() -> Result<(), CBORError> {
    println!("<==================== rfc8949_encode_empty_array ===================>");
    let mut buffer = [0u8; 64];
    {
        let expected: &[u8] = &[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(1, |buff| buff.insert(&1363896240)))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    {
        let expected: &[u8] = &[0xc1, 0xfb, 0x41, 0xd4, 0x52, 0xd9, 0xec, 0x20, 0x00, 0x00];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(1, |buff| buff.insert(&1363896240.5)))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    {
        let expected: &[u8] = &[0xd7, 0x44, 0x01, 0x02, 0x03, 0x04];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(23, |buff| {
            buff.insert(&[1u8, 2u8, 3u8, 4u8].as_slice())
        }))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    {
        let expected: &[u8] = &[0xd7, 0x44, 0x01, 0x02, 0x03, 0x04];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(23, |buff| {
            buff.insert(&[1u8, 2u8, 3u8, 4u8].as_slice())
        }))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    Ok(())
}

#[test]
#[cfg(not(feature = "float"))]
fn rfc8949_encode_tag() -> Result<(), CBORError> {
    println!("<==================== rfc8949_encode_empty_array ===================>");
    let mut buffer = [0u8; 64];
    {
        let expected: &[u8] = &[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(1, |buff| buff.insert(&1363896240)))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    {
        let expected: &[u8] = &[0xd7, 0x44, 0x01, 0x02, 0x03, 0x04];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(23, |buff| {
            buff.insert(&[1u8, 2u8, 3u8, 4u8].as_slice())
        }))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    {
        let expected: &[u8] = &[0xd7, 0x44, 0x01, 0x02, 0x03, 0x04];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(23, |buff| {
            buff.insert(&[1u8, 2u8, 3u8, 4u8].as_slice())
        }))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_empty_array() -> Result<(), CBORError> {
    println!("<==================== rfc8949_encode_empty_array ===================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0x80];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&array(|buff| Ok(buff)))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_array() -> Result<(), CBORError> {
    println!("<======================= rfc8949_encode_array ======================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0x83, 0x01, 0x02, 0x03];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&array(|buff| {
        buff.insert(&01u8)?.insert(&02u8)?.insert(&03u8)
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_nested_array() -> Result<(), CBORError> {
    println!("<=================== rfc8949_encode_nested_array ===================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0x83, 0x01, 0x82, 0x02, 0x03, 0x82, 0x04, 0x05];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&array(|buff| {
        buff.insert(&1u8)?
            .insert(&array(|buff| buff.insert(&2u8)?.insert(&3u8)))?
            .insert(&array(|buff| buff.insert(&4u8)?.insert(&5u8)))
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_array_long() -> Result<(), CBORError> {
    println!("<==================== rfc8949_encode_array_long ====================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[
        0x98, 0x19, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
        0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x18, 0x18, 0x19,
    ];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&array(|buff| {
        buff.insert(&1)?
            .insert(&2)?
            .insert(&3)?
            .insert(&4)?
            .insert(&5)?
            .insert(&6)?
            .insert(&7)?
            .insert(&8)?
            .insert(&9)?
            .insert(&10)?
            .insert(&11)?
            .insert(&12)?
            .insert(&13)?
            .insert(&14)?
            .insert(&15)?
            .insert(&16)?
            .insert(&17)?
            .insert(&18)?
            .insert(&19)?
            .insert(&20)?
            .insert(&21)?
            .insert(&22)?
            .insert(&23)?
            .insert(&24)?
            .insert(&25)
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_array_with_map() -> Result<(), CBORError> {
    println!("<================== rfc8949_encode_array_with_map ==================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0x82, 0x61, 0x61, 0xa1, 0x61, 0x62, 0x61, 0x63];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&array(|buff| {
        buff.insert(&"a")?
            .insert(&map(|buff| buff.insert_key_value(&"b", &"c")))
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_empty_map() -> Result<(), CBORError> {
    println!("<===================== rfc8949_encode_empty_map ====================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0xa0];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&map(|buff| Ok(buff)))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_map() -> Result<(), CBORError> {
    println!("<======================== rfc8949_encode_map =======================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0xa2, 0x01, 0x02, 0x03, 0x04];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&map(|buff| {
        buff.insert_key_value(&0x01u8, &0x02u8)?
            .insert_key_value(&0x03u8, &0x04u8)
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_map_with_str_keys() -> Result<(), CBORError> {
    println!("<================= rfc8949_encode_map_with_str_keys ================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x82, 0x02, 0x03];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&map(|buff| {
        buff.insert_key_value(&"a", &1)?
            .insert_key_value(&"b", &array(|buff| buff.insert(&2)?.insert(&3)))
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

#[test]
fn rfc8949_encode_map_long() -> Result<(), CBORError> {
    println!("<====================+ rfc8949_encode_map_long =====================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[
        0xa5, 0x61, 0x61, 0x61, 0x41, 0x61, 0x62, 0x61, 0x42, 0x61, 0x63, 0x61, 0x43, 0x61, 0x64,
        0x61, 0x44, 0x61, 0x65, 0x61, 0x45,
    ];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&map(|buff| {
        buff.insert_key_value(&"a", &"A")?
            .insert_key_value(&"b", &"B")?
            .insert_key_value(&"c", &"C")?
            .insert_key_value(&"d", &"D")?
            .insert_key_value(&"e", &"E")
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}

*/
