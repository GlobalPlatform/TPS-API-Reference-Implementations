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
 * Test cases for tps_minicbor: bugfixes and adversarial cases
 **************************************************************************************************/
extern crate tps_minicbor;

use tps_minicbor::encoder::*;
use tps_minicbor::error::CBORError;
use tps_minicbor::types::{array};

/*
 * This test case checks that the first entry in an array can be another array
 */
#[test]
fn encode_nested_array_first_item_bugfix() -> Result<(), CBORError> {
    println!("<=================== rfc8949_encode_nested_array ===================>");
    let mut buffer = [0u8; 64];
    let expected: &[u8] = &[0x82, 0x82, 0x01, 0x02, 0x82, 0x03, 0x04];

    let mut encoder = CBORBuilder::new(&mut buffer);
    let _ = encoder.insert(&array(|buff| {
        buff.insert(&array(|buff| buff.insert(&1u8)?.insert(&2u8)))?
            .insert(&array(|buff| buff.insert(&3u8)?.insert(&4u8)))
    }))?;
    assert_eq!(encoder.encoded()?, expected);
    Ok(())
}
