/***************************************************************************************************
 * Copyright (c) 2020-2023 Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice (including the next
 * paragraph) shall be included in all copies or substantial portions of the
 * Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 **************************************************************************************************/
/***************************************************************************************************
 * Test cases from RFC8949, for decoding using the low level API (if highly memory constrained)
 *
 * Test cases from RFC8949, Table 6.
 **************************************************************************************************/

extern crate tps_minicbor;

use std::convert::TryFrom;

#[cfg(feature = "float")]
use half::f16;

use tps_minicbor::decoder::*;
use tps_minicbor::types::CBOR;

/***************************************************************************************************
 * Test cases for basic decoding using low-level interfaces
 **************************************************************************************************/
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
        let u1 = u8::try_from(item);
        let u2 = u16::try_from(item);
        let u3 = u32::try_from(item);
        let u4 = u64::try_from(item);
        let s1 = i8::try_from(item);
        let s2 = i16::try_from(item);
        let s3 = i32::try_from(item);
        let s4 = i64::try_from(item);
        let s5 = i128::try_from(item);

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
                    if let Ok(value) = i128::try_from(item) {
                        result.push(value)
                    }
                }
                CBOR::NInt(_) => {
                    if let Ok(value) = i128::try_from(item) {
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

/***************************************************************************************************
 * Test cases from RFC8949
 **************************************************************************************************/
// Verify unsigned integer item decode using iterator API and <type>::try_from(&item) for all cases
#[test]
fn rfc8949_decode_uint() {
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
fn rfc8949_decode_sint() {
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
fn rfc8949_decode_bs() {
    println!("<======================= rfc8949_decode_bs =====================>");

    // Test1: h''
    println!("<======================= Test with h'' =====================>");
    decode_bstr(&[0x40], &[]);
    // Test2: h'01020304'
    println!("<======================= Test with h'01020304' =====================>");
    decode_bstr(&[0x44, 0x01, 0x02, 0x03, 0x04], &[01u8, 02u8, 03u8, 04u8]);
}

#[test]
fn rfc8949_decode_str() {
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
fn rfc8949_decode_arr() {
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
                        check_int_result!(u8::try_from(i), Some(1))
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
#[cfg(feature = "float")]
fn rfc8949_decode_float() {
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
