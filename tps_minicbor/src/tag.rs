/***************************************************************************************************
 * Copyright (c) 2021-2022 Qualcomm Innovation Center, Inc. All rights reserved.
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
 * rs_minicbor CBOR Tag deserialser API
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
use core::convert::TryFrom;
use crate::ast::CBOR;
use crate::decode::{DecodeBufIterator, DecodeBufIteratorSource};

use crate::encode::{EncodeBuffer, EncodeContext, EncodeItem};
use crate::error::CBORError;
#[cfg(feature = "trace")]
use func_trace::trace;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/***************************************************************************************************
 * Decoding Tags
 **************************************************************************************************/
/// A buffer which contains a tagged item to be decoded. The buffer has lifetime `'buf`,
/// which must be longer than any borrow from the buffer itself. This is generally used to represent
/// a CBOR map with an exposed map-like API.
///
/// This CBOR buffer implementation does not support indefinite length items.
#[derive(PartialEq, Debug, Copy, Clone)]
pub struct TagBuf<'buf> {
    tag: u64,
    bytes: &'buf [u8],
}

impl<'buf> TagBuf<'buf> {
    /// Construct a new instance of `TagBuf` with all context initialized.
    #[cfg_attr(feature = "trace", trace)]
    pub fn new(init: &'buf [u8], tag: u64) -> TagBuf<'buf> {
        TagBuf { bytes: init, tag }
    }

    /// Get the tag value for this instance of `TagBuf`.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    pub fn get_tag(&self) -> u64 {
        self.tag
    }

    /// Return the item in the `TagBuf`, converted (fallibly) from CBOR.
    ///
    pub fn item<V>(&'buf self) -> Result<V, CBORError>
        where V: TryFrom<CBOR<'buf>> + Clone
    {
        let mut it = self.into_iter();
        let item = it.next();
        match item {
            Some(cbor) => match V::try_from(cbor) {
                Ok(v) => Ok(v.clone()),
                Err(_) => Err(CBORError::IncompatibleType)
            },
            None => Err(CBORError::MalformedEncoding)
        }
    }
}

impl<'buf> IntoIterator for TagBuf<'buf> {
    type Item = CBOR<'buf>;
    type IntoIter = DecodeBufIterator<'buf>;

    /// Construct an Iterator adapter from a `DecodeBuf`.
    #[cfg_attr(feature = "trace", trace)]
    fn into_iter(self) -> Self::IntoIter {
        DecodeBufIterator {
            buf: self.bytes,
            index: 0,
            source: DecodeBufIteratorSource::Tag,
        }
    }
}

/***************************************************************************************************
 * Encoding Tags
 **************************************************************************************************/

/// A container structure for the closure used to manage encoding of CBOR tags, and in particular
/// to ensure that the correct lifetime bounds are specified.
///
/// The user is able to encode the tagged value within a closure, and the tag will
/// automatically be correctly constructed. There is a run-time check to ensure that only a single
/// CBOR item is tagged (CBOR tag applies only to the next item.
///
/// Users should never need to directly instantiate `Map`. Instead, see [`map`].
pub struct Tag<F>
where
    F: for<'f, 'buf> Fn(
        &'f mut EncodeBuffer<'buf>,
    ) -> Result<&'f mut EncodeBuffer<'buf>, CBORError>,
{
    tag: u64,
    f: F,
}

impl<F> Tag<F>
where
    F: for<'f, 'buf> Fn(
        &'f mut EncodeBuffer<'buf>,
    ) -> Result<&'f mut EncodeBuffer<'buf>, CBORError>,
{
    pub fn new(tag: u64, f: F) -> Tag<F> {
        Tag { tag, f }
    }
}

impl<F> EncodeItem for Tag<F>
where
    F: for<'f, 'buf> Fn(
        &'f mut EncodeBuffer<'buf>,
    ) -> Result<&'f mut EncodeBuffer<'buf>, CBORError>,
{
    fn encode<'f, 'buf>(
        &self,
        buf: &'f mut EncodeBuffer<'buf>,
    ) -> Result<&'f mut EncodeBuffer<'buf>, CBORError> {
        let mut tag_ctx = EncodeContext::new();
        buf.tag_start(&mut tag_ctx)?;
        let _ = buf.tag_next_item(self.tag)?;
        let _ = (self.f)(buf)?;
        buf.tag_finalize(&tag_ctx)
    }
}

/// A convenience function for the user to tag a CBOR item. The user provides a
/// closure which constructs the array contents.
///
/// ```
///# use tps_minicbor::encoder::CBORBuilder;
///# use tps_minicbor::error::CBORError;
///# use tps_minicbor::types::tag;
///
///# fn main() -> Result<(), CBORError> {
///    let mut buffer = [0u8; 16];
///
///    let mut encoder = CBORBuilder::new(&mut buffer);
///    encoder.insert(&tag(37, |buf| buf.insert(&-31i16)))?;
///#    Ok(())
///# }
/// ```
pub fn tag<F>(tag: u64, f: F) -> Tag<F>
where
    F: for<'f, 'buf> Fn(
        &'f mut EncodeBuffer<'buf>,
    ) -> Result<&'f mut EncodeBuffer<'buf>, CBORError>,
{
    Tag::new(tag, f)
}
