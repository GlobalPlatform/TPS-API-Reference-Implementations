/***************************************************************************************************
 * Copyright (c) 2021, 2022 Qualcomm Innovation Center, Inc. All rights reserved.
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
 * rs_minicbor CBOR Array deserialser API
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
use crate::ast::CBOR;
use crate::decode::{DecodeBufIterator, DecodeBufIteratorSource};

#[cfg(feature = "trace")]
use func_trace::trace;
use crate::encode::{EncodeBuffer, EncodeContext, EncodeItem};
use crate::error::CBORError;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/***************************************************************************************************
 * Decoding Arrays
 **************************************************************************************************/
/// A buffer which contains a CBOR Array to be decoded. The buffer has lifetime `'buf`,
/// which must be longer than any borrow from the buffer itself. This is generally used to represent
/// a CBOR array with an exposed slice-like API.
///
/// This CBOR buffer implementation does not support indefinite length items.
#[derive(PartialEq, Debug, Copy, Clone)]
pub struct ArrayBuf<'buf> {
    bytes: &'buf [u8],
    n_items: usize,
}

impl<'buf> ArrayBuf<'buf> {
    /// Construct a new instance of `ArrayBuf` with all context initialized.
    #[cfg_attr(feature = "trace", trace)]
    pub fn new(init: &'buf [u8], n_items: usize) -> ArrayBuf<'buf> {
        ArrayBuf {
            bytes: init,
            n_items,
        }
    }

    /// Return the number of items in the `ArrayBuf`.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn len(&self) -> usize {
        self.n_items
    }

    /// Return `true` if `ArrayBuf` is empty.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.n_items == 0 && self.bytes.len() == 0
    }

    /// Return the `n`th value (zero indexed) in the `ArrayBuf`.
    ///
    /// Worst case performance of this function is O(n) in standalone form, but performance is
    /// likely to be O(n^2) if used for random access in general.
    #[cfg_attr(feature = "trace", trace)]
    pub fn index(&self, n: usize) -> Option<CBOR> {
        let mut count = 0;
        let mut it = self.into_iter();
        let mut item = it.next();
        while count < n && Option::is_some(&item) {
            item = it.next();
            count += 1;
        }
        item
    }
}

impl<'buf> IntoIterator for ArrayBuf<'buf> {
    type Item = CBOR<'buf>;
    type IntoIter = DecodeBufIterator<'buf>;

    /// Construct an Iterator adapter from a `DecodeBuf`.
    #[cfg_attr(feature = "trace", trace)]
    fn into_iter(self) -> Self::IntoIter {
        DecodeBufIterator {
            buf: self.bytes,
            index: 0,
            source: DecodeBufIteratorSource::Array,
        }
    }
}

/***************************************************************************************************
 * Encoding Arrays
 **************************************************************************************************/

/// A container structure for the closure used to manage encoding of CBOR arrays, and in particular
/// to ensure that the correct lifetime bounds are specified.
///
/// The user is able to encode members of the array within a closure, and the array length will
/// automatically be correctly constructed. Arbitrary nesting of arrays and maps is supported.
///
/// Users should never need to directly instantiate `Array`. Instead, see [`array`].
pub struct Array<F>
where F: for<'f, 'buf> Fn(&'f mut EncodeBuffer<'buf>) -> Result<&'f mut EncodeBuffer<'buf>, CBORError> {
    f: F
}

/// `Array` provides a constructor to contain the closure that constructs it
impl<F> Array<F> where
    F: for<'f, 'buf> Fn(&'f mut EncodeBuffer<'buf>) -> Result<&'f mut EncodeBuffer<'buf>, CBORError> {
    pub fn new(f: F) -> Array<F> { Array { f } }
}

/// The [`EncodeItem`] instance for `Array` performs the required manipulations to correctly
/// calculate the size of the array.
impl<F> EncodeItem for Array<F>
where F: for<'f, 'buf> Fn(&'f mut EncodeBuffer<'buf>) -> Result<&'f mut EncodeBuffer<'buf>, CBORError>
{
    fn encode<'f, 'buf>(&self, buf: &'f mut EncodeBuffer<'buf>) -> Result<&'f mut EncodeBuffer<'buf>, CBORError> {
        let mut array_ctx = EncodeContext::new();
        buf.array_start(&mut array_ctx)?;
        let _ = (self.f)(buf)?;
        buf.array_finalize(&array_ctx)?;
        Ok(buf)
    }
}

/// A convenience function for the user to create an instance of a CBOR array. The user provides a
/// closure which constructs the array contents.
///
/// ```
///# use tps_minicbor::encoder::CBORBuilder;
///# use tps_minicbor::error::CBORError;
///# use tps_minicbor::types::array;
///# fn main() -> Result<(), CBORError> {
///    let mut buffer = [0u8; 16];
///    let expected : &[u8] = &[132, 1, 2, 3, 4];
///
///    let mut encoder = CBORBuilder::new(&mut buffer);
///    encoder.insert(&array(|buff| {
///        buff.insert(&1)?.insert(&2)?.insert(&3)?.insert(&4)
///    }));
///    assert_eq!(encoder.encoded()?, expected);
///#    Ok(())
///# }
/// ```
pub fn array<F>(f: F) -> Array<F>
    where F: for<'f, 'buf> Fn(&'f mut EncodeBuffer<'buf>) -> Result<&'f mut EncodeBuffer<'buf>, CBORError>
{
    Array::new(f)
}
