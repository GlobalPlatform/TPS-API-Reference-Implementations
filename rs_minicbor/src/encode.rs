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
 * CBOR Encoder
 *
 * A fairly comprehensive, memory efficient, serializer for CBOR (RFC7049). This serializer
 * is designed for use in constrained systems and requires neither the Rust standard library
 * nor an allocator.
 *
 * There is an optional simplified serialization API which has a small memory cost. This can be
 * disabled with the `embedded` profile option.
 **************************************************************************************************/
use crate::ast::CBOR;
use crate::constants::*;
use crate::decoder::{is_any, CBORDecoder, SequenceBuffer};
use crate::error::CBORError;
use crate::utils::within;

#[cfg(feature = "std_tags")]
use std::mem::size_of;

#[cfg(feature = "std_tags")]
use crate::ast::CBOR::{NInt, Tstr, UInt};

#[cfg(feature = "float")]
use half::f16;

#[cfg(any(feature = "std_tags", feature = "trace"))]
use std::string::String;

#[cfg(feature = "std_tags")]
use chrono::{DateTime, FixedOffset};

#[cfg(feature = "trace")]
use func_trace::trace;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

// Private structure used when returning integer encodings to indicate that the Major Type has
// not been set. The caller should consider whether this needs to be addressed
#[derive(Debug, Clone)]
struct MtUnset(usize);

/// CBORBuilder implements the Builder Pattern to simplify encoding data as CBOR.
///
/// Usage is something similar to the following:
///
/// ```
///# use rs_minicbor::encoder::CBORBuilder;
///# use rs_minicbor::error::CBORError;
///# use rs_minicbor::types::array;
///# fn main() -> Result<(), CBORError> {
///    let mut buffer = [0u8; 64];
///    let expected : &[u8] = &[0x83, 0x01, 0x82, 0x02, 0x03, 0x82, 0x04, 0x05];
///
///    let mut encoder = CBORBuilder::new(&mut buffer);
///     encoder.insert(&array(|buff| {
///     buff.insert(&1u8)?
///         .insert(&array(|buff| buff.insert(&2u8)?.insert(&3u8)))?
///         .insert(&array(|buff| buff.insert(&4u8)?.insert(&5u8)))
///    }));
///    assert_eq!(encoder.encoded()?, expected);
///#    Ok(())
///# }
/// ```
#[derive(Debug)]
#[cfg(feature = "combinators")]
pub struct CBORBuilder<'buf> {
    pub(self) buf: EncodeBuffer<'buf>,
}

#[cfg(feature = "combinators")]
impl<'buf> CBORBuilder<'buf> {
    /// Create an instance of CBORBuilder from a mutable reference to a slice.
    pub fn new(buf: &'buf mut [u8]) -> Self {
        CBORBuilder {
            buf: EncodeBuffer::new(buf),
        }
    }

    /// Insert an `EncodeItem` item into an `EncodeBuffer`.
    #[inline]
    pub fn insert(&mut self, item: &dyn EncodeItem) -> Result<&mut Self, CBORError> {
        self.buf.insert(item)?;
        Ok(self)
    }

    /// Return the underlying slice with CBOR encoded data
    #[inline]
    pub fn encoded(&self) -> Result<&[u8], CBORError> {
        self.buf.encoded()
    }

    /// Return an instance of `SequenceBuffer` owning the underlying slice
    pub fn build(&'buf self) -> Result<SequenceBuffer<'buf>, CBORError> {
        Ok(SequenceBuffer::new(self.buf.encoded()?))
    }
}

/***************************************************************************************************
 * Encode Buffer
 **************************************************************************************************/

/// `EncodeBuffer` is a smart view over a byte slice onto which we are encoding CBOR data. In most
/// cases it is wrapped with a `CBORBuilder`, but it can be used stand-alone in which case the
/// simplest code looks something like:
/// ```
///# use rs_minicbor::encoder::EncodeBuffer;
///# use rs_minicbor::error::CBORError;
///# use rs_minicbor::types::array;
///# fn main() -> Result<(), CBORError> {
///    use rs_minicbor::encoder::EncodeBuffer;
/// let mut buffer = [0u8; 64];
///    let expected : &[u8] = &[24, 32];
///
///    let mut encoder = EncodeBuffer::new(&mut buffer);
///    encoder.insert(&32)?;
///    assert_eq!(encoder.encoded()?, expected);
///#   Ok(())
///# }
/// ```
///
#[derive(Debug)]
#[cfg(feature = "combinators")]
pub struct EncodeBuffer<'buf> {
    bytes: &'buf mut [u8],
    index: usize,
    items: usize,
}

#[cfg(feature = "combinators")]
impl<'buf, 'short> EncodeBuffer<'buf>
where
    'buf: 'short,
{
    /// Construct an instance of EncodeBuffer from a buffer.
    ///
    /// The buffer is cleared on each instantiation of `EncodeBuffer`. This allows the same
    /// underlying mutable buffer to be re-used.
    ///
    /// ```
    ///# use rs_minicbor::encoder::EncodeBuffer;
    /// let mut buf = [0u8; 16];
    /// let mut encode_buffer = EncodeBuffer::new(&mut buf);
    /// ```
    #[inline]
    pub fn new(b: &'buf mut [u8]) -> EncodeBuffer<'buf> {
        b.fill(0);
        EncodeBuffer {
            bytes: b,
            index: 0,
            items: 0,
        }
    }

    /// Insert an `EncodeItem` item into an `EncodeBuffer`.
    pub fn insert(&mut self, item: &dyn EncodeItem) -> Result<&mut Self, CBORError> {
        let _ = item.encode(self)?;
        self.items += 1;
        Ok(self)
    }

    /// Insert a (key, value) pair of `EncodeItems` into an `EncodeBuffer`.
    ///
    /// This function is most likely to be useful when encoding CBOR maps, although it actually
    /// is just a convenience function for calling `insert` twice in sequence.
    pub fn insert_key_value(
        &mut self,
        key: &dyn EncodeItem,
        value: &dyn EncodeItem,
    ) -> Result<&mut Self, CBORError> {
        let _ = self.insert(key)?;
        let _ = self.insert(value)?;
        Ok(self)
    }

    /// Tag the item that follows
    pub(crate) fn tag_next_item(&mut self, tag: u64) -> Result<usize, CBORError> {
        // Encode the tag
        let tag_len = encode_unsigned(self, tag)?;
        self.set_mt(MT_TAG);
        self.update_index(tag_len.0 + 1)?;
        Ok(tag_len.0)
    }

    /// Marker for the start of a CBOR Array structure, which must later be finalized with a call
    /// to `array_finalize`.
    ///
    /// > End-users should not call this function directly. The [`Array::array`] function manages
    /// > this automatically.
    ///
    /// Information about the state of the buffer before the insertion of the Array is saved in an
    /// opaque `Array` context structure which is used to store information required to fix up the
    /// array length information once it is known.
    ///
    /// If the array is not finalized, the encoded CBOR representation will be incorrect.
    #[inline]
    pub(crate) fn array_start(&mut self, ctx: &mut EncodeContext) -> Result<&mut Self, CBORError> {
        ctx.context_type = ContextType::Array;
        self.context_start_common(ctx)
    }

    /// Marker to finalize a CBOR Array structure once its contents have been inserted, using the
    /// information in an `Array` context to complete the finalization depending on the number of
    /// items inserted.
    ///
    /// > End-users should not call this function directly. The [`Array::array`] function manages
    /// > this automatically.
    #[inline]
    pub(crate) fn array_finalize(&mut self, ctx: &EncodeContext) -> Result<&mut Self, CBORError> {
        self.context_finalize_common(ctx)
    }

    /// Marker for the start of a CBOR Map structure, which must later be finalized with a call
    /// to `map_finalize`.
    ///
    /// > End-users should not call this function directly. The [`Map::map`] function manages
    /// > this automatically.
    ///
    /// Information about the state of the buffer before the insertion of the Map is saved in an
    /// opaque `Map` context structure which is used to store information required to fix up the
    /// map length information once it is known.
    ///
    /// If the map is not finalized, the encoded CBOR representation will be incorrect.
    #[inline]
    pub(crate) fn map_start(&mut self, ctx: &mut EncodeContext) -> Result<&mut Self, CBORError> {
        ctx.context_type = ContextType::Map;
        self.context_start_common(ctx)
    }

    /// Marker to finalize a CBOR Map structure once its contents have been inserted, using the
    /// information in an `Map` context to complete the finalization depending on the number of
    /// items inserted.
    ///
    /// > End-users should not call this function directly. The [`Map::map`] function manages
    /// > this automatically.
    #[inline]
    pub(crate) fn map_finalize(&mut self, ctx: &EncodeContext) -> Result<&mut Self, CBORError> {
        self.context_finalize_common(ctx)
    }

    /// Marker for the start of a CBOR Tag structure, which must later be finalized with a call
    /// to `tag_finalize`.
    ///
    /// > End-users should not call this function directly. The [`Tag::tag`] function manages
    /// > this automatically.
    ///
    /// Information about the state of the buffer before the insertion of the Tag is saved in an
    /// opaque `Tag` context structure which is used to store information required to check that
    /// only a single CBOR item was serialized in the tag closure.
    ///
    /// If the tag is not finalized, the encoded CBOR representation may be.
    #[inline]
    pub(crate) fn tag_start(&mut self, ctx: &mut EncodeContext) -> Result<&mut Self, CBORError> {
        // Save tag context. DO NOT call context_start_common as this manipulates the buffer
        // index which is needed for array and map, but *not* for tag.
        ctx.context_type = ContextType::Tag;
        ctx.mt_ai_index = self.get_index()?;
        ctx.no_of_items_before_ctx = self.items;
        ctx.ctx_encode_start = ctx.mt_ai_index + 1;
        Ok(self)
    }

    /// Marker to finalize a CBOR Tag structure once its contents have been inserted, using the
    /// information in a `Tag` context to ensure that only a single item was created.
    ///
    /// > End-users should not call this function directly. The [`Array::array`] function manages
    /// > this automatically.
    #[inline]
    pub(crate) fn tag_finalize(&mut self, _ctx: &EncodeContext) -> Result<&mut Self, CBORError> {
        // TODO: Currently there is no good way to check that the Tag has only a single item
        Ok(self)
    }

    /// Return a slice containing the encoded input.
    ///
    /// Will generate a buffer overflow error if the current encoding overflowed the buffer
    //#[cfg_attr(feature = "trace", trace)]
    pub fn encoded(&self) -> Result<&[u8], CBORError> {
        if within(self.bytes, 0, self.index) {
            Ok(self.bytes[0..self.index].as_ref())
        } else {
            Err(CBORError::EndOfBuffer)
        }
    }

    /// Return `true` if `offset` is within the remaining space in the buffer (i.e. starting at
    /// `index`.
    #[cfg_attr(feature = "trace", trace)]
    fn within(&'buf self, offset: usize) -> bool {
        within(self.bytes, self.index, offset)
    }

    /// Update `index` with the number of bytes inserted
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn update_index(&mut self, len: usize) -> Result<usize, CBORError> {
        self.index += len;
        Ok(len)
    }

    /// Set `index` to an absolute position within the buffer
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn set_index_abs(&mut self, index: usize) {
        self.index = index
    }

    /// Get the current value of `index`.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    pub(crate) fn get_index(&self) -> Result<usize, CBORError> {
        if self.within(0) {
            Ok(self.index)
        } else {
            Err(CBORError::EndOfBuffer)
        }
    }

    /// Set the Major Type. Assumes that `index` is at the `MT/AI` byte.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn set_mt(&mut self, mt: u8) {
        self.bytes[self.index] |= mt;
    }

    /// Write a byte at an `offset` from the current `index` where the Item being processed starts.
    ///
    /// Will generate a buffer overflow error if the write would overflow the buffer
    #[cfg_attr(feature = "trace", trace)]
    fn write_byte_at_offset(&mut self, offset: usize, val: u8) -> Result<(), CBORError> {
        if within(self.bytes, self.index, offset) {
            self.bytes[self.index + offset] = val;
            Ok(())
        } else {
            Err(CBORError::EndOfBuffer)
        }
    }

    /// Write values from `src` to an `offset` from the current `index` where the item being
    /// processed starts.
    ///
    /// Will generate a buffer overflow error if the write would overflow the buffer
    #[cfg_attr(feature = "trace", trace)]
    fn write_slice_at_offset(&mut self, offset: usize, src: &[u8]) -> Result<(), CBORError> {
        if within(self.bytes, self.index, offset + src.len()) {
            self.bytes[self.index + offset..self.index + offset + src.len()].copy_from_slice(src);
            Ok(())
        } else {
            Err(CBORError::EndOfBuffer)
        }
    }

    /// Move items from `src_index` to `dst_index`, where `src_index` < `dest_index`.
    #[cfg_attr(feature = "trace", trace)]
    fn move_items(
        &mut self,
        src_index: usize,
        dst_index: usize,
        len: usize,
    ) -> Result<(), CBORError> {
        if src_index < dst_index {
            if self.within(dst_index + len) {
                for i in (0..len).rev() {
                    self.bytes[dst_index + i] = self.bytes[src_index + i];
                }
                Ok(())
            } else {
                Err(CBORError::EndOfBuffer)
            }
        } else {
            Err(CBORError::BadSliceLength)
        }
    }

    fn context_start_common(&mut self, ctx: &mut EncodeContext) -> Result<&mut Self, CBORError> {
        // Save the context of the start of the array
        ctx.mt_ai_index = self.get_index()?;
        ctx.no_of_items_before_ctx = self.items;
        ctx.ctx_encode_start = ctx.mt_ai_index + 1;

        // Update the buffer index to start to the next element after MT/AI. We may need to
        // revisit this on finalization.
        self.update_index(1)?;
        Ok(self)
    }

    fn context_finalize_common(&mut self, ctx: &EncodeContext) -> Result<&mut Self, CBORError> {
        // Determine what we put into the array
        let context_encode_end = self.get_index()?;
        let no_of_items_after_context_added = self.items;
        let context_items_len_bytes = context_encode_end - ctx.ctx_encode_start;

        // Create a slice over the content of the array or map and count the items
        let content = &self.bytes[ctx.ctx_encode_start..self.get_index()?];
        let ctx_param_value = if context_items_len_bytes > 0 {
            match ctx.context_type {
                ContextType::Array => count_items(content),
                ContextType::Map => count_items(content) / 2,
                // There should be no path to ContextType::Tag - context_finalize_common is
                // not called from tag_finalize()
                ContextType::Tag => return Err(CBORError::NotAllowed)
            }
        } else {
            0
        };

        // We need to check the size of encoding for the number of array items. If it is more than
        // can fit on MT/AI byte, we will need to move the encoded array items to follow the encoded
        // number of items. This is unfortunate, but it is consequence of not knowing number of
        // items a-priori
        let ctx_param_len = match ctx_param_value {
            0..=23 => 0,
            24..=0xff => 1,
            0x100..=0xffff => 2,
            0x10000..=0xffff_ffff => 4,
            _ => 8,
        };

        if ctx_param_len > 0 {
            // Move array items up by ctx_param_len
            self.move_items(
                ctx.ctx_encode_start,
                ctx.ctx_encode_start + ctx_param_len,
                context_items_len_bytes,
            )?;
        }

        // Now can go back and encode array length and MT/AI byte
        self.set_index_abs(ctx.mt_ai_index);
        let _ = encode_unsigned(self, ctx_param_value as u64)?;

        match ctx.context_type {
            ContextType::Array => self.set_mt(MT_ARRAY),
            ContextType::Map => self.set_mt(MT_MAP),
            // There should be no path to ContextType::Tag - context_finalize_common is
            // not called from tag_finalize()
            ContextType::Tag => return Err(CBORError::NotAllowed)
        }
        self.set_index_abs(context_encode_end + ctx_param_len);

        // Final check on the encoded value rules before we return a value.
        match ctx.context_type {
            ContextType::Array => Ok(self),
            ContextType::Map => {
                if (no_of_items_after_context_added - ctx.no_of_items_before_ctx) % 2 == 0 {
                    Ok(self)
                } else {
                    Err(CBORError::MalformedEncoding)
                }
            },
            // There should be no path to ContextType::Tag - context_finalize_common is
            // not called from tag_finalize()
            ContextType::Tag => return Err(CBORError::NotAllowed)
        }
    }
}

fn count_items(buf: &[u8]) -> usize {
    let mut count = 0;
    {
        let mut decoder = CBORDecoder::from_slice(buf);
        let inc_count = |_a, _b| {
            count += 1;
            Ok(())
        };

        let _r = decoder.many0(is_any(), inc_count);
    }
    count
}

/***************************************************************************************************
 * Encode Item
 **************************************************************************************************/

/// The `EncodeItem` trait encapsulates encoding operations as anything that can be serialized to
/// CBOR.
pub trait EncodeItem {
    fn encode<'f, 'buf>(
        &self,
        buf: &'f mut EncodeBuffer<'buf>,
    ) -> Result<&'f mut EncodeBuffer<'buf>, CBORError>;
}

#[cfg(feature = "std_tags")]
impl<'buf> EncodeItem for CBOR<'buf> {
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        match *self {
            CBOR::UInt(val) => (&val).encode(buf),
            CBOR::NInt(val) => {
                let signed_val: i128 = -1 - (val as i128);
                (&signed_val).encode(buf)
            }
            CBOR::Float64(val) => (&val).encode(buf),
            CBOR::Float32(val) => (&val).encode(buf),
            CBOR::Float16(val) => (&val).encode(buf),
            CBOR::Bstr(bs) => bs.encode(buf),
            CBOR::Tstr(ts) => ts.encode(buf),
            CBOR::Array(_) => Err(CBORError::NotImplemented),
            CBOR::Map(_) => Err(CBORError::NotImplemented),
            CBOR::Tag(_) => Err(CBORError::NotImplemented),
            CBOR::Simple(v) => {
                match v {
                    // Values below are reserved for specific usage or are illegal
                    20..=31 => Err(CBORError::MalformedEncoding),
                    _ => encode_item_simple(buf, v),
                }
            }
            CBOR::False => encode_item_simple(buf, 20),
            CBOR::True => encode_item_simple(buf, 21),
            CBOR::Null => encode_item_simple(buf, 22),
            CBOR::Undefined => encode_item_simple(buf, 23),
            CBOR::Eof => Err(CBORError::MalformedEncoding),
            CBOR::DateTime(date_time) => encode_date_time(buf, &date_time),
            CBOR::Epoch(secs_since_1970) => encode_epoch(buf, secs_since_1970),
        }
    }
}

#[cfg(all(feature = "float", not(feature = "std_tags")))]
impl<'buf> EncodeItem for CBOR<'buf> {
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        match *self {
            CBOR::UInt(val) => (&val).encode(buf),
            CBOR::NInt(val) => {
                let signed_val: i128 = -1 - (val as i128);
                (&signed_val).encode(buf)
            }
            CBOR::Float64(val) => (&val).encode(buf),
            CBOR::Float32(val) => (&val).encode(buf),
            CBOR::Float16(val) => (&val).encode(buf),
            CBOR::Bstr(bs) => bs.encode(buf),
            CBOR::Tstr(ts) => ts.encode(buf),
            CBOR::Array(_) => Err(CBORError::NotImplemented),
            CBOR::Map(_) => Err(CBORError::NotImplemented),
            CBOR::Tag(_) => Err(CBORError::NotImplemented),
            CBOR::Simple(v) => {
                match v {
                    // Values below are reserved for specific usage or are illegal
                    20..=31 => Err(CBORError::MalformedEncoding),
                    _ => encode_item_simple(buf, v),
                }
            }
            CBOR::False => encode_item_simple(buf, 20),
            CBOR::True => encode_item_simple(buf, 21),
            CBOR::Null => encode_item_simple(buf, 22),
            CBOR::Undefined => encode_item_simple(buf, 23),
            CBOR::Eof => Err(CBORError::MalformedEncoding),
        }
    }
}

#[cfg(not(feature = "float"))]
impl<'buf> EncodeItem for CBOR<'buf> {
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        match *self {
            CBOR::UInt(val) => (&val).encode(buf),
            CBOR::NInt(val) => {
                let signed_val: i128 = -1 - (val as i128);
                (&signed_val).encode(buf)
            }
            CBOR::Bstr(bs) => bs.encode(buf),
            CBOR::Tstr(ts) => ts.encode(buf),
            CBOR::Array(_) => Err(CBORError::NotImplemented),
            CBOR::Map(_) => Err(CBORError::NotImplemented),
            CBOR::Tag(_) => Err(CBORError::NotImplemented),
            CBOR::Simple(v) => {
                match v {
                    // Values below are reserved for specific usage or are illegal
                    20..=31 => Err(CBORError::MalformedEncoding),
                    _ => encode_item_simple(buf, v),
                }
            }
            CBOR::False => encode_item_simple(buf, 20),
            CBOR::True => encode_item_simple(buf, 21),
            CBOR::Null => encode_item_simple(buf, 22),
            CBOR::Undefined => encode_item_simple(buf, 23),
            CBOR::Eof => Err(CBORError::MalformedEncoding),
        }
    }
}

impl EncodeItem for u64 {
    /// Encode a `u64` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        let item_len = encode_unsigned(buf, *self)?;
        buf.set_mt(MT_UINT);
        buf.update_index(item_len.0 + 1)?;
        Ok(buf)
    }
}

impl EncodeItem for u32 {
    /// Encode a `u32` value on a buffer
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        (*self as u64).encode(buf)
    }
}

impl EncodeItem for u16 {
    /// Encode a `u16` value on a buffer
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        (*self as u64).encode(buf)
    }
}

impl EncodeItem for u8 {
    /// Encode a `u8` value on a buffer
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        (*self as u64).encode(buf)
    }
}

impl EncodeItem for bool {
    /// Encode a `bool` value on a buffer
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        match *self {
            true => (CBOR::True).encode(buf),
            false => (CBOR::False).encode(buf),
        }
    }
}

impl EncodeItem for i128 {
    /// Encode a `i128` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0
    /// or Major Type 1.
    ///
    /// Note that serialization of `i128` can fail out of range as it can hold values exceeding the
    /// maxima and minima for 64 bit encoding in CBOR.
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        if *self < 0 {
            let v = -1 - *self;
            if v >= 0 && v <= u64::MAX as i128 {
                // We cannot just encode as u64 because we need a different MT value
                let item_len = encode_unsigned(buf, v as u64)?;
                buf.set_mt(MT_NINT);
                buf.update_index(item_len.0 + 1)?;
                Ok(buf)
            } else {
                Err(CBORError::OutOfRange)
            }
        } else {
            if *self >= 0 && *self <= u64::MAX as i128 {
                (*self as u64).encode(buf)
            } else {
                Err(CBORError::OutOfRange)
            }
        }
    }
}

impl EncodeItem for i64 {
    /// Encode a `i64` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        if *self < 0 {
            let v = -1 - *self;
            let item_len = encode_unsigned(buf, v as u64)?;
            buf.set_mt(MT_NINT);
            buf.update_index(item_len.0 + 1)?;
            Ok(buf)
        } else {
            let item_len = encode_unsigned(buf, *self as u64)?;
            buf.set_mt(MT_UINT);
            buf.update_index(item_len.0 + 1)?;
            Ok(buf)
        }
    }
}

impl EncodeItem for i32 {
    /// Encode a `i32` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0
    /// or Major Type 1.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        (*self as i64).encode(buf)
    }
}

impl EncodeItem for i16 {
    /// Encode a `i16` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0
    /// or Major Type 1.
    #[inline]
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        (*self as i64).encode(buf)
    }
}

impl EncodeItem for i8 {
    /// Encode a `i8` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0
    /// or Major Type 1.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        (*self as i64).encode(buf)
    }
}

impl EncodeItem for &str {
    /// Encode an `&str` value onto a buffer.
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        // First encode the string length
        let item_len = encode_unsigned(buf, self.len() as u64)?;
        let len_bytes = item_len.0;

        // Then encode the string
        buf.write_slice_at_offset(1 + len_bytes, self.as_bytes())?;
        let written_bytes = self.len() + len_bytes + 1;
        buf.set_mt(MT_TSTR);
        buf.update_index(written_bytes)?;
        Ok(buf)
    }
}

impl EncodeItem for &[u8] {
    /// Encode an `&[u8]` value onto a buffer.
    #[cfg_attr(feature = "trace", trace)]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        // First encode the string length
        let item_len = encode_unsigned(buf, self.len() as u64)?;
        let len_bytes = item_len.0;

        // Then encode the byte string
        buf.write_slice_at_offset(1 + len_bytes, self)?;
        let written_bytes = self.len() + len_bytes + 1;
        buf.set_mt(MT_BSTR);
        buf.update_index(written_bytes)?;
        Ok(buf)
    }
}

#[cfg(feature = "float")]
impl EncodeItem for f64 {
    /// Encode an `f64` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0
    /// or Major Type 1.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        match preferred_encode(FloatType::Float64(*self)) {
            FloatType::Float64(_) => {
                buf.write_byte_at_offset(0, PAYLOAD_EIGHT_BYTES)?;
                buf.write_slice_at_offset(1, &(self.to_be_bytes()))?;
                let written_bytes = 1 + size_of::<f64>();
                buf.set_mt(MT_FLOAT);
                buf.update_index(written_bytes)?;
                Ok(buf)
            }
            FloatType::Float32(v32) => v32.encode(buf),
            FloatType::Float16(v16) => v16.encode(buf),
        }
    }
}

#[cfg(feature = "float")]
impl EncodeItem for f32 {
    /// Encode an `f32` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0
    /// or Major Type 1.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        match preferred_encode(FloatType::Float32(*self)) {
            FloatType::Float64(_) | FloatType::Float32(_) => {
                buf.write_byte_at_offset(0, PAYLOAD_FOUR_BYTES)?;
                buf.write_slice_at_offset(1, &(self.to_be_bytes()))?;
                let written_bytes = 1 + size_of::<f32>();
                buf.set_mt(MT_FLOAT);
                buf.update_index(written_bytes)?;
                Ok(buf)
            }
            FloatType::Float16(v16) => v16.encode(buf),
        }
    }
}

#[cfg(feature = "float")]
impl EncodeItem for f16 {
    /// Encode an `f16` value on a buffer.
    ///
    /// Value is serialized using the preferred (shortest) serialization as a Major Type 0
    /// or Major Type 1.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    fn encode<'f, 'b>(
        &self,
        buf: &'f mut EncodeBuffer<'b>,
    ) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
        buf.write_byte_at_offset(0, PAYLOAD_TWO_BYTES)?;
        buf.write_slice_at_offset(1, &(self.to_be_bytes()))?;
        let written_bytes = 1 + size_of::<f16>();
        buf.set_mt(MT_FLOAT);
        buf.update_index(written_bytes)?;
        Ok(buf)
    }
}

#[cfg(feature = "float")]
enum FloatType {
    Float16(f16),
    Float32(f32),
    Float64(f64),
}

/// To support CBOR preferred encoding of floating point values, we convert a value to each
/// possible lower precision, convert it back and check whether it is identical to the original
/// value. If it is, reduced size does not imply any loss of precision.
///
/// The comparisons are made at the byte-wise representation level
#[cfg(feature = "float")]
fn preferred_encode(v: FloatType) -> FloatType {
    match v {
        FloatType::Float64(v64) => {
            let v32 = v64 as f32;
            let v16: f16 = f16::from_f64(v64);

            let c64_64 = v64.to_ne_bytes();
            let c16_64 = (f16::to_f64(v16)).to_ne_bytes();
            let c32_64 = (v32 as f64).to_ne_bytes();

            if c16_64 == c64_64 {
                FloatType::Float16(v16)
            } else if c32_64 == c64_64 {
                FloatType::Float32(v32)
            } else {
                FloatType::Float64(v64)
            }
        }
        FloatType::Float32(v32) => {
            let v16: f16 = f16::from_f32(v32);

            let c32_32 = v32.to_ne_bytes();
            let c16_32 = (f16::to_f32(v16)).to_ne_bytes();

            if c16_32 == c32_32 {
                FloatType::Float16(v16)
            } else {
                FloatType::Float32(v32)
            }
        }
        _ => v,
    }
}
/***************************************************************************************************
 * Encoding context for Array, Map
 **************************************************************************************************/
pub enum ContextType {
    Array,
    Map,
    Tag,
}

/// The `EncodeContext` structure encodes the information needed to encode a sequence of
/// `EncodeItem`s on an `EncodeBuffer` and fix up the composite MT/AI/Length information.
pub struct EncodeContext {
    pub(self) context_type: ContextType,
    pub(self) no_of_items_before_ctx: usize, // Number of items in buffer before the array starts
    pub(self) mt_ai_index: usize,            // Index in buffer of the MT/AI for the array
    pub(self) ctx_encode_start: usize,       // Index
}

impl EncodeContext {
    /// Construct a new context structure which can later be used in a start/finalize context pair
    /// of function calls.
    pub fn new() -> Self {
        EncodeContext {
            context_type: ContextType::Array,
            no_of_items_before_ctx: 0,
            mt_ai_index: 0,
            ctx_encode_start: 0,
        }
    }
}

/***************************************************************************************************
 * Private helper functions
 **************************************************************************************************/

#[inline]
#[cfg_attr(feature = "trace", trace)]
fn encode_item_simple<'f, 'b>(
    buf: &'f mut EncodeBuffer<'b>,
    v: u8,
) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
    encode_unsigned(buf, v as u64)?;
    match v {
        24..=31 => return Err(CBORError::MalformedEncoding),
        _ => buf.set_mt(MT_SIMPLE),
    }
    if v < 32 {
        // Encoded on one byte
        buf.update_index(1)?;
        Ok(buf)
    } else {
        // Encoded on two bytes
        buf.update_index(2)?;
        Ok(buf)
    }
}

/// Encode an unsigned integer value on `buf` starting at `start_index`.
///
/// Integer values are always encoded using preferred serialization as defined in RFC8949.
/// The index just after the serialized value is returned if serialization was successful.
/// `Err(CBORError::EndOfBuffer` is returned if there is no space for serialization.
///
/// The caller is expected to set the Major Type, if required, after the function returns.
#[cfg_attr(feature = "trace", trace)]
fn encode_unsigned(buf: &mut EncodeBuffer, v: u64) -> Result<MtUnset, CBORError> {
    let vs = v.to_be_bytes();
    if v < 24 {
        // Encode on the AI bits
        buf.write_byte_at_offset(0, vs[7])?;
        Ok(MtUnset(0))
    } else if v <= u8::MAX as u64 {
        buf.write_byte_at_offset(0, PAYLOAD_ONE_BYTE)?;
        buf.write_byte_at_offset(1, vs[7])?;
        Ok(MtUnset(1))
    } else if v <= u16::MAX as u64 {
        buf.write_byte_at_offset(0, PAYLOAD_TWO_BYTES)?;
        buf.write_slice_at_offset(1, &vs[6..=7])?;
        Ok(MtUnset(2))
    } else if v <= u32::MAX as u64 {
        buf.write_byte_at_offset(0, PAYLOAD_FOUR_BYTES)?;
        buf.write_slice_at_offset(1, &vs[4..=7])?;
        Ok(MtUnset(4))
    } else {
        buf.write_byte_at_offset(0, PAYLOAD_EIGHT_BYTES)?;
        buf.write_slice_at_offset(1, &vs[0..=7])?;
        Ok(MtUnset(8))
    }
}

/// Encode a `DateTime<FixedOffset>` on `buf`, starting at the (internal) `start_index`.
/// The index just after the serialized value is returned if serialization was successful.
/// `Err(CBORError::EndOfBuffer` is returned if there is no space for serialization.
#[cfg(feature = "std_tags")]
fn encode_date_time<'f, 'b>(
    buf: &'f mut EncodeBuffer<'b>,
    date: &DateTime<FixedOffset>,
) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
    let date_string: String = date.to_rfc3339();
    let _tag_len = buf.tag_next_item(0)?;
    let _ = buf.insert(&Tstr(date_string.as_str()))?;
    Ok(buf)
}

/// Encode a `DateTime<FixedOffset>` on `buf`, starting at the (internal) `start_index`.
/// The index just after the serialized value is returned if serialization was successful.
/// `Err(CBORError::EndOfBuffer` is returned if there is no space for serialization.
#[cfg(feature = "std_tags")]
fn encode_epoch<'f, 'b>(
    buf: &'f mut EncodeBuffer<'b>,
    secs: i64,
) -> Result<&'f mut EncodeBuffer<'b>, CBORError> {
    let _tag_len = buf.tag_next_item(1)?;
    let _val_len = if secs < 0 {
        let neg_secs = (-1 - secs) as u64;
        buf.insert(&NInt(neg_secs))?
    } else {
        buf.insert(&UInt(secs as u64))?
    };
    Ok(buf)
}
