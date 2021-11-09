/***************************************************************************************************
 * Copyright (c) 2020, 2021 Jeremy O'Donoghue. All rights reserved.
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

use rs_minicbor::decoder::*;
use rs_minicbor::encoder::*;
use rs_minicbor::error::CBORError;
use rs_minicbor::types::CBOR;

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
        &(1000000u32).encode(&mut buf)?;
        &(1000001u32).encode(&mut buf)?;
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
fn encode_decode_cbor_ast() -> Result<(), CBORError> {
    // Encode-decode round trip test
    println!("<======================= rfc8949_encode_decode_cbor_ast =====================>");
    let mut bytes = [0u8; 128];

    {
        let val: &[u8] = &[1, 2, 3, 4];

        // values
        let uval = CBOR::UInt(32);
        let nval = CBOR::NInt(0xa5a5a5);
        let sval = CBOR::Tstr("新年快乐");
        let bval = CBOR::Bstr(&val);
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

        let mut array_ctx = EncodeContext::new();
        let mut map_ctx = EncodeContext::new();
        let mut encoded_cbor = CBOREncoder::new(&mut bytes);
        encoded_cbor
            .insert(&uval)?
            .insert(&nval)?
            .insert(&sval)?
            .insert(&bval)?
            .insert(&s1)?
            .insert(&s2)?
            .insert(&s3)?
            .tag_next_item(37)?
            .insert(&tval)?
            .array_start(&mut array_ctx)?
            .insert(&aval1)?
            .insert(&aval2)?
            .insert(&aval3)?
            .array_finalize(&array_ctx)?
            .map_start(&mut map_ctx)?
            .insert_key_value(&mkey1, &mval1)?
            .insert_key_value(&mkey2, &mval2)?
            .insert_key_value(&mkey3, &mval3)?
            .map_finalize(&map_ctx)?;

        let _decoder = CBORDecoder::new(encoded_cbor.build()?)
            .decode_with(is_uint(), |cbor| Ok(assert_eq!(cbor, uval)))?
            .decode_with(is_nint(), |cbor| Ok(assert_eq!(cbor, nval)))?
            .decode_with(is_tstr(), |cbor| Ok(assert_eq!(cbor, sval)))?
            .decode_with(is_bstr(), |cbor| Ok(assert_eq!(cbor, bval)))?
            .decode_with(is_simple(), |cbor| Ok(assert_eq!(cbor, s1)))?
            .decode_with(is_simple(), |cbor| Ok(assert_eq!(cbor, s2)))?
            .decode_with(is_false(), |cbor| Ok(assert_eq!(cbor, s3)))?
            .decode_with(is_tag_with_value(37), |cbor| {
                CBORDecoder::from_tag(cbor)?
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
    {
        let date_time_str = "2013-03-21T20:04:00Z";
        match chrono::DateTime::parse_from_rfc3339(date_time_str) {
            Ok(date_time_val) => {
                let date_time = CBOR::DateTime(date_time_val);
                let epoch = CBOR::Epoch(1626198094);

                let mut encoded_cbor = CBOREncoder::new(&mut bytes);
                encoded_cbor.insert(&date_time)?.insert(&epoch)?;

                let _decoder = CBORDecoder::new(encoded_cbor.build()?)
                    .decode_with(is_date_time(), |cbor| Ok(assert_eq!(cbor, date_time)))?
                    .decode_with(is_epoch(), |cbor| Ok(assert_eq!(cbor, epoch)))?;
            }
            Err(_) => assert!(false),
        }
    }
    Ok(())
}
