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
 * CBOR Decoder
 *
 * A fairly comprehensive, memory efficient, deserializer for CBOR (RFC8949).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
/// # Low-level CBOR decoding functions
///
/// This module contains the low-level CBOR decoding primitives. While it is generally recommended
/// to parse CBOR using the higher-level primitives in the [`decode_combinators`] module, where
/// memory is very constrained, the low level parsing functions allow for a reasonably comfortable
/// iterator-based decoding style.
///
/// CBOR input is parsed via a [`SequenceBuffer`], which is a constructed over a byte slice and
/// keeps track of the current parse position and the like.
///
/// ## Example
///
/// ```
///# use std::convert::TryFrom;
///# use tps_minicbor::decoder::SequenceBuffer;
///# use tps_minicbor::types::CBOR;
/// let b = [0x18u8; 0x18];
/// let buf = SequenceBuffer::new(&b);
/// let mut it = buf.into_iter();
/// if let Some(cbor) = it.next() {
///     assert_eq!(CBOR::UInt(24), cbor);
/// } else {
///     assert!(false)
/// }
/// ```

use crate::array::ArrayBuf;
use crate::ast::CBOR;
use crate::constants::*;
use crate::decode::DecodeBufIteratorSource::Sequence;
use crate::error::{CBORError, Result};
use crate::map::MapBuf;
use crate::tag::TagBuf;
use crate::utils::within;

use std::convert::TryInto;
use std::mem::size_of;
use std::str::from_utf8;

#[cfg(feature = "float")]
use half::f16;

#[cfg(feature = "trace")]
use func_trace::trace;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/***************************************************************************************************
 * Integer parsing assistance
 **************************************************************************************************/

/// Value obtained by reading an unsigned value, retaining original representation.
#[derive(Debug)]
pub enum AnyUnsigned {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

impl<'buf> AnyUnsigned {
    /// Convert `AnyUnsigned` into a `usize`. ALways succeeds.
    #[cfg_attr(feature = "trace", trace)]
    fn as_usize(self) -> usize {
        match self {
            Self::U8(v) => v as usize,
            Self::U16(v) => v as usize,
            Self::U32(v) => v as usize,
            Self::U64(v) => v as usize,
        }
    }
    /// Convert `AnyUnsigned` into a `u64`. ALways succeeds.
    #[cfg_attr(feature = "trace", trace)]
    fn as_u64(self) -> u64 {
        match self {
            Self::U8(v) => v as u64,
            Self::U16(v) => v as u64,
            Self::U32(v) => v as u64,
            Self::U64(v) => v,
        }
    }
    /// Convert `AnyUnsigned` into a `CBOR::Simple` value. We follow the rules in [RFC8949] for
    /// Simple values: 20..23 have particular meanings; 24..31 are illegal; values must be encoded
    /// on 8 bits (the larger values are encodings for floats).
    #[cfg_attr(feature = "trace", trace)]
    fn try_into_simple(self) -> Result<CBOR<'buf>> {
        match self {
            Self::U8(v) => match v {
                0..=19 => Ok(CBOR::Simple(v)),
                20 => Ok(CBOR::False),
                21 => Ok(CBOR::True),
                22 => Ok(CBOR::Null),
                23 => Ok(CBOR::Undefined),
                24..=31 => Err(CBORError::MalformedEncoding),
                v => Ok(CBOR::Simple(v)),
            },
            _ => Err(CBORError::MalformedEncoding),
        }
    }
}

/***************************************************************************************************
 * CBOR Sequence Buffer definitions
 **************************************************************************************************/

/// A buffer which contains a CBOR Sequence CBOR to be decoded. The buffer has lifetime `'buf`,
/// which must be longer than any borrow from the buffer itself. This is generally used to represent
/// an RFC8742 CBOR sequence with an exposed Iterator API, or as the top level structure generally
/// for CBOR parsing.
///
/// This CBOR buffer implementation does not support indefinite length items.
#[derive(Debug, Copy, Clone)]
pub struct SequenceBuffer<'buf> {
    /// Underlying reference to data buffer
    pub bytes: &'buf [u8],
}

impl<'buf> SequenceBuffer<'buf> {
    /// Construct a new instance of `DecodeBuf` with all context initialized.
    ///
    /// ## Example
    /// ```
    ///# use tps_minicbor::decoder::SequenceBuffer;
    /// let b = [0x18u8; 0x18];
    /// let buf = SequenceBuffer::new(&b);
    /// ```
    #[cfg_attr(feature = "trace", trace)]
    pub fn new(init: &'buf [u8]) -> SequenceBuffer<'buf> {
        SequenceBuffer { bytes: init }
    }
}

/// A `DecodeBufIterator` can be constructed from any of `SequenceBuffer`, `ArrayBuf`, `MapBuf`
/// or `TagBuf`. We keep track of which of these was the source of the iterator as it has some
/// impact on which combinator operations are allowed.
#[derive(Debug, Clone, Copy)]
pub enum DecodeBufIteratorSource {
    Sequence,
    Array,
    Map,
    Tag,
}

/// `DecodeBuffer` Iterator adapter to keep track of current position in `DecodeBuf`.
#[derive(Debug, Clone, Copy)]
pub struct DecodeBufIterator<'buf> {
    /// This is the `DecodeBuf` itself. It's a simple wrapper around a reference.
    pub buf: &'buf [u8],
    /// The current position in `buf`.
    pub index: usize,
    /// The source of this `DecodeBufIterator instance.
    pub source: DecodeBufIteratorSource,
}

impl<'buf> IntoIterator for SequenceBuffer<'buf> {
    type Item = CBOR<'buf>;
    type IntoIter = DecodeBufIterator<'buf>;

    /// Construct an Iterator adapter from a `DecodeBuf`.
    #[cfg_attr(feature = "trace", trace)]
    fn into_iter(self) -> Self::IntoIter {
        DecodeBufIterator {
            buf: self.bytes,
            index: 0,
            source: Sequence,
        }
    }
}

impl<'buf> DecodeBufIterator<'buf> {
    /// Parse a single CBOR item from DecodeBufIterator. On exit, `self.index` will point at the
    /// start of the next item (if there is one)
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    fn item(&mut self) -> Result<CBOR<'buf>> {
        let (next_index, cbor) = parse_item(self.buf, self.index)?;
        self.index = next_index;
        Ok(cbor)
    }
}

impl<'buf> Iterator for DecodeBufIterator<'buf> {
    type Item = CBOR<'buf>;

    #[cfg_attr(feature = "trace", trace)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.buf.len() {
            match self.item() {
                Ok(it) => Some(it),
                _ => None,
            }
        } else {
            None
        }
    }
}
/***************************************************************************************************
 * CBOR Parser
 **************************************************************************************************/

/// Basic function for parsing a single CBOR Item from `buf` starting at `start_index`.
///
/// Assuming that all goes well, a pair, `(usize, CBOR)` is returned where the `usize` value is the
/// index in `buf` of the next item in `buf` - this may be outside the bounds of `buf`, and must
/// be checked before it is used. This function does bounds checking, so it is safe to use a
/// previously returned next item index as an error will be returned if it is out of bounds.
#[cfg(all(feature = "float", feature = "full"))]
fn parse_item(buf: &[u8], start_index: usize) -> Result<(usize, CBOR)> {
    if within(buf, start_index, 0) {
        let mt_ai_byte = buf[start_index];
        match mt_ai_byte {
            // Positive integers
            0x00..=0x1b => parse_unsigned(buf, start_index)
                .map(|(next_idx, val)| (next_idx, CBOR::UInt(val.as_u64()))),
            // Negative integers
            0x20..=0x3b => parse_unsigned(buf, start_index)
                .map(|(next_idx, val)| (next_idx, CBOR::NInt(val.as_u64()))),
            // Byte Strings
            0x40..=0x5b => parse_bytestring(buf, start_index)
                .map(|(next_idx, bytes)| (next_idx, CBOR::Bstr(bytes))),
            // TODO: 0x5f - indefinite length byte string
            // UTF8 strings
            0x60..=0x7b => {
                let (next_index, raw_bytes) = parse_bytestring(buf, start_index)?;
                match from_utf8(&raw_bytes) {
                    Ok(s) => Ok((next_index, CBOR::Tstr(s))),
                    Err(_) => Err(CBORError::UTF8Error),
                }
            }
            // TODO: 0x7f - indefinite length string
            // Arrays
            0x80..=0x9b => parse_array(buf, start_index),
            // TODO: 0x9f - indefinite length array
            // Maps
            0xa0..=0xbb => parse_map(buf, start_index),
            // TODO: 0xbf - indefinite length map
            // Tagged values
            0xc0..=0xdb => parse_tag(buf, start_index),
            // Simple values
            0xe0..=0xf8 => {
                let (next_index, v) = parse_unsigned(buf, start_index)?;
                Ok((next_index, v.try_into_simple()?))
            }
            0xf9 => {
                let (next_index, val) = parse_f16(buf, start_index)?;
                Ok((next_index, CBOR::Float16(val)))
            }
            0xfa => {
                let (next_index, val) = parse_f32(buf, start_index)?;
                Ok((next_index, CBOR::Float32(val)))
            }
            0xfb => {
                let (next_index, val) = parse_f64(buf, start_index)?;
                Ok((next_index, CBOR::Float64(val)))
            }
            _ => Err(CBORError::NotImplemented),
        }
    } else {
        Err(CBORError::EndOfBuffer)
    }
}

// Version for no float and no full
#[cfg(not(feature = "float"))]
fn parse_item(buf: &[u8], start_index: usize) -> Result<(usize, CBOR)> {
    if within(buf, start_index, 0) {
        let mt_ai_byte = buf[start_index];
        match mt_ai_byte {
            // Positive integers
            0x00..=0x1b => parse_unsigned(buf, start_index)
                .map(|(next_idx, val)| (next_idx, CBOR::UInt(val.as_u64()))),
            // Negative integers
            0x20..=0x3b => parse_unsigned(buf, start_index)
                .map(|(next_idx, val)| (next_idx, CBOR::NInt(val.as_u64()))),
            // Byte Strings
            0x40..=0x5b => parse_bytestring(buf, start_index)
                .map(|(next_idx, bytes)| (next_idx, CBOR::Bstr(bytes))),
            // TODO: 0x5f - indefinite length byte string
            // UTF8 strings
            0x60..=0x7b => {
                let (next_index, raw_bytes) = parse_bytestring(buf, start_index)?;
                match from_utf8(&raw_bytes) {
                    Ok(s) => Ok((next_index, CBOR::Tstr(s))),
                    Err(_) => Err(CBORError::UTF8Error),
                }
            }
            // TODO: 0x7f - indefinite length string
            // Arrays
            0x80..=0x9b => parse_array(buf, start_index),
            // TODO: 0x9f - indefinite length array
            // Maps
            0xa0..=0xbb => parse_map(buf, start_index),
            // TODO: 0xbf - indefinite length map
            // Tagged values
            0xc0..=0xdb => parse_tag(buf, start_index),
            // Simple values
            0xe0..=0xf8 => {
                let (next_index, v) = parse_unsigned(buf, start_index)?;
                Ok((next_index, v.try_into_simple()?))
            }
            _ => Err(CBORError::NotImplemented),
        }
    } else {
        Err(CBORError::EndOfBuffer)
    }
}

/***************************************************************************************************
 * Integer parser helpers
 **************************************************************************************************/

/// Parse an unsigned integer value.
///
/// On entry the `start` index is assumed to identify an MT/AI byte within `buf`.
/// On return we have a sized unsigned integer value and the index within `buf` of the next value.
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn parse_unsigned(buf: &[u8], start_index: usize) -> Result<(usize, AnyUnsigned)> {
    // We do not care about the value of the MT bits
    if within(buf, start_index, 0) {
        let ai = buf[start_index] & AI_MASK;
        if ai <= PAYLOAD_AI_BITS {
            Ok((start_index + size_of::<u8>(), AnyUnsigned::U8(ai)))
        } else if ai == PAYLOAD_ONE_BYTE {
            let (next_index, item_slice) = read_extent(buf, start_index + 1, size_of::<u8>())?;
            let result: core::result::Result<[u8; 1], _> = item_slice.try_into();
            match result {
                Ok(bytes) => Ok((next_index, AnyUnsigned::U8(u8::from_be_bytes(bytes)))),
                Err(_) => Err(CBORError::BadSliceLength),
            }
        } else if ai == PAYLOAD_TWO_BYTES {
            let (next_index, item_slice) = read_extent(buf, start_index + 1, size_of::<u16>())?;
            let result: core::result::Result<[u8; 2], _> = item_slice.try_into();
            match result {
                Ok(bytes) => Ok((next_index, AnyUnsigned::U16(u16::from_be_bytes(bytes)))),
                Err(_) => Err(CBORError::BadSliceLength),
            }
        } else if ai == PAYLOAD_FOUR_BYTES {
            let (next_index, item_slice) = read_extent(buf, start_index + 1, size_of::<u32>())?;
            let result: core::result::Result<[u8; 4], _> = item_slice.try_into();
            match result {
                Ok(bytes) => Ok((next_index, AnyUnsigned::U32(u32::from_be_bytes(bytes)))),
                Err(_) => Err(CBORError::BadSliceLength),
            }
        } else if ai == PAYLOAD_EIGHT_BYTES {
            let (next_index, item_slice) = read_extent(buf, start_index + 1, size_of::<u64>())?;
            let result: core::result::Result<[u8; 8], _> = item_slice.try_into();
            match result {
                Ok(bytes) => Ok((next_index, AnyUnsigned::U64(u64::from_be_bytes(bytes)))),
                Err(_) => Err(CBORError::BadSliceLength),
            }
        } else {
            Err(CBORError::MalformedEncoding)
        }
    } else {
        Err(CBORError::EndOfBuffer)
    }
}

/***************************************************************************************************
 * Float Parse Helpers
 **************************************************************************************************/

/// Parse a 64bit floating point value.
///
/// On entry the `start` index is assumed to identify an MT/AI byte within `buf`.
/// On return we have an `f64` value and the index within `buf` of the next value.
#[cfg(feature = "float")]
#[cfg_attr(feature = "trace", trace)]
fn parse_f64(buf: &[u8], start_index: usize) -> Result<(usize, f64)> {
    let (next_index, item_slice) = read_extent(buf, start_index + 1, size_of::<f64>())?;
    let result: core::result::Result<[u8; 8], _> = item_slice.try_into();
    match result {
        Ok(bytes) => Ok((next_index, f64::from_be_bytes(bytes))),
        Err(_) => Err(CBORError::BadSliceLength),
    }
}

/// Parse a 32bit floating point value.
///
/// On entry the `start` index is assumed to identify an MT/AI byte within `buf`.
/// On return we have an `f32` value and the index within `buf` of the next value.
#[cfg(feature = "float")]
#[cfg_attr(feature = "trace", trace)]
fn parse_f32(buf: &[u8], start_index: usize) -> Result<(usize, f32)> {
    let (next_index, item_slice) = read_extent(buf, start_index + 1, size_of::<f32>())?;
    let result: core::result::Result<[u8; 4], _> = item_slice.try_into();
    match result {
        Ok(bytes) => Ok((next_index, f32::from_be_bytes(bytes))),
        Err(_) => Err(CBORError::BadSliceLength),
    }
}

/// Parse a 16bit floating point value.
///
/// On entry the `start` index is assumed to identify an MT/AI byte within `buf`.
/// On return we have an `f16` value and the index within `buf` of the next value.
#[cfg(feature = "float")]
#[cfg_attr(feature = "trace", trace)]
fn parse_f16(buf: &[u8], start_index: usize) -> Result<(usize, f16)> {
    let (next_index, item_slice) = read_extent(buf, start_index + 1, size_of::<f16>())?;
    let result: core::result::Result<[u8; 2], _> = item_slice.try_into();
    match result {
        Ok(bytes) => Ok((next_index, f16::from_be_bytes(bytes))),
        Err(_) => Err(CBORError::BadSliceLength),
    }
}

/***************************************************************************************************
 * Bytestring, Arrays, Maps and String Helpers
 **************************************************************************************************/

/// Parse a bytestring starting at `start_index` in buffer `buf`. The index `start_index` should
/// indicate the MT/AI byte for the item to be parsed.
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn parse_bytestring(buf: &[u8], start_index: usize) -> Result<(usize, &[u8])> {
    let (start_bstr_index, value) = parse_unsigned(buf, start_index)?;
    let length = value.as_usize();
    let (next_item_index, bytes) = read_extent(buf, start_bstr_index, length)?;
    Ok((next_item_index, bytes))
}

/// Parse an array. An array of length N is simply a sequence of N CBOR Items, some of which
/// could themselves be arrays or maps.
///
/// In order to avoid heap allocation we return a typed buffer which itself can be mapped over
/// with an iterator and other helpful API functions resembling the slice API provided by Rust
/// as standard.
#[cfg_attr(feature = "trace", trace)]
fn parse_array(buf: &[u8], start_index: usize) -> Result<(usize, CBOR)> {
    let (array_start_index, u_value) = parse_unsigned(buf, start_index)?;
    let n_items = u_value.as_usize();
    let next_index = skip_items(buf, array_start_index, n_items)?;

    // No need to check that length + index is legal - already checked in skip_item
    Ok((
        next_index,
        CBOR::Array(ArrayBuf::new(&buf[array_start_index..next_index], n_items)),
    ))
}

/// Parse a map. An map of N items is simply a sequence of N*2 CBOR Items, some of which
/// could themselves be arrays or maps.
///
/// In order to avoid heap allocation we return a typed buffer which itself can be mapped over
/// with an iterator and other helpful API functions resembling the slice API provided by Rust
/// as standard.
#[cfg_attr(feature = "trace", trace)]
fn parse_map(buf: &[u8], start_index: usize) -> Result<(usize, CBOR)> {
    let (array_start_index, value) = parse_unsigned(buf, start_index)?;
    let n_pairs = value.as_usize();
    let n_items = n_pairs * 2; // We read pairs of Items
    let next_index = skip_items(buf, array_start_index, n_items)?;

    // No need to check that length + index is legal - already checked in skip_item
    Ok((
        next_index,
        CBOR::Map(MapBuf::new(&buf[array_start_index..next_index], n_pairs)),
    ))
}

/// Parse a tagged item. An Item tagged with N is followed by a single CBOR Item, which
/// could be an array or map.
///
/// In order to avoid heap allocation we return a typed buffer which itself can be mapped over
/// with an iterator and other helpful API functions resembling the slice API provided by Rust
/// as standard.
#[cfg_attr(feature = "trace", trace)]
fn parse_tag(buf: &[u8], start_index: usize) -> Result<(usize, CBOR)> {
    let (tag_item_start_index, tag_value) = parse_unsigned(buf, start_index)?;
    let next_index = parse_item(buf, tag_item_start_index)?.0;
    Ok((
        next_index,
        CBOR::Tag(TagBuf::new(
            &buf[tag_item_start_index..next_index],
            tag_value.as_u64(),
        )),
    ))
}

/***************************************************************************************************
 * Other helpers
 **************************************************************************************************/

/// Try to skip over N Items, returning the index (which may be out of bounds) of the start of the
/// N+1 thItem.
///
/// There is no "parse" variant for this function because, in a no_std environment, we have no way
/// to return a sequence of CBOR directly.
#[cfg_attr(feature = "trace", trace)]
fn skip_items(buf: &[u8], start_index: usize, n_items: usize) -> Result<usize> {
    let mut next_index = start_index;

    // We only call skip_items() if we are parsing an array, map or tagged item. In each case we
    // have already parsed the length component, which means that if `n_items` is zero,
    // `start_index` is alreay the index of the next item.
    // The call to `parse_item()` fails if we overflow the buffer.
    if n_items > 0 {
        for _i in 0..n_items {
            next_index = parse_item(buf, next_index)?.0;
        }
        Ok(next_index)
    } else {
        Ok(start_index)
    }
}

/// Return the index of the next item to parse and a slice over the item within `buf`.
#[cfg_attr(feature = "trace", trace)]
fn read_extent(buf: &[u8], start: usize, length: usize) -> Result<(usize, &[u8])> {
    if within(buf, start, length) {
        Ok((start + length, &buf[start..start + length]))
    } else {
        Err(CBORError::EndOfBuffer)
    }
}
