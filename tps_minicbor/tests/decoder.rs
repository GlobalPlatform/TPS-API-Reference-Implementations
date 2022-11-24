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
 * Test cases from RFC7049, for decoding
 *
 * Test cases from RFC7049, Table 4.
 **************************************************************************************************/

extern crate rs_minicbor;

use core::convert::TryFrom;
use half::f16;

use rs_minicbor::decoder::*;
use rs_minicbor::error::CBORError;
use rs_minicbor::types::CBOR;

macro_rules! check_int_result {
    ($result:expr, $expected:expr) => {
        if let Ok(value) = $result {
            if let Some(expected_value) = $expected {
                // Have value, expect value
                println!("value: {:?}, expected: {:?}", value, expected_value);
                assert_eq!(value as i128, expected_value)
            } else {
                // No value, expect value
                println!("value {:?}, expected {:?}", value, $expected);
                assert!(false)
            }
        } else {
            if $expected.is_some() {
                // Have value, not expected
                assert!(false)
            } else {
                // No value, none expected
                assert!(true)
            }
        }
    };
}

fn decode_single(buf: &[u8]) -> Option<CBOR> {
    let b = SequenceBuffer::new(buf);
    let mut it = b.into_iter();
    it.next()
}

// Check that integer values are decoded into the expected values by all of the parsers, and that
// over/underflows are properly detected.
fn decode_integer(buf: &[u8], expected_values: &[Option<i128>; 9]) {
    if let Some(item) = decode_single(buf) {
        let u1 = u8::try_from(&item);
        let u2 = u16::try_from(&item);
        let u3 = u32::try_from(&item);
        let u4 = u64::try_from(&item);
        let s1 = i8::try_from(&item);
        let s2 = i16::try_from(&item);
        let s3 = i32::try_from(&item);
        let s4 = i64::try_from(&item);
        let s5 = i128::try_from(&item);

        check_int_result!(u1, expected_values[0]);
        check_int_result!(u2, expected_values[1]);
        check_int_result!(u3, expected_values[2]);
        check_int_result!(u4, expected_values[3]);
        check_int_result!(s1, expected_values[4]);
        check_int_result!(s2, expected_values[5]);
        check_int_result!(s3, expected_values[6]);
        check_int_result!(s4, expected_values[7]);
        check_int_result!(s5, expected_values[8]);
    } else {
        assert!(false)
    }
}

// Check that bstr values are decoded into the expected values
fn decode_bstr(buf: &[u8], expect: &[u8]) {
    if let Some(item) = decode_single(buf) {
        if let CBOR::Bstr(slice) = item {
            let result: &[u8] = slice.into();
            assert_eq!(expect, result);
        } else {
            assert!(false);
        }
    }
}

// Check that str values are decoded into the expected values
fn decode_str(buf: &[u8], expect: &str) {
    if let Some(item) = decode_single(buf) {
        if let CBOR::Tstr(s) = item {
            let result: &str = s.into();
            assert_eq!(expect, result);
        } else {
            assert!(false);
        }
    }
}

// Check the content of an array containing integer values is as expected. We convert everything
// to i128 so that we can test over all integer values
fn decode_integer_array(buf: &[u8], expect: &[i128]) {
    if let Some(CBOR::Array(ab)) = decode_single(&buf) {
        let mut result: Vec<i128> = Vec::new();
        for item in ab.into_iter() {
            match item {
                CBOR::UInt(_) => {
                    if let Ok(value) = i128::try_from(&item) {
                        result.push(value)
                    }
                }
                CBOR::NInt(_) => {
                    if let Ok(value) = i128::try_from(&item) {
                        result.push(value)
                    }
                }
                _ => (),
            }
        }
        assert_eq!(&result, expect);
    } else {
        // If we don't have an array when we expect one, it's obviously a failure
        assert!(false)
    }
}

// Verify unsigned integer item decode using iterator API and <type>::try_from(&item) for all cases
#[test]
fn rfc8949_decode_uint_manual() {
    println!("<======================= rfc8949_decode_uint =====================>");

    // Test1: 0
    println!("<======================= Test with v = 0 =====================>");
    decode_integer(
        &[0x00],
        &[
            Some(0), // u8
            Some(0), // u16
            Some(0), // u32
            Some(0), // u64
            Some(0), // i8
            Some(0), // i16
            Some(0), // i32
            Some(0), // i64
            Some(0), // i128
        ],
    );
    // Test2: 1
    println!("<======================= Test with v = 1 =====================>");
    decode_integer(
        &[0x01],
        &[
            Some(1), // u8
            Some(1), // u16
            Some(1), // u32
            Some(1), // u64
            Some(1), // i8
            Some(1), // i16
            Some(1), // i32
            Some(1), // i64
            Some(1), // i128
        ],
    );
    // Test3: 10
    println!("<======================= Test with v = 10 =====================>");
    decode_integer(
        &[0x0a],
        &[
            Some(10), // u8
            Some(10), // u16
            Some(10), // u32
            Some(10), // u64
            Some(10), // i8
            Some(10), // i16
            Some(10), // i32
            Some(10), // i64
            Some(10), // i128
        ],
    );
    // Test4: 23
    println!("<======================= Test with v = 23 =====================>");
    decode_integer(
        &[0x17],
        &[
            Some(23), // u8
            Some(23), // u16
            Some(23), // u32
            Some(23), // u64
            Some(23), // i8
            Some(23), // i16
            Some(23), // i32
            Some(23), // i64
            Some(23), // i128
        ],
    );
    // Test5: 24
    println!("<======================= Test with v = 24 =====================>");
    decode_integer(
        &[0x18, 0x18],
        &[
            Some(24), // u8
            Some(24), // u16
            Some(24), // u32
            Some(24), // u64
            Some(24), // i8
            Some(24), // i16
            Some(24), // i32
            Some(24), // i64
            Some(24), // i128
        ],
    );
    // Test6: 25
    println!("<======================= Test with v = 25 =====================>");
    decode_integer(
        &[0x18, 0x19],
        &[
            Some(25), // u8
            Some(25), // u16
            Some(25), // u32
            Some(25), // u64
            Some(25), // i8
            Some(25), // i16
            Some(25), // i32
            Some(25), // i64
            Some(25), // i128
        ],
    );
    // Test7: 100
    println!("<======================= Test with v = 100 =====================>");
    decode_integer(
        &[0x18, 0x64],
        &[
            Some(100), // u8
            Some(100), // u16
            Some(100), // u32
            Some(100), // u64
            Some(100), // i8
            Some(100), // i16
            Some(100), // i32
            Some(100), // i64
            Some(100), // i128
        ],
    );
    // Test8: 1000
    println!("<======================= Test with v = 1000 =====================>");
    decode_integer(
        &[0x19, 0x03, 0xe8],
        &[
            None,       // u8
            Some(1000), // u16
            Some(1000), // u32
            Some(1000), // u64
            None,       // i8
            Some(1000), // i16
            Some(1000), // i32
            Some(1000), // i64
            Some(1000), // i128
        ],
    );
    // Test9: 1000000
    println!("<======================= Test with v = 1000000 =====================>");
    decode_integer(
        &[0x1a, 0x00, 0x0f, 0x42, 0x40],
        &[
            None,          // u8
            None,          // u16
            Some(1000000), // u32
            Some(1000000), // u64
            None,          // i8
            None,          // i16
            Some(1000000), // i32
            Some(1000000), // i64
            Some(1000000), // i128
        ],
    );
    // Test10: 1000000000000
    println!("<======================= Test with v = 1000000000000 =====================>");
    decode_integer(
        &[0x1b, 0x00, 0x00, 0x00, 0xe8, 0xd4, 0xa5, 0x10, 0x00],
        &[
            None,                    // u8
            None,                    // u16
            None,                    // u32
            Some(1_000_000_000_000), // u64
            None,                    // i8
            None,                    // i16
            None,                    // i32
            Some(1_000_000_000_000), // i64
            Some(1_000_000_000_000), // i128
        ],
    );
    // Test11: 18446744073709551615 (u64::MAX)
    println!("<======================= Test with v = 18446744073709551615 =====================>");
    decode_integer(
        &[0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        &[
            None,                             // u8
            None,                             // u16
            None,                             // u32
            Some(18_446_744_073_709_551_615), // u64
            None,                             // i8
            None,                             // i16
            None,                             // i32
            None,                             // i64
            Some(18_446_744_073_709_551_615), // i128
        ],
    );
}

// Verify signed integer item decode using iterator API and <type>::try_from(&item) for all cases
#[test]
fn rfc8949_decode_sint_manual() {
    println!("<======================= rfc8949_decode_sint =====================>");

    // Test1: -1
    println!("<======================= Test with v = -1 =====================>");
    decode_integer(
        &[0x20],
        &[
            None,     // u8
            None,     // u16
            None,     // u32
            None,     // u64
            Some(-1), // i8
            Some(-1), // i16
            Some(-1), // i32
            Some(-1), // i64
            Some(-1), // i128
        ],
    );
    // Test2: -10
    println!("<======================= Test with v = -10 =====================>");
    decode_integer(
        &[0x29],
        &[
            None,      // u8
            None,      // u16
            None,      // u32
            None,      // u64
            Some(-10), // i8
            Some(-10), // i16
            Some(-10), // i32
            Some(-10), // i64
            Some(-10), // i128
        ],
    );
    // Test3: -100
    println!("<======================= Test with v = -100 =====================>");
    decode_integer(
        &[0x38, 0x63],
        &[
            None,       // u8
            None,       // u16
            None,       // u32
            None,       // u64
            Some(-100), // i8
            Some(-100), // i16
            Some(-100), // i32
            Some(-100), // i64
            Some(-100), // i128
        ],
    );
    // Test4: -1000
    println!("<======================= Test with v = -1000 =====================>");
    decode_integer(
        &[0x39, 0x03, 0xe7],
        &[
            None,        // u8
            None,        // u16
            None,        // u32
            None,        // u64
            None,        // i8
            Some(-1000), // i16
            Some(-1000), // i32
            Some(-1000), // i64
            Some(-1000), // i128
        ],
    );
    // Test5: -18446744073709551616 (i64::MIN)
    println!("<======================= Test with v = -18446744073709551616 =====================>");
    decode_integer(
        &[0x3b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        &[
            None,                             // u8
            None,                             // u16
            None,                             // u32
            None,                             // u64
            None,                             // i8
            None,                             // i16
            None,                             // i32
            Some(-9_223_372_036_854_775_808), // i64
            Some(-9_223_372_036_854_775_808), // i128
        ],
    );
}

#[test]
fn rfc8949_decode_bs_manual() {
    println!("<======================= rfc8949_decode_bs =====================>");

    // Test1: h''
    println!("<======================= Test with h'' =====================>");
    decode_bstr(&[0x40], &[]);
    // Test2: h'01020304'
    println!("<======================= Test with h'01020304' =====================>");
    decode_bstr(&[0x44, 0x01, 0x02, 0x03, 0x04], &[01u8, 02u8, 03u8, 04u8]);
}

#[test]
fn rfc8949_decode_str_manual() {
    println!("<======================= rfc8949_decode_str =====================>");

    // Test1: ""
    println!("<======================= Test with \"\" =====================>");
    decode_str(&[0x60], "");

    // Test2: "a"
    println!("<======================= Test with \"a\" =====================>");
    decode_str(&[0x61, 0x61], "a");

    // Test3: "IETF"
    println!("<======================= Test with \"IETF\" =====================>");
    decode_str(&[0x64, 0x49, 0x45, 0x54, 0x46], "IETF");

    // Test4: "\"\\"
    decode_str(&[0x62, 0x22, 0x5c], "\"\\");

    // Test5: "\u00fc"
    decode_str(&[0x62, 0xc3, 0xbc], "\u{00fc}");

    // Test6: "\u6c34"
    decode_str(&[0x63, 0xe6, 0xb0, 0xb4], "\u{6c34}");

    // The below is illegal as Rust requires chars to be Unicode scalar values
    // which means that only values in [0x0, 0xd7ff] and [0xe000, 0x10ffff] are
    // legal. In short, RFC7049 has an example which is not actually legal UTF8
    // - see Unicode Standard, v12.0, Section 3.9, Table 3-7.
    //decode_ok!([0x64, 0xf0, 0x90, 0x85, 0x91], CBOR::UTF8Str("\u{d800}\u{dd51}".to_string()));
}

#[test]
fn rfc8949_decode_arr_manual() {
    println!("<======================= rfc8949_decode_arr =====================>");
    // Test1: []
    println!("\n\nrfc8949_decode_arr - Test 1 - []");
    decode_integer_array(&[0x80], &[]);

    // Test2: [1, 2, 3]
    println!("\n\nrfc8949_decode_arr - Test 1 - [1,2,3]");
    decode_integer_array(&[0x83, 0x01, 0x02, 0x03], &[1, 2, 3]);

    // Test3: [1, [2, 3], [4, 5]] - tests nested array
    {
        println!("\n\nrfc8949_decode_arr - Test 3 - [1,[2,3],[4,5]");
        let items = SequenceBuffer::new(&[0x83, 0x01, 0x82, 0x02, 0x03, 0x82, 0x04, 0x05]);
        let mut iter = items.into_iter();

        // Iter.nth(0) (same as iter.next()) should return an Array
        if let Some(CBOR::Array(ab)) = iter.nth(0) {
            for i in 0..ab.len() {
                println!("==> i = {}", i);
                // Yes, it's normally an anti-pattern, but here we need the positional information
                match (ab.index(i), i) {
                    // Expect first item to be UInt(1)
                    (Some(i @ CBOR::UInt(_)), 0) => {
                        check_int_result!(u8::try_from(&i), Some(1))
                    }
                    (Some(CBOR::Array(ab1)), 1) => {
                        println!("ab1: {:?}", ab1);
                        // foo
                        assert!(true)
                    }
                    (Some(CBOR::Array(ab2)), 2) => {
                        println!("ab2: {:?}", ab2);
                        // bar
                        assert!(true)
                    }
                    e => {
                        println!("e: {:?}", e);
                        assert!(false)
                    }
                }
            }
        }
    }

    // Test4: [1, 2, 3, .., 24, 25] - tests array with different integer lengths
    println!("\n\nrfc8949_decode_arr - Test 1 - [1,2,3, .., 24, 25]");
    decode_integer_array(
        &[
            0x98, 0x19, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
            0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x18, 0x18,
            0x19,
        ],
        &[
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
        ],
    );
}

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
fn rfc8949_decode_uint_with_combinator() -> Result<(), CBORError> {
    fn decode_uint(seq: &[u8], v: u64) -> Result<(), CBORError> {
        println!(
            "<======================= Test with v = {} =====================>",
            v
        );
        let it = SequenceBuffer::new(seq).into_iter();
        let (_it, r1) = with_value(is_uint(), CBOR::UInt(v))(it)?;
        println!("r1 = {:?}, v = {}", r1, v);
        assert_eq!(r1, CBOR::UInt(v));
        Ok(())
    }
    println!("<======================= rfc8949_decode_uint_with_combinator =====================>");
    decode_uint(&[0x00], 0)?;
    decode_uint(&[0x01], 1)?;
    decode_uint(&[0x0a], 10)?;
    decode_uint(&[0x17], 23)?;
    decode_uint(&[0x18, 0x18], 24)?;
    decode_uint(&[0x18, 0x19], 25)?;
    decode_uint(&[0x18, 0x64], 100)?;
    decode_uint(&[0x19, 0x03, 0xe8], 1000)?;
    decode_uint(&[0x1a, 0x00, 0x0f, 0x42, 0x40], 1000000)?;
    decode_uint(
        &[0x1b, 0x00, 0x00, 0x00, 0xe8, 0xd4, 0xa5, 0x10, 0x00],
        1000000000000,
    )?;
    decode_uint(
        &[0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        18446744073709551615,
    )?;
    Ok(())
}

#[test]
fn rfc8949_decode_nint_with_combinator() -> Result<(), CBORError> {
    fn decode_nint(seq: &[u8], v: i128) -> Result<(), CBORError> {
        println!(
            "<======================= Test with v = {} =====================>",
            v
        );
        let nint_repr: u64 = (-v - 1) as u64;
        let it = SequenceBuffer::new(seq).into_iter();
        let (_it, r1) = with_value(is_nint(), CBOR::NInt(nint_repr))(it)?;
        println!("r1 = {:?}, v = {}, nint_repr = {}", r1, v, nint_repr);
        assert_eq!(r1, CBOR::NInt(nint_repr));
        Ok(())
    }
    println!("<======================= rfc8949_decode_nint_with_combinator =====================>");
    decode_nint(&[0x20], -1)?;
    decode_nint(&[0x29], -10)?;
    decode_nint(&[0x38, 0x63], -100)?;
    decode_nint(&[0x39, 0x03, 0xe7], -1000)?;
    decode_nint(
        &[0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        -18446744073709551616,
    )?;
    Ok(())
}

#[test]
fn rfc8949_decode_simple_with_combinator() -> Result<(), CBORError> {
    println!(
        "<======================= rfc8949_decode_simple_with_combinator =====================>"
    );
    {
        println!("<======================= Test with false =====================>");
        let it = SequenceBuffer::new(&[0xf4]).into_iter();
        let (_, r) = is_false()(it)?;
        assert_eq!(r, CBOR::False);
    }
    {
        println!("<======================= Test with true =====================>");
        let it = SequenceBuffer::new(&[0xf5]).into_iter();
        let (_, r) = is_true()(it)?;
        assert_eq!(r, CBOR::True);
    }
    {
        println!("<======================= Test with null =====================>");
        let it = SequenceBuffer::new(&[0xf6]).into_iter();
        let (_, r) = is_null()(it)?;
        assert_eq!(r, CBOR::Null);
    }
    {
        println!("<======================= Test with undefined =====================>");
        let it = SequenceBuffer::new(&[0xf7]).into_iter();
        let (_, r) = is_undefined()(it)?;
        assert_eq!(r, CBOR::Undefined);
    }
    {
        println!("<======================= Test with simple(16) =====================>");
        let it = SequenceBuffer::new(&[0xf0]).into_iter();
        let (_, r) = with_value(is_simple(), CBOR::Simple(16))(it)?;
        assert_eq!(r, CBOR::Simple(16));
    }
    {
        println!("<======================= Test with simple(255) =====================>");
        let it = SequenceBuffer::new(&[0xf8, 0xff]).into_iter();
        let (_, r) = with_value(is_simple(), CBOR::Simple(255))(it)?;
        assert_eq!(r, CBOR::Simple(255));
    }
    {
        println!(
            "<======================= Test with simple(19), simple(253) =====================>"
        );
        let it = SequenceBuffer::new(&[0xf3, 0xf8, 0xfd]).into_iter();
        let (it, r1) = with_value(is_simple(), CBOR::Simple(19))(it)?;
        let (_, r2) = with_value(is_simple(), CBOR::Simple(253))(it)?;
        assert_eq!(r1, CBOR::Simple(19));
        assert_eq!(r2, CBOR::Simple(253));
    }
    Ok(())
}

#[test]
fn rfc8949_decode_tstr_with_combinator() -> Result<(), CBORError> {
    fn decode_str(b: &[u8], s: &str) -> Result<(), CBORError> {
        println!(
            "<======================= Test with {:?} =====================>",
            s
        );
        let it = SequenceBuffer::new(b).into_iter();
        let (_, r) = with_value(is_tstr(), CBOR::Tstr(s))(it)?;
        assert_eq!(r, CBOR::Tstr(s));
        Ok(())
    }
    println!("<======================= rfc8949_decode_tstr_with_combinator =====================>");
    decode_str(&[0x60], "")?;
    decode_str(&[0x61, 0x61], "a")?;
    decode_str(&[0x64, 0x49, 0x45, 0x54, 0x46], "IETF")?;
    decode_str(&[0x62, 0x22, 0x5c], "\"\\")?;
    decode_str(&[0x62, 0xc3, 0xbc], "\u{00fc}")?;
    decode_str(&[0x63, 0xe6, 0xb0, 0xb4], "\u{6c34}")?;

    // The below is illegal as Rust requires chars to be Unicode scalar values
    // which means that only values in [0x0, 0xd7ff] and [0xe000, 0x10ffff] are
    // legal. In short, RFC7049 has an example which is not actually legal UTF8
    // - see Unicode Standard, v12.0, Section 3.9, Table 3-7.
    // decode_str(false, &[0x64, 0xf0, 0x90, 0x85, 0x91],  "\u{00d800}\u{00dd51}")?;
    Ok(())
}

#[test]
fn rfc8949_decode_bstr_with_combinator() -> Result<(), CBORError> {
    fn decode_bstr(b: &[u8], bs: &[u8]) -> Result<(), CBORError> {
        println!(
            "<======================= Test with {:?} =====================>",
            bs
        );
        let it = SequenceBuffer::new(b).into_iter();
        let (_, r) = with_value(is_bstr(), CBOR::Bstr(bs))(it)?;
        assert_eq!(r, CBOR::Bstr(bs));
        Ok(())
    }
    println!("<======================= rfc8949_decode_str_with_combinator =====================>");
    decode_bstr(&[0x40], &[])?;
    decode_bstr(&[0x44, 0x01, 0x02, 0x03, 0x04], &[1, 2, 3, 4])?;
    Ok(())
}

#[test]
fn rfc8949_decode_array_with_combinator() -> Result<(), CBORError> {
    println!(
        "<======================= rfc8949_decode_array_with_combinator =====================>"
    );
    {
        println!("<======================= Test with empty array =====================>");
        let it = SequenceBuffer::new(&[0x80]).into_iter();
        if let (_, CBOR::Array(ab)) = is_array()(it)? {
            assert_eq!(ab.len(), 0);
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with [1,2,3] =====================>");
        let it_seq = SequenceBuffer::new(&[0x83, 0x01, 0x02, 0x03]).into_iter();
        if let (_, CBOR::Array(ab)) = is_array()(it_seq)? {
            assert_eq!(ab.len(), 3);
            let it_array = ab.into_iter();
            let (it_array, r1) = is_uint()(it_array)?;
            let (it_array, r2) = is_uint()(it_array)?;
            let (_it_array, r3) = is_uint()(it_array)?;
            assert_eq!(r1, CBOR::UInt(1));
            assert_eq!(r2, CBOR::UInt(2));
            assert_eq!(r3, CBOR::UInt(3));
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with [1,[2,3],[4,5]] =====================>");
        let it_seq =
            SequenceBuffer::new(&[0x83, 0x01, 0x82, 0x02, 0x03, 0x82, 0x04, 0x05]).into_iter();
        if let (_, CBOR::Array(ab)) = is_array()(it_seq)? {
            assert_eq!(ab.len(), 3);
            let it_array = ab.into_iter();
            let (it_array, r1) = is_uint()(it_array)?;
            let (it_array, r2) = is_array()(it_array)?;
            let (_it_array, r3) = is_array()(it_array)?;
            assert_eq!(r1, CBOR::UInt(1));
            if let CBOR::Array(ab2) = r2 {
                let it_ab2 = ab2.into_iter();
                let (it_ab2, r2a) = is_uint()(it_ab2)?;
                let (_it_ab2, r2b) = is_uint()(it_ab2)?;
                assert!(r2a == CBOR::UInt(2) && r2b == CBOR::UInt(3));
            } else {
                assert!(false);
            }
            if let CBOR::Array(ab3) = r3 {
                let it_ab3 = ab3.into_iter();
                let (it_ab3, r3a) = is_uint()(it_ab3)?;
                let (_it_ab3, r3b) = is_uint()(it_ab3)?;
                assert!(r3a == CBOR::UInt(4) && r3b == CBOR::UInt(5))
            } else {
                assert!(false);
            }
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with [1,2, ..., 25] =====================>");
        let it_seq = SequenceBuffer::new(&[
            0x98, 0x19, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
            0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x18, 0x18,
            0x19,
        ])
        .into_iter();
        if let (_, CBOR::Array(ab)) = is_array()(it_seq)? {
            assert_eq!(ab.len(), 25);
            // Note slightly nasty handling of the iterator here...
            let mut it_array = ab.into_iter();
            for i in 0..ab.len() {
                // The iterator is assigned here in a let, so dropped on next loop iteration...
                let (it_array_tmp, v) = is_uint()(it_array)?;
                // So we assign it to the mutable copy here, and the value is moved.
                it_array = it_array_tmp;

                if let CBOR::UInt(num) = v {
                    assert_eq!((i + 1) as u64, num);
                } else {
                    assert!(false);
                }
            }
        } else {
            assert!(false);
        }
    }
    {
        println!(
            "<======================= Test with [\"a\", (\"b\": \"c\")]  =====================>"
        );
        let it = SequenceBuffer::new(&[0x82, 0x61, 0x61, 0xa1, 0x61, 0x62, 0x61, 0x63]).into_iter();
        if let (_, CBOR::Array(ab)) = is_array()(it)? {
            assert_eq!(ab.len(), 2);
            assert_eq!(ab.index(0), Some(CBOR::Tstr("a")));
            if let Some(CBOR::Map(mb2)) = ab.index(1) {
                assert_eq!(mb2.len(), 1);
                assert_eq!(
                    mb2.get_key_value(&CBOR::Tstr("b")),
                    Some((CBOR::Tstr("b"), CBOR::Tstr("c")))
                );
            } else {
                assert!(false)
            }
        } else {
            assert!(false);
        }
    }

    Ok(())
}

#[test]
fn rfc8949_decode_map_with_combinator() -> Result<(), CBORError> {
    println!("<======================= rfc8949_decode_map_with_combinator =====================>");
    {
        println!("<======================= Test with empty map =====================>");
        let it = SequenceBuffer::new(&[0xa0]).into_iter();
        if let (_, CBOR::Map(mb)) = is_map()(it)? {
            assert_eq!(mb.len(), 0);
            assert!(mb.is_empty());
            assert_eq!(mb.contains_key(&CBOR::UInt(1)), false);
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with (1: 2, 3: 4)  =====================>");
        let it = SequenceBuffer::new(&[0xa2, 0x01, 0x02, 0x03, 0x04]).into_iter();
        if let (_, CBOR::Map(mb)) = is_map()(it)? {
            assert_eq!(mb.len(), 2);
            assert_eq!(mb.contains_key(&CBOR::UInt(1)), true);
            assert_eq!(mb.get(&CBOR::UInt(1)), Some(CBOR::UInt(2)));
            assert_eq!(
                mb.get_key_value(&CBOR::UInt(3)),
                Some((CBOR::UInt(3), CBOR::UInt(4)))
            );
        } else {
            assert!(false);
        }
    }
    {
        println!(
            "<======================= Test with (\"a\": 1, \"b\": [2,3])  =====================>"
        );
        let it = SequenceBuffer::new(&[0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x82, 0x02, 0x03])
            .into_iter();
        if let (_, CBOR::Map(mb)) = is_map()(it)? {
            assert_eq!(mb.len(), 2);
            assert_eq!(mb.contains_key(&CBOR::Tstr("a")), true);
            assert_eq!(mb.get(&CBOR::Tstr("a")), Some(CBOR::UInt(1)));
            if let Some(CBOR::Array(ab2)) = mb.get(&CBOR::Tstr("b")) {
                assert_eq!(ab2.len(), 2);
                assert_eq!(ab2.index(0), Some(CBOR::UInt(2)));
                assert_eq!(ab2.index(1), Some(CBOR::UInt(3)));
            } else {
                assert!(false)
            }
        } else {
            assert!(false);
        }
    }
    {
        println!(
            "<======================= Test with (\"a\": \"A\", \"b\": \"B\", \"c\": \"C\", \"d\": \"D\", \"e\": \"E\")  =====================>"
        );
        let it = SequenceBuffer::new(&[
            0xa5, 0x61, 0x61, 0x61, 0x41, 0x61, 0x62, 0x61, 0x42, 0x61, 0x63, 0x61, 0x43, 0x61,
            0x64, 0x61, 0x44, 0x61, 0x65, 0x61, 0x45,
        ])
        .into_iter();
        if let (_, CBOR::Map(mb)) = is_map()(it)? {
            assert_eq!(mb.len(), 5);
            assert_eq!(mb.get(&CBOR::Tstr("a")), Some(CBOR::Tstr("A")));
            assert_eq!(mb.get(&CBOR::Tstr("e")), Some(CBOR::Tstr("E")));
            assert_eq!(mb.get(&CBOR::Tstr("c")), Some(CBOR::Tstr("C")));
            assert_eq!(mb.get(&CBOR::Tstr("d")), Some(CBOR::Tstr("D")));
            assert_eq!(mb.get(&CBOR::Tstr("b")), Some(CBOR::Tstr("B")));
        } else {
            assert!(false);
        }
    }
    Ok(())
}

#[test]
fn rfc8949_decode_tag_with_combinator() -> Result<(), CBORError> {
    println!("<======================= rfc8949_decode_tag_with_combinator =====================>");
    {
        println!(
            "<======================= Test with 0(\"2013-03-21T20:04:00Z\") =====================>"
        );
        let it = SequenceBuffer::new(&[
            0xc0, 0x74, 0x32, 0x30, 0x31, 0x33, 0x2d, 0x30, 0x33, 0x2d, 0x32, 0x31, 0x54, 0x32,
            0x30, 0x3a, 0x30, 0x34, 0x3a, 0x30, 0x30, 0x5a,
        ])
        .into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(0)(it)? {
            let t_it = tb.into_iter();
            if let (_, CBOR::Tstr(s)) = is_tstr()(t_it)? {
                assert_eq!(s, "2013-03-21T20:04:00Z");
            } else {
                assert!(false);
            }
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with 1(1363896240) =====================>");
        let it = SequenceBuffer::new(&[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0]).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(1)(it)? {
            let t_it = tb.into_iter();
            if let (_, CBOR::UInt(v)) = is_uint()(t_it)? {
                assert_eq!(v, 1363896240);
            } else {
                assert!(false);
            }
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with 23(h'01020304') =====================>");
        let it = SequenceBuffer::new(&[0xd7, 0x44, 0x01, 0x02, 0x03, 0x04]).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(23)(it)? {
            let t_it = tb.into_iter();
            if let (_, CBOR::Bstr(bs)) = is_bstr()(t_it)? {
                assert_eq!(bs, &[1, 2, 3, 4]);
            } else {
                assert!(false);
            }
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with 24(h'6449455446') =====================>");
        let it = SequenceBuffer::new(&[0xd8, 0x18, 0x45, 0x64, 0x49, 0x45, 0x54, 0x46]).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(24)(it)? {
            let t_it = tb.into_iter();
            if let (_, CBOR::Bstr(bs)) = is_bstr()(t_it)? {
                assert_eq!(bs, &[0x64, 0x49, 0x45, 0x54, 0x46]);
            } else {
                assert!(false);
            }
        } else {
            assert!(false);
        }
    }
    {
        println!("<======================= Test with 32(\"http://www.example.com\") =====================>");
        let it = SequenceBuffer::new(&[
            0xd8, 0x20, 0x76, 0x68, 0x74, 0x74, 0x70, 0x3a, 0x2f, 0x2f, 0x77, 0x77, 0x77, 0x2e,
            0x65, 0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x2e, 0x63, 0x6f, 0x6d,
        ])
        .into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(32)(it)? {
            let t_it = tb.into_iter();
            if let (_, CBOR::Tstr(ts)) = is_tstr()(t_it)? {
                assert_eq!(ts, "http://www.example.com");
            } else {
                assert!(false);
            }
        } else {
            assert!(false);
        }
    }
    Ok(())
}

#[test]
fn rfc8949_decode_float_manual() {
    println!("<======================= rfc8949_decode_float =====================>");

    // Tests for f16 encodings
    for (bs, expect) in [
        ([0xf9, 0x00, 0x00], 0.0),
        ([0xf9, 0x80, 0x00], -0.0),
        ([0xf9, 0x3c, 0x00], 1.0),
        ([0xf9, 0x3e, 0x00], 1.5),
        ([0xf9, 0x7b, 0xff], 65504.0),
        ([0xf9, 0x00, 0x01], 5.960464477539063e-8),
        ([0xf9, 0x04, 0x00], 0.00006103515625),
        ([0xf9, 0xc4, 0x00], -4.0),
    ]
    .iter()
    {
        println!(
            "<======================= Test with {} (f16) =====================>",
            expect
        );
        if let Some(CBOR::Float16(v)) = decode_single(bs) {
            assert_eq!(v, f16::from_f32(*expect));
        } else {
            assert!(false)
        }
    }

    println!("<======================= Test +Infinity (f16) =====================>");
    if let Some(CBOR::Float16(v)) = decode_single(&[0xf9, 0x7c, 0x00]) {
        assert!(v.is_infinite() && v.is_sign_positive());
    } else {
        assert!(false)
    }
    println!("<======================= Test NaN (f16) =====================>");
    if let Some(CBOR::Float16(v)) = decode_single(&[0xf9, 0x7e, 0x00]) {
        assert!(v.is_nan());
    } else {
        assert!(false)
    }
    println!("<======================= Test -Infinity (f16) =====================>");
    if let Some(CBOR::Float16(v)) = decode_single(&[0xf9, 0xfc, 0x00]) {
        assert!(v.is_infinite() && v.is_sign_negative());
    } else {
        assert!(false)
    }

    // Tests with f32 encodings
    for (bs, expect) in [
        ([0xfa, 0x47, 0xc3, 0x50, 0x00], 100000.0),
        ([0xfa, 0x7f, 0x7f, 0xff, 0xff], 3.4028234663852886e+38f32),
    ]
    .iter()
    {
        println!(
            "<======================= Test with {} (f32) =====================>",
            expect
        );
        if let Some(CBOR::Float32(v)) = decode_single(bs) {
            assert_eq!(v, *expect);
        } else {
            assert!(false)
        }
    }

    println!("<======================= Test +Infinity (f32) =====================>");
    if let Some(CBOR::Float32(v)) = decode_single(&[0xfa, 0x7f, 0x80, 0x00, 0x00]) {
        assert!(v.is_infinite() && v.is_sign_positive());
    } else {
        assert!(false)
    }
    println!("<======================= Test NaN (f32) =====================>");
    if let Some(CBOR::Float32(v)) = decode_single(&[0xfa, 0x7f, 0xc0, 0x00, 0x00]) {
        assert!(v.is_nan());
    } else {
        assert!(false)
    }
    println!("<======================= Test -Infinity (f32) =====================>");
    if let Some(CBOR::Float32(v)) = decode_single(&[0xfa, 0xff, 0x80, 0x00, 0x00]) {
        assert!(v.is_infinite() && v.is_sign_negative());
    } else {
        assert!(false)
    }

    // Tests with f64 encodings
    for (bs, expect) in [
        ([0xfb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a], 1.1),
        (
            [0xfb, 0x7e, 0x37, 0xe4, 0x3c, 0x88, 0x00, 0x75, 0x9c],
            1.0e+300,
        ),
        ([0xfb, 0xc0, 0x10, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66], -4.1),
    ]
    .iter()
    {
        println!(
            "<======================= Test with {} (f64) =====================>",
            expect
        );
        if let Some(CBOR::Float64(v)) = decode_single(bs) {
            assert_eq!(v, *expect);
        } else {
            assert!(false)
        }
    }

    println!("<======================= Test +Infinity (f64) =====================>");
    if let Some(CBOR::Float64(v)) =
        decode_single(&[0xfb, 0x7f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
    {
        assert!(v.is_infinite() && v.is_sign_positive());
    } else {
        assert!(false)
    }
    println!("<======================= Test NaN (f64) =====================>");
    if let Some(CBOR::Float64(v)) =
        decode_single(&[0xfb, 0x7f, 0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
    {
        assert!(v.is_nan());
    } else {
        assert!(false)
    }
    println!("<======================= Test -Infinity (f64) =====================>");
    if let Some(CBOR::Float64(v)) =
        decode_single(&[0xfb, 0xff, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
    {
        assert!(v.is_infinite() && v.is_sign_negative());
    } else {
        assert!(false)
    }
}
