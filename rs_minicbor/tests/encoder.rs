/***************************************************************************************************
 * Copyright (c) 2020-2022 Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all copies or
 * substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 **************************************************************************************************/
/***************************************************************************************************
 * Test cases from RFC8949, for encoding
 *
 * Test cases from RFC7049, Table 4.
 **************************************************************************************************/

extern crate rs_minicbor;

use half::f16;
use rs_minicbor::decoder::*;
use rs_minicbor::encoder::*;
use rs_minicbor::error::CBORError;
use rs_minicbor::types::{array, map, tag, CBOR};

#[test]
fn rfc8949_encode_int() -> Result<(), CBORError> {
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
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // 32 bit encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // 16 bit encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // 8 bit encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // Concatenations
    {
        println!("<======================= Concatenate 2 x u32 =====================>");
        let mut buf = EncodeBuffer::new(&mut bytes);
        let _ = &(1000000u32).encode(&mut buf)?;
        let _ = &(1000001u32).encode(&mut buf)?;
        assert_eq!(
            buf.encoded()?,
            &[0x1a, 0x00, 0x0f, 0x42, 0x40, 0x1a, 0x00, 0x0f, 0x42, 0x41]
        );
    }

    // i64 encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // i32 encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // i16 encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // i8 encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    // i128 encodings
    for (val, expect) in [
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
            *val
        );
        let mut buf = EncodeBuffer::new(&mut bytes);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, *expect);
    }

    Ok(())
}

#[test]
fn rfc8949_encode_tstr() -> Result<(), CBORError> {
    println!("<======================= rfc8949_encode_tstr =====================>");
    let mut bytes = [0u8; 32];

    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        "".encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x60]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        "a".encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x61, 0x61]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        "IETF".encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x64, 0x49, 0x45, 0x54, 0x46]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        "\"\\".encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x62, 0x22, 0x5c]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        "\u{00fc}".encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x62, 0xc3, 0xbc]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        "\u{6c34}".encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x63, 0xe6, 0xb0, 0xb4]);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_bstr() -> Result<(), CBORError> {
    println!("<======================= rfc8949_encode_tstr =====================>");
    let mut bytes = [0u8; 32];

    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val: &[u8] = &[];
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x40]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val: &[u8] = &[1, 2, 3, 4];
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0x44, 0x01, 0x02, 0x03, 0x04]);
    }
    {
        // Test using the insert API
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val: &[u8] = &[1, 2, 3, 4];

        buf.insert(&val)?;
        assert_eq!(buf.encoded()?, &[0x44, 0x01, 0x02, 0x03, 0x04]);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_simple() -> Result<(), CBORError> {
    println!("<======================= rfc8949_encode_simple =====================>");
    let mut bytes = [0u8; 32];

    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(CBOR::False);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf4]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(CBOR::True);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf5]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(CBOR::Null);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf6]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(CBOR::Undefined);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf7]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(CBOR::Simple(16));
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf0]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(CBOR::Simple(255));
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf8, 0xff]);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_float() -> Result<(), CBORError> {
    println!("<======================= rfc8949_encode_float ======================>");
    let mut bytes = [0u8; 32];

    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(0.0);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x00, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(-0.0);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x80, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(1.0);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x3c, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(1.1);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xfb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(1.5);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x3e, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(65504.0);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x7b, 0xff]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(100000.0);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xfa, 0x47, 0xc3, 0x50, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(3.4028234663852886e+38);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xfa, 0x7f, 0x7f, 0xff, 0xff]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(1.0e+300);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xfb, 0x7e, 0x37, 0xe4, 0x3c, 0x88, 0x00, 0x75, 0x9c]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(5.960464477539063e-8);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x00, 0x01]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(0.00006103515625);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x04, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(-4.0);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0xc4, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(-4.1);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xfb, 0xc0, 0x10, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(f16::INFINITY);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x7c, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(f16::NAN);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0x7e, 0x00]);
    }
    {
        let mut buf = EncodeBuffer::new(&mut bytes);
        let val = &(f16::NEG_INFINITY);
        val.encode(&mut buf)?;
        assert_eq!(buf.encoded()?, &[0xf9, 0xfc, 0x00]);
    }
    Ok(())
}

#[test]
fn rfc8949_encode_tag() -> Result<(), CBORError> {
    println!("<==================== rfc8949_encode_empty_array ===================>");
    let mut buffer = [0u8; 64];
    {
        let expected: &[u8] = &[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(1, |buff| {
            buff.insert(&1363896240)
        }))?;
        assert_eq!(encoder.encoded()?, expected);
    }
    {
        let expected: &[u8] = &[0xc1, 0xfb, 0x41, 0xd4, 0x52, 0xd9, 0xec, 0x20, 0x00, 0x00];

        let mut encoder = CBORBuilder::new(&mut buffer);
        let _ = encoder.insert(&tag(1, |buff| {
            buff.insert(&1363896240.5)
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

#[test]
fn encode_decode_cbor_ast() -> Result<(), CBORError> {
    // Encode-decode round trip test
    println!("<======================= encode_decode_cbor_ast =====================>");
    let mut bytes = [0u8; 128];

    {
        let val: &[u8] = &[1, 2, 3, 4];

        // values
        let s1 = CBOR::Simple(17);
        let s2 = CBOR::Simple(234);
        let s3 = CBOR::False;
        let tval = CBOR::UInt(0x5a5a5a5a5a5a);
        let aval1 = CBOR::Tstr("usine à gaz");
        let aval2 = CBOR::UInt(42);
        let aval3 = CBOR::Undefined;
        let mkey1 = CBOR::UInt(1);
        let mval1 = CBOR::UInt(1023);
        let mkey2 = CBOR::UInt(2);
        let mval2 = CBOR::UInt(1025);
        let mkey3 = CBOR::NInt(1);
        let mval3 = CBOR::NInt(1024);
        let mut tag_val: u64 = 0;

        let mut encoded_cbor = CBORBuilder::new(&mut bytes);
        encoded_cbor
            .insert(&32u8)?
            .insert(&(-(0xa5a5a5i32)))?
            .insert(&"新年快乐")?
            .insert(&val)?
            .insert(&s1)?
            .insert(&s2)?
            .insert(&s3)?
            .insert(&tag(37, |buf| buf.insert(&tval)))?
            .insert(&array(|buf| {
                buf.insert(&"usine à gaz")?
                    .insert(&42u8)?
                    .insert(&CBOR::Undefined)
            }))?
            .insert(&map(|buf| {
                buf.insert_key_value(&1u8, &1023u32)?
                    .insert_key_value(&2u8, &1025u32)?
                    .insert_key_value(&(-1i8), &(-1024i32))
            }))?;

        let _decoder = CBORDecoder::new(encoded_cbor.build()?)
            .decode_with(is_uint(), |cbor| {
                Ok(assert_eq!(cbor.try_into_u32()?, 32u32))
            })?
            .decode_with(is_nint(), |cbor| {
                Ok(assert_eq!(cbor.try_into_i32()?, -(0xa5a5a5i32)))
            })?
            .decode_with(is_tstr(), |cbor| {
                Ok(assert_eq!(cbor.try_into_str()?, "新年快乐"))
            })?
            .decode_with(is_bstr(), |cbor| {
                Ok(assert_eq!(cbor.try_into_u8slice()?, val))
            })?
            .decode_with(is_simple(), |cbor| Ok(assert_eq!(cbor, s1)))?
            .decode_with(is_simple(), |cbor| Ok(assert_eq!(cbor, s2)))?
            .decode_with(is_false(), |cbor| Ok(assert_eq!(cbor, s3)))?
            .decode_with(is_tag(), |cbor| {
                CBORDecoder::from_tag(cbor, &mut tag_val)?
                    .decode_with(is_uint(), |cbor| Ok(assert_eq!(cbor, tval)))?
                    .finalize()
            })?
            .decode_with(is_array(), |cbor| {
                CBORDecoder::from_array(cbor)?
                    .decode_with(is_tstr(), |cbor| Ok(assert_eq!(cbor, aval1)))?
                    .decode_with(is_uint(), |cbor| Ok(assert_eq!(cbor, aval2)))?
                    .decode_with(is_undefined(), |cbor| Ok(assert_eq!(cbor, aval3)))?
                    .finalize()
            })?
            .decode_with(is_map(), |cbor| {
                if let CBOR::Map(mb) = cbor {
                    // In the test cases we do not care about the results of these - the assert_eq!()
                    // tell us all we need.
                    let _ = mb.get(&mkey1).map_or_else(
                        || Err(CBORError::KeyNotPresent),
                        |v| Ok(assert_eq!(v, mval1)),
                    );
                    let _ = mb.get(&mkey2).map_or_else(
                        || Err(CBORError::KeyNotPresent),
                        |v| Ok(assert_eq!(v, mval2)),
                    );
                    let _ = mb.get(&mkey3).map_or_else(
                        || Err(CBORError::KeyNotPresent),
                        |v| Ok(assert_eq!(v, mval3)),
                    );
                    Ok(())
                } else {
                    Err(CBORError::ExpectedType("Map"))
                }
            })?;
    }
    Ok(())
}

// / This is an example of a token produced by a HW block            /
// / purpose-built for attestation.  Only the nonce claim changes    /
// / from one attestation to the next as the rest  either come       /
// / directly from the hardware or from one-time-programmable memory /
// / (e.g. a fuse). 47 bytes encoded in CBOR (8 byte nonce, 16 byte  /
// / UEID). /
//
// {
// / nonce /           10: h'948f8860d13a463e',
// / UEID /           256: h'0198f50a4ff6c05861c8860d13a638ea',
// / OEMID /          258: 64242, / Private Enterprise Number /
// / security-level / 261: 3, / hardware level security /
// / secure-boot /    262: true,
// / debug-status /   263: 3, / disabled-permanently /
// / HW version /     260: [ "3.1", 1 ] / Type is multipartnumeric /
// }
#[test]
fn encode_tee_eat() -> Result<(), CBORError> {
    // Encode-decode round trip test
    println!("<========================== encode_tee_eat =========================>");
    let mut bytes = [0u8; 1024];
    let expected: &[u8] = &[
        167, 10, 72, 148, 143, 136, 96, 209, 58, 70, 62, 25, 1, 0, 80, 1, 152, 245, 10, 79, 246,
        192, 88, 97, 200, 134, 13, 19, 166, 56, 234, 25, 1, 2, 25, 250, 242, 25, 1, 5, 3, 25, 1, 6,
        245, 25, 1, 7, 3, 25, 1, 4, 130, 99, 51, 46, 49, 1,
    ];
    let nonce: &[u8] = &[0x94, 0x8f, 0x88, 0x60, 0xd1, 0x3a, 0x46, 0x3e];
    let ueid: &[u8] = &[
        0x01, 0x98, 0xf5, 0x0a, 0x4f, 0xf6, 0xc0, 0x58, 0x61, 0xc8, 0x86, 0x0d, 0x13, 0xa6, 0x38,
        0xea,
    ];

    let mut encoded_cbor = CBORBuilder::new(&mut bytes);
    encoded_cbor.insert(&map(|buff| {
        buff.insert_key_value(&10, &nonce)?
            .insert_key_value(&256, &ueid)?
            .insert_key_value(&258, &64242)?
            .insert_key_value(&261, &3)?
            .insert_key_value(&262, &true)?
            .insert_key_value(&263, &3)?
            .insert_key_value(&260, &array(|buf| buf.insert(&"3.1")?.insert(&1)))
    }))?;

    assert_eq!(encoded_cbor.encoded()?, expected);
    Ok(())
}

#[test]
fn decode_tee_eat() -> Result<(), CBORError> {
    let mut input: &[u8] = &[
        167, 10, 72, 148, 143, 136, 96, 209, 58, 70, 62, 25, 1, 0, 80, 1, 152, 245, 10, 79, 246,
        192, 88, 97, 200, 134, 13, 19, 166, 56, 234, 25, 1, 2, 25, 250, 242, 25, 1, 5, 3, 25, 1, 6,
        245, 25, 1, 7, 3, 25, 1, 4, 130, 99, 51, 46, 49, 1,
    ];
    let mut nonce = None;
    let mut ueid = None;
    let mut oemid = None;
    let mut sec_level = None;
    let mut sec_boot = None;
    let mut debug_state = None;
    let mut hw_ver_int = None;

    let mut decoder = CBORDecoder::from_slice(&mut input);
    decoder.decode_with(is_map(), |cbor| {
        if let CBOR::Map(map) = cbor {
            nonce = map.get_int(10);
            ueid = map.get_int(256);
            oemid = map.get_int(258);
            sec_level = map.get_int(261);
            sec_boot = map.get_int(262);
            debug_state = map.get_int(263);
            if let Some(CBOR::Array(ab)) = map.get_int(260) {
                hw_ver_int = match ab.index(1) {
                    None => None,
                    Some(CBOR::UInt(vi)) => Some(vi.clone()),
                    _ => None
                };
            }
        }
        Ok(())
    })?;

    assert_eq!(oemid, Some(CBOR::UInt(64242)));
    assert_eq!(sec_level, Some(CBOR::UInt(3)));
    assert_eq!(sec_boot, Some(CBOR::True));
    assert_eq!(debug_state, Some(CBOR::UInt(3)));
    assert_eq!(hw_ver_int, Some(1));
    Ok(())
}

#[test]
fn encode_array_array() -> Result<(), CBORError> {

    println!("<=================== rfc8949_encode_nested_array ===================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[130, 130, 1, 2, 130, 3, 4];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&array(|buff| {
        buff.insert(&array(|buff| buff.insert(&1u8)?.insert(&2u8)))?
            .insert(&array(|buff| buff.insert(&3u8)?.insert(&4u8)))
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}
