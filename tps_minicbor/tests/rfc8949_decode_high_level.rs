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
 * Test cases from RFC8949, for decoding using the high-level API (recommended)
 *
 * Test cases from RFC8949, Table 6.
 **************************************************************************************************/

extern crate tps_minicbor;

use tps_minicbor::decoder::*;
use tps_minicbor::error::CBORError;
use tps_minicbor::types::{CBOR};

#[test]
fn decode_combinators_basic() -> Result<(), CBORError> {
    println!("<======================= test_decode_combinators =====================>");
    {
        // Test 1: two parsers in sequence
        let it = SequenceBuffer::new(&[0x19, 0x03, 0xe8, 0x19, 0x03, 0xe9]).into_iter();
        let (it, r1) = is_uint()(it)?;
        let (_it, r2) = is_uint()(it)?;
        assert!(r1 == CBOR::UInt(1000) && r2 == CBOR::UInt(1001));
    }
    {
        // Test 2: cond
        let it = SequenceBuffer::new(&[0x19, 0x03, 0xe8]).into_iter();
        let (it, r1) = is_uint()(it)?;
        let (_next, v) = cond(true, is_eof())(it)?;
        assert!(r1 == CBOR::UInt(1000) && v == Some(CBOR::Eof));
    }
    {
        // Test 3: opt. First parse succeeds, second is skipped
        let it = SequenceBuffer::new(&[0x19, 0x03, 0xe8]).into_iter();
        let (it, r1) = opt(is_uint())(it)?;
        let (_it, r2) = opt(is_bool())(it)?;
        assert!(r1 == Some(CBOR::UInt(1000)) && r2 == None);
    }
    {
        // Test 4: with_pred, nesting of parsers, sequence of parsers
        let it = SequenceBuffer::new(&[0x19, 0x03, 0xe8, 0x19, 0x03, 0xe9]).into_iter();
        let (it, r1) = opt(with_pred(is_uint(), |v| {
            if let CBOR::UInt(value) = *v {
                value == 1000
            } else {
                false
            }
        }))(it)?;
        let (_it, r2) = is_uint()(it)?;
        assert!(r1 == Some(CBOR::UInt(1000)) && r2 == CBOR::UInt(1001))
    }
    {
        // Test 4: with_pred, nesting of parsers, sequence of parsers
        let it = SequenceBuffer::new(&[0x19, 0x03, 0xe8, 0x19, 0x03, 0xe9]).into_iter();
        let (it, r1) = opt(with_value(is_uint(), CBOR::UInt(1000)))(it)?;
        let (_it, r2) = is_uint()(it)?;
        assert!(r1 == Some(CBOR::UInt(1000)) && r2 == CBOR::UInt(1001))
    }
    Ok(())
}

#[test]
fn rfc8949_decode_zero() {
    let zero = [0x00].as_slice();
    {
        // Coerce to u8
        let mut result: u8 = 33;
        let _ = CBORDecoder::from_slice(&zero).value(decode_int(), &mut result);
        assert_eq!(result, 0);
    }
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&zero).value(decode_int(), &mut result);
        assert_eq!(result, 0);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&zero).value(decode_int(), &mut result);
        assert_eq!(result, 0);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&zero).value(decode_int(), &mut result);
        assert_eq!(result, 0);
    }
}

#[test]
fn rfc8949_decode_one() {
    let one = [0x01].as_slice();
    {
        // Coerce to u8
        let mut result: u8 = 33;
        let _ = CBORDecoder::from_slice(&one).value(decode_int(), &mut result);
        assert_eq!(result, 1);
    }
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&one).value(decode_int(), &mut result);
        assert_eq!(result, 1);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&one).value(decode_int(), &mut result);
        assert_eq!(result, 1);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&one).value(decode_int(), &mut result);
        assert_eq!(result, 1);
    }
}

#[test]
fn rfc8949_decode_ten() {
    let ten = [0x0a].as_slice();
    {
        // Coerce to u8
        let mut result: u8 = 33;
        let _ = CBORDecoder::from_slice(&ten).value(decode_int(), &mut result);
        assert_eq!(result, 10);
    }
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&ten).value(decode_int(), &mut result);
        assert_eq!(result, 10);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&ten).value(decode_int(), &mut result);
        assert_eq!(result, 10);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&ten).value(decode_int(), &mut result);
        assert_eq!(result, 10);
    }
}

#[test]
fn rfc8949_decode_twenty_three() {
    let twenty_three = [0x17].as_slice();
    {
        // Coerce to u8
        let mut result: u8 = 33;
        let _ = CBORDecoder::from_slice(&twenty_three).value(decode_int(), &mut result);
        assert_eq!(result, 23);
    }
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&twenty_three).value(decode_int(), &mut result);
        assert_eq!(result, 23);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&twenty_three).value(decode_int(), &mut result);
        assert_eq!(result, 23);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&twenty_three).value(decode_int(), &mut result);
        assert_eq!(result, 23);
    }
}

#[test]
fn rfc8949_decode_twenty_four() {
    let twenty_four = [0x18, 0x18].as_slice();
    {
        // Coerce to u8
        let mut result: u8 = 33;
        let _ = CBORDecoder::from_slice(&twenty_four).value(decode_int(), &mut result);
        assert_eq!(result, 24);
    }
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&twenty_four).value(decode_int(), &mut result);
        assert_eq!(result, 24);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&twenty_four).value(decode_int(), &mut result);
        assert_eq!(result, 24);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&twenty_four).value(decode_int(), &mut result);
        assert_eq!(result, 24);
    }
}

#[test]
fn rfc8949_decode_twenty_five() {
    let twenty_five = [0x18, 0x19].as_slice();
    {
        // Coerce to u8
        let mut result: u8 = 33;
        let _ = CBORDecoder::from_slice(&twenty_five).value(decode_int(), &mut result);
        assert_eq!(result, 25);
    }
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&twenty_five).value(decode_int(), &mut result);
        assert_eq!(result, 25);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&twenty_five).value(decode_int(), &mut result);
        assert_eq!(result, 25);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&twenty_five).value(decode_int(), &mut result);
        assert_eq!(result, 25);
    }
}

#[test]
fn rfc8949_decode_one_hundred() {
    let one_hundred = [0x18, 0x64].as_slice();
    {
        // Coerce to u8
        let mut result: u8 = 33;
        let _ = CBORDecoder::from_slice(&one_hundred).value(decode_int(), &mut result);
        assert_eq!(result, 100);
    }
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&one_hundred).value(decode_int(), &mut result);
        assert_eq!(result, 100);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&one_hundred).value(decode_int(), &mut result);
        assert_eq!(result, 100);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&one_hundred).value(decode_int(), &mut result);
        assert_eq!(result, 100);
    }
}

#[test]
fn rfc8949_decode_one_thousand() {
    let one_thousand = [0x19, 0x03, 0xe8].as_slice();
    // u8 would be out of range (detected at comepile time)
    {
        // Coerce to u16
        let mut result: u16 = 33;
        let _ = CBORDecoder::from_slice(&one_thousand).value(decode_int(), &mut result);
        assert_eq!(result, 1000);
    }
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&one_thousand).value(decode_int(), &mut result);
        assert_eq!(result, 1000);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&one_thousand).value(decode_int(), &mut result);
        assert_eq!(result, 1000);
    }
}

#[test]
fn rfc8949_decode_one_million() {
    let one_million = [0x1a, 0x00, 0x0f, 0x42, 0x40].as_slice();
    // u8 and u16 would be out of range (detected at comepile time)
    {
        // Coerce to u32
        let mut result: u32 = 33;
        let _ = CBORDecoder::from_slice(&one_million).value(decode_int(), &mut result);
        assert_eq!(result, 1_000_000);
    }
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&one_million).value(decode_int(), &mut result);
        assert_eq!(result, 1_000_000);
    }
}

#[test]
fn rfc8949_decode_one_trillion() {
    let one_trillion = [0x1b, 0x00, 0x00, 0x00, 0xe8, 0xd4, 0xa5, 0x10, 0x00].as_slice();
    // u8, u16 and u32 would be out of range (detected at comepile time)
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&one_trillion).value(decode_int(), &mut result);
        assert_eq!(result, 1_000_000_000_000);
    }
}

#[test]
fn rfc8949_decode_max_uint() {
    let max_int = [0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff].as_slice();
    // u8, u16 and u32 would be out of range (detected at comepile time)
    {
        // Coerce to u64
        let mut result: u64 = 33;
        let _ = CBORDecoder::from_slice(&max_int).value(decode_int(), &mut result);
        assert_eq!(result, u64::MAX);
    }
}

#[test]
fn rfc8949_decode_minus_one() {
    let minus_one = [0x20].as_slice();
    {
        // Coerce to i8
        let mut result: i8 = 33;
        let _ = CBORDecoder::from_slice(&minus_one).value(decode_nint(), &mut result);
        assert_eq!(result, -1);
    }
    {
        // Coerce to i16
        let mut result: i16 = 33;
        let _ = CBORDecoder::from_slice(&minus_one).value(decode_int(), &mut result);
        assert_eq!(result, -1);
    }
    {
        // Coerce to i32
        let mut result: i32 = 33;
        let _ = CBORDecoder::from_slice(&minus_one).value(decode_int(), &mut result);
        assert_eq!(result, -1);
    }
    {
        // Coerce to i64
        let mut result: i64 = 33;
        let _ = CBORDecoder::from_slice(&minus_one).value(decode_int(), &mut result);
        assert_eq!(result, -1);
    }
}

#[test]
fn rfc8949_decode_minus_ten() {
    let minus_ten = [0x29].as_slice();
    {
        // Coerce to i8
        let mut result: i8 = 33;
        let _ = CBORDecoder::from_slice(&minus_ten).value(decode_nint(), &mut result);
        assert_eq!(result, -10);
    }
    {
        // Coerce to i16
        let mut result: i16 = 33;
        let _ = CBORDecoder::from_slice(&minus_ten).value(decode_int(), &mut result);
        assert_eq!(result, -10);
    }
    {
        // Coerce to i32
        let mut result: i32 = 33;
        let _ = CBORDecoder::from_slice(&minus_ten).value(decode_int(), &mut result);
        assert_eq!(result, -10);
    }
    {
        // Coerce to i64
        let mut result: i64 = 33;
        let _ = CBORDecoder::from_slice(&minus_ten).value(decode_int(), &mut result);
        assert_eq!(result, -10);
    }
}

#[test]
fn rfc8949_decode_minus_one_hundred() {
    let minus_one_hundred = [0x38, 0x63].as_slice();
    {
        // Coerce to i8
        let mut result: i8 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_hundred).value(decode_nint(), &mut result);
        assert_eq!(result, -100);
    }
    {
        // Coerce to i16
        let mut result: i16 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_hundred).value(decode_int(), &mut result);
        assert_eq!(result, -100);
    }
    {
        // Coerce to i32
        let mut result: i32 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_hundred).value(decode_int(), &mut result);
        assert_eq!(result, -100);
    }
    {
        // Coerce to i64
        let mut result: i64 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_hundred).value(decode_int(), &mut result);
        assert_eq!(result, -100);
    }
}

#[test]
fn rfc8949_decode_minus_one_thousand() {
    let minus_one_thousand = [0x39, 0x03, 0xe7].as_slice();
    // Out of range for i8
    {
        // Coerce to i16
        let mut result: i16 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_thousand).value(decode_int(), &mut result);
        assert_eq!(result, -1000);
    }
    {
        // Coerce to i32
        let mut result: i32 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_thousand).value(decode_int(), &mut result);
        assert_eq!(result, -1000);
    }
    {
        // Coerce to i64
        let mut result: i64 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_thousand).value(decode_int(), &mut result);
        assert_eq!(result, -1000);
    }
}

#[test]
fn rfc8949_decode_min_nint() {
    let minus_one_thousand = [0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff].as_slice();
    // Out of range for i8, i16, i32
    {
        // Coerce to i128
        let mut result: i128 = 33;
        let _ = CBORDecoder::from_slice(&minus_one_thousand).value(decode_int(), &mut result);
        assert_eq!(result, -18446744073709551616);
    }
}

#[test]
fn rfc8949_decode_simple() {
    println!(
        "<=============================== rfc8949_decode_simple_hl ============================>"
    );
    {
        println!("<======================= Test with false =====================>");
        let mut result = true;
        let _ = CBORDecoder::from_slice(&[0xf4]).value(decode_bool(), &mut result);
        assert_eq!(result, false);
    }
    {
        println!("<======================= Test with true =====================>");
        let mut result = false;
        let _ = CBORDecoder::from_slice(&[0xf5]).value(decode_bool(), &mut result);
        assert_eq!(result, true);
    }
    {
        println!("<======================= Test with null =====================>");
        let mut result = CBOR::Undefined;
        let _ = CBORDecoder::from_slice(&[0xf6]).value(decode_null(), &mut result);
        assert_eq!(result, CBOR::Null);
    }
    {
        println!("<======================= Test with undefined =====================>");
        let mut result = CBOR::Null;
        let _ = CBORDecoder::from_slice(&[0xf7]).value(decode_undefined(), &mut result);
        assert_eq!(result, CBOR::Undefined);
    }
    {
        println!("<======================= Test with simple(16) =====================>");
        let mut result = 1;
        let _ = CBORDecoder::from_slice(&[0xf0]).value(decode_simple(), &mut result);
        assert_eq!(result, 16);
    }
    {
        println!("<======================= Test with simple(255) =====================>");
        let mut result = 1;
        let _ = CBORDecoder::from_slice(&[0xf8, 0xff]).value(decode_simple(), &mut result);
        assert_eq!(result, 255);
    }
    {
        println!(
            "<======================= Test with simple(19), simple(253) =====================>"
        );
        let mut r1 = 0;
        let mut r2 = 0;
        let _ = CBORDecoder::from_slice(&[0xf3, 0xf8, 0xfd])
            .value(decode_simple(), &mut r1)
            .unwrap()
            .value(decode_simple(), &mut r2);
        assert_eq!(r1, 19);
        assert_eq!(r2, 253);
    }
}

#[test]
fn rfc8949_decode_tstr() {
    println!(
        "<=============================== rfc8949_decode_tstr ==============================>"
    );
    {
        // This is not nice, but easiest way to get a mutable &str which we can reassign
        let mut result = "abc";
        let _ = CBORDecoder::from_slice(&[0x60]).value(decode_tstr(), &mut result);
        assert_eq!(result, "");
    }
    {
        let mut result = "abc";
        let _ = CBORDecoder::from_slice(&[0x61, 0x61]).value(decode_tstr(), &mut result);
        assert_eq!(result, "a");
    }
    {
        let mut result = "abc";
        let _ = CBORDecoder::from_slice(&[0x64, 0x49, 0x45, 0x54, 0x46])
            .value(decode_tstr(), &mut result);
        assert_eq!(result, "IETF");
    }
    {
        let mut result = "abc";
        let _ = CBORDecoder::from_slice(&[0x62, 0x22, 0x5c]).value(decode_tstr(), &mut result);
        assert_eq!(result, "\"\\");
    }
    {
        let mut result = "abc";
        let _ = CBORDecoder::from_slice(&[0x62, 0xc3, 0xbc]).value(decode_tstr(), &mut result);
        assert_eq!(result, "\u{00fc}");
    }
    {
        let mut result = "abc";
        let _ =
            CBORDecoder::from_slice(&[0x63, 0xe6, 0xb0, 0xb4]).value(decode_tstr(), &mut result);
        assert_eq!(result, "\u{6c34}");
    }
    // The below is illegal as Rust requires chars to be Unicode scalar values
    // which means that only values in 0x0..=0xd7ff and 0xe000..=0x10ffff are
    // legal. In short, RFC8949 has an example which is not actually legal UTF8
    // - see Unicode Standard, v12.0, Section 3.9, Table 3-7.
    // [0x64, 0xf0, 0x90, 0x85, 0x91],  "\u{00d800}\u{00dd51}"
}

#[test]
fn rfc8949_decode_bstr() {
    println!(
        "<=============================== rfc8949_decode_bstr ==============================>"
    );
    {
        // This is not nice, but easiest way to get a mutable &[u8] slide which we can reassign
        let mut result = [0x37].as_slice();
        let _ = CBORDecoder::from_slice(&[0x40]).value(decode_bstr(), &mut result);
        assert_eq!(result, &[]);
    }
    {
        // This is not nice, but easiest way to get a mutable &[u8] slide which we can reassign
        let mut result = [0x37].as_slice();
        let _ = CBORDecoder::from_slice(&[0x44, 0x01, 0x02, 0x03, 0x04])
            .value(decode_bstr(), &mut result);
        assert_eq!(result, &[1, 2, 3, 4]);
    }
}

#[test]
fn rfc8949_decode_array() -> Result<(), CBORError> {
    println!(
        "<=============================== rfc8949_decode_array =============================>"
    );
    {
        println!("<======================= Test with empty array =====================>");
        let _decoder = CBORDecoder::from_slice(&[0x80]).array(|ab| {
            assert_eq!(ab.len(), 0);
            Ok(())
        });
    }
    {
        println!("<======================= Test with [1,2,3] =====================>");
        let _ = CBORDecoder::from_slice(&[0x83, 0x01, 0x02, 0x03]).array(|ab| {
            assert_eq!(ab.item::<u8>(2)?, 3u8);
            assert_eq!(ab.item::<u8>(1)?, 2u8);
            assert_eq!(ab.item::<u8>(0)?, 1u8);
            Ok(())
        });
    }
    {
        println!("<======================= Test with [1,[2,3],[4,5]] =====================>");
        // NB: the array items have to be coerced to the correct type
        // - type inference doesn't work in this example
        let _d = CBORDecoder::from_slice(&[0x83, 0x01, 0x82, 0x02, 0x03, 0x82, 0x04, 0x05]).array(
            |ab| {
                assert_eq!(ab.item::<u8>(0)?, 1u8);
                let a1 = CBORDecoder::from_array(ab.item(1)?)?;
                let _ = a1.array(|ab1| {
                    assert_eq!(ab1.item::<u8>(1)?, 3u8);
                    assert_eq!(ab1.item::<u8>(0)?, 2u8);
                    Ok(())
                });
                let a2 = CBORDecoder::from_array(ab.item(2)?)?;
                let _ = a2.array(|ab2| {
                    assert_eq!(ab2.item::<u8>(1)?, 5u8);
                    assert_eq!(ab2.item::<u8>(0)?, 4u8);
                    Ok(())
                });
                Ok(())
            },
        );
    }
    {
        println!("<======================= Test with [1,2, ..., 25] =====================>");
        let _ = CBORDecoder::from_slice(&[
            0x98, 0x19, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
            0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x18, 0x18,
            0x19,
        ])
        .array(|ab| {
            for i in 0..=ab.len() {
                assert_eq!(ab.item::<u8>(i)?, i.clone() as u8 + 1);
            }
            Ok(())
        });
    }
    {
        println!(
            "<======================= Test with [\"a\", (\"b\": \"c\")]  =====================>"
        );
        let _ = CBORDecoder::from_slice(&[0x82, 0x61, 0x61, 0xa1, 0x61, 0x62, 0x61, 0x63])
            .array(|ab| {
                assert_eq!(ab.len(), 2);
                assert_eq!(ab.item::<&str>(0)?, "a");
                let _ = CBORDecoder::from_map(ab.item(2)?)?
                    .map(|mb| {
                    assert_eq!(mb.lookup::<&str, &str>("b")?, "c");
                    Ok(())
                });
                Ok(())
            });
    }
    Ok(())
}

#[test]
fn rfc8949_decode_map() -> Result<(), CBORError> {
    println!("<=============================== rfc8949_decode_map =============================>");
    {
        println!("<======================= Test with empty map =====================>");
        let _ = CBORDecoder::from_slice(&[0xa0])
            .map(|mb| {
                assert_eq!(mb.len(), 0);
                assert!(mb.is_empty());
                Ok(())
            });
    }
    {
        println!("<======================= Test with (1: 2, 3: 4)  =====================>");
        let _ = CBORDecoder::from_slice(&[0xa2, 0x01, 0x02, 0x03, 0x04])
            .map(|mb| {
                assert_eq!(mb.len(), 2);
                assert_eq!(mb.lookup::<u8, u8>(1)?, 2);
                assert_eq!(mb.lookup::<u8, u8>(3)?, 4);
                Ok(())
            });
    }
    {
        println!(
            "<======================= Test with (\"a\": 1, \"b\": [2,3])  =====================>"
        );
        let _ = CBORDecoder::from_slice(&[0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x82, 0x02, 0x03])
            .map(|mb| {
                assert_eq!(mb.len(), 2);
                assert_eq!(mb.lookup::<&str, u8>("a")?, 1);
                let _a = CBORDecoder::from_map(mb.lookup("b")?)?
                    .array(|ab| {
                        assert_eq!(ab.item::<u8>(0)?, 2);
                        assert_eq!(ab.item::<u8>(1)?, 3);
                        Ok(())
                    });
                Ok(())
            });
    }
    {
        println!(
            "<======================= Test with (\"a\": \"A\", \"b\": \"B\", \"c\": \"C\", \"d\": \"D\", \"e\": \"E\")  =====================>"
        );
        let _ = CBORDecoder::from_slice(&[
            0xa5, 0x61, 0x61, 0x61, 0x41, 0x61, 0x62, 0x61, 0x42, 0x61, 0x63, 0x61, 0x43, 0x61,
            0x64, 0x61, 0x44, 0x61, 0x65, 0x61, 0x45,
        ])
            .map(|mb| {
                assert_eq!(mb.len(), 5);
                assert_eq!(mb.lookup::<&str, &str>("a")?, "A");
                assert_eq!(mb.lookup::<&str, &str>("e")?, "E");
                assert_eq!(mb.lookup::<&str, &str>("b")?, "B");
                assert_eq!(mb.lookup::<&str, &str>("d")?, "D");
                assert_eq!(mb.lookup::<&str, &str>("c")?, "C");
                Ok(())
            });
    }
    Ok(())
}

#[test]
fn rfc8949_decode_tag() -> Result<(), CBORError> {
    println!("<=============================== rfc8949_decode_tag =============================>");
    {
        println!(
            "<======================= Test with 0(\"2013-03-21T20:04:00Z\") =====================>"
        );
        let _ = CBORDecoder::from_slice(&[
            0xc0, 0x74, 0x32, 0x30, 0x31, 0x33, 0x2d, 0x30, 0x33, 0x2d, 0x32, 0x31, 0x54, 0x32,
            0x30, 0x3a, 0x30, 0x34, 0x3a, 0x30, 0x30, 0x5a,
        ])
            .tag(|tb|{
                if tb.get_tag() == 0 {
                    let result = tb.item::<&str>()?;
                    assert_eq!(result, "2013-03-21T20:04:00Z");
                } else {
                    assert!(false);
                }
                Ok(())
            })?;
    }
    {
        println!("<======================= Test with 1(1363896240) =====================>");
        let _ = CBORDecoder::from_slice(&[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0])
            .tag(|tb| {
                if tb.get_tag() == 1 {
                    let result = tb.item::<u64>()?;
                    assert_eq!(result, 1363896240);
                } else {
                    assert!(false)
                }
                Ok(())
            })?;
    }
    {
        println!("<======================= Test with 23(h'01020304') =====================>");
        let _ = CBORDecoder::from_slice(&[0xd7, 0x44, 0x01, 0x02, 0x03, 0x04])
            .tag(|tb| {
                if tb.get_tag() == 23 {
                    let result = tb.item::<&[u8]>()?;
                    assert_eq!(result, &[1, 2, 3, 4]);
                } else {
                    assert!(false);
                }
                Ok(())
            })?;
    }
    {
        println!("<======================= Test with 24(h'6449455446') =====================>");
        let _ = CBORDecoder::from_slice(&[0xd8, 0x18, 0x45, 0x64, 0x49, 0x45, 0x54, 0x46])
            .tag(|tb| {
                if tb.get_tag() == 24 {
                    let result = tb.item::<&[u8]>()?;
                    assert_eq!(result, &[0x64, 0x49, 0x45, 0x54, 0x46]);
                } else {
                    assert!(false)
                }
                Ok(())
            })?;
    }
    {
        println!("<======================= Test with 32(\"http://www.example.com\") =====================>");
        let _ = CBORDecoder::from_slice(&[
            0xd8, 0x20, 0x76, 0x68, 0x74, 0x74, 0x70, 0x3a, 0x2f, 0x2f, 0x77, 0x77, 0x77, 0x2e,
            0x65, 0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x2e, 0x63, 0x6f, 0x6d,
        ])
            .tag(|tb| {
                if tb.get_tag() == 32 {
                    let result = tb.item::<&str>()?;
                    assert_eq!(result, "http://www.example.com")
                } else {
                    assert!(false);
                }
                Ok(())
            })?;
    }
    Ok(())
}
