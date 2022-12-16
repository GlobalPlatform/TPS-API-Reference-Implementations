/***************************************************************************************************
 * Copyright (c) 2020-2022 Qualcomm Innovation Center, Inc. All rights reserved.
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
 * Test cases from RFC8949, for encoding using the high-level API (recommended)
 *
 * Test cases from RFC8949, Table 6.
 **************************************************************************************************/

extern crate tps_minicbor;

use std::convert::TryFrom;

use tps_minicbor::decoder::*;
use tps_minicbor::encoder::*;
use tps_minicbor::error::CBORError;
use tps_minicbor::types::{array, map, tag, CBOR};

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
                Ok(assert_eq!(u32::try_from(cbor)?, 32u32))
            })?
            .decode_with(is_nint(), |cbor| {
                Ok(assert_eq!(i32::try_from(cbor)?, -(0xa5a5a5i32)))
            })?
            .decode_with(is_tstr(), |cbor| {
                Ok(assert_eq!(<&str>::try_from(cbor)?, "新年快乐"))
            })?
            .decode_with(is_bstr(), |cbor| {
                Ok(assert_eq!(<&[u8]>::try_from(cbor)?, val))
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
#[derive(Debug, Clone)]
struct TeeEat<'t> {
    pub nonce: &'t [u8],
    pub ueid: &'t [u8],
    pub oemid: u64,
    pub sec_level: u64,
    pub sec_boot: bool,
    pub debug_status: u64,
    pub hw_version: HwVersion<'t>,
}

#[derive(Debug, Clone)]
struct HwVersion<'t> {
    s: &'t str,
    v: u64,
}

#[test]
fn decode_tee_eat() -> Result<(), CBORError> {
    let mut input: &[u8] = &[
        167, 10, 72, 148, 143, 136, 96, 209, 58, 70, 62, 25, 1, 0, 80, 1, 152, 245, 10, 79, 246,
        192, 88, 97, 200, 134, 13, 19, 166, 56, 234, 25, 1, 2, 25, 250, 242, 25, 1, 5, 3, 25, 1, 6,
        245, 25, 1, 7, 3, 25, 1, 4, 130, 99, 51, 46, 49, 1,
    ];
    let mut vsn_string = String::new();
    let mut token = TeeEat {
        nonce: &[],
        ueid: &[],
        oemid: 0,
        sec_level: 0,
        sec_boot: false,
        debug_status: 0,
        hw_version: HwVersion { s: &"", v: 0},
    };

    let decoder = CBORDecoder::from_slice(&mut input);
    {
        let _d = decoder.map(|mb| {
            token.nonce = mb.lookup(10)?;
            token.ueid = mb.lookup(256)?;
            token.oemid = mb.lookup(258)?;
            token.sec_level = mb.lookup(261)?;
            token.sec_boot = mb.lookup(262)?;
            token.debug_status = mb.lookup(263)?;
            let ab: ArrayBuf = mb.lookup(260)?;
            // Any bstr or tstr needs to be deep copied somewhere that
            // definitely lives longer than the closure if we wish to use it
            // outside. Here we copy into a string which is inferred to outlive
            // the closure.
            vsn_string = String::from(ab.item::<&str>(0)?);
            token.hw_version.s = &vsn_string;
            token.hw_version.v = ab.item(1)?;
            Ok(())
        })?;
    }

    assert_eq!(
        token.nonce,
        &[0x94, 0x8f, 0x88, 0x60, 0xd1, 0x3a, 0x46, 0x3e]
    );
    assert_eq!(
        token.ueid,
        &[
            0x01, 0x98, 0xf5, 0x0a, 0x4f, 0xf6, 0xc0, 0x58, 0x61, 0xc8, 0x86, 0x0d, 0x13, 0xa6,
            0x38, 0xea
        ]
    );
    assert_eq!(token.oemid, 64242);
    assert_eq!(token.sec_level, 3);
    assert_eq!(token.sec_boot, true);
    assert_eq!(token.debug_status, 3);
    assert_eq!(token.hw_version.s, "3.1");
    assert_eq!(token.hw_version.v, 1);
    Ok(())
}

#[test]
fn foo() -> Result<(), CBORError> {
    // Encode-decode round trip test
    println!("<======================= encode_decode_cbor_ast =====================>");
    let mut bytes = [0u8; 128];

    {
        let mut encoded_cbor = CBORBuilder::new(&mut bytes);
        encoded_cbor
            .insert(&32u8)?
            .insert(&(-(0xa5a5a5i32)))?
            .insert(&"新年快乐")?
            .insert(&array(|buf| {
                buf.insert(&42u8)?
                    .insert(&CBOR::Undefined)
            }))?;

        let _decoder = CBORDecoder::new(encoded_cbor.build()?)
            .decode_with(is_uint(), |cbor| {
                Ok(assert_eq!(u32::try_from(cbor)?, 32u32))
            })?
            .decode_with(is_nint(), |cbor| {
                Ok(assert_eq!(i32::try_from(cbor)?, -(0xa5a5a5i32)))
            })?
            .decode_with(is_tstr(), |cbor| {
                Ok(assert_eq!(<&str>::try_from(cbor)?, "新年快乐"))
            })?
            .decode_with(is_array(), |cbor| {
                CBORDecoder::from_array(cbor)?
                    .decode_with(is_uint(), |cbor| Ok(assert_eq!(u8::try_from(cbor)?, 42)))?
                    .decode_with(is_undefined(), |cbor| Ok(assert_eq!(cbor, CBOR::Undefined)))?
                    .finalize()
            })?;
    }
    Ok(())
}
