/***************************************************************************************************
 * Copyright (c) 2021 Jeremy O'Donoghue. All rights reserved.
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

extern crate rs_minicbor;

use rs_minicbor::decoder::*;
use rs_minicbor::types::*;

use std::convert::TryFrom;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Just about simplest ever: the below decodes as 1000
    let b = SequenceBuffer::new(&[0x19, 0x03, 0xe8]);
    let mut it = b.into_iter();
    let item = it.next();

    if let Some(item) = item {
        let v1 = u16::try_from(&item); // should succeed
        let v2 = u32::try_from(&item); // should succeed
        let v3 = i32::try_from(&item); // should succeed
        let v4 = u8::try_from(&item); // should fail
        println!("v1 = {:?}, v2 = {:?}, v3 = {:?}, v4 = {:?}", v1, v2, v3, v4);
    }

    // Testing how to use combinators
    let it = SequenceBuffer::new(&[0x19, 0x03, 0xe8]).into_iter();
    let (it, r1) = is_uint()(it)?;
    let (_next, v) = cond(true, &is_eof())(it)?;
    //let (_next, v) = cond!(false, is_eof, it)?;
    //let (_next, v) = is_eof()(it)?;
    println!("r1 = {:?}, e = {:?}", r1, v);

    let it = SequenceBuffer::new(&[0x19, 0x03, 0xe8, 0x19, 0x03, 0xe9]).into_iter();
    let (it, r1) = opt(apply(&is_uint(), |v| {
        println!("Value: {:?}", v);
    }))(it)?;
    let (_it, r2) = is_uint()(it)?;
    assert!(r1 == Some(CBOR::UInt(1000)) && r2 == CBOR::UInt(1001));
    Ok(())
}
