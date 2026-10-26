/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use crate::{
    DecodeError, MAJOR_ARRAY, MAJOR_BYTES, MAJOR_MAP, MAJOR_NEGATIVE, MAJOR_SIMPLE, MAJOR_TAG,
    MAJOR_TEXT, MAJOR_UNSIGNED, MAX_DECODE_DEPTH,
};

#[cfg(feature = "diagnostic")]
use crate::DiagnosticError;

#[cfg(feature = "standard_tags")]
use crate::standard_tags;

use core::convert::TryFrom;
use core::str;

#[cfg(feature = "diagnostic")]
use std::vec;
#[cfg(feature = "diagnostic")]
use std::vec::Vec;

#[cfg(all(feature = "diagnostic", feature = "spans"))]
use core::fmt::Write as _;

/// Maps a CBOR item's byte range (relative to the decoded buffer) to the byte
/// range of the EDN text written for it, as produced by
/// [`write_diagnostic_pretty_all_fmt_with_spans`](Decoder::write_diagnostic_pretty_all_fmt_with_spans).
#[cfg(all(feature = "diagnostic", feature = "spans"))]
type DiagnosticSpan = (core::ops::Range<usize>, core::ops::Range<usize>);

/******************************************************************************
 * Decoder implementation
 ******************************************************************************/

/// Decoded CBOR data item.
#[derive(Debug, Clone, PartialEq)]
pub enum Item<'a> {
    /// CBOR major type 0 (unsigned integer).
    Unsigned(u64),
    /// CBOR major type 1 (negative integer, value = -1 - n).
    Negative(i64),
    /// CBOR major type 2 (byte string).
    Bytes(&'a [u8]),
    /// CBOR major type 3 (UTF-8 text string).
    Text(&'a str),
    /// CBOR major type 4 (array).
    Array(Array<'a>),
    /// CBOR major type 5 (map).
    Map(Map<'a>),
    /// CBOR major type 6 (tagged item).
    Tag(TaggedItem<'a>),
    /// CBOR simple false/true (0xf4/0xf5).
    Bool(bool),
    /// CBOR null (0xf6).
    Null,
    /// CBOR undefined (0xf7).
    Undefined,
    /// CBOR simple value (0..19, 32..255).
    Simple(u8),
    /// CBOR floating-point value (requires feature `float`).
    #[cfg(feature = "float")]
    Float(Float),
}

impl TryFrom<Item<'_>> for u64 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for u32 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => u32::try_from(v).map_err(|_| DecodeError::OutOfRange),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for u16 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => u16::try_from(v).map_err(|_| DecodeError::OutOfRange),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for u8 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => u8::try_from(v).map_err(|_| DecodeError::OutOfRange),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for i64 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => i64::try_from(v).map_err(|_| DecodeError::OutOfRange),
            Item::Negative(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for i32 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => i32::try_from(v).map_err(|_| DecodeError::OutOfRange),
            Item::Negative(v) => i32::try_from(v).map_err(|_| DecodeError::OutOfRange),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for i16 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => i16::try_from(v).map_err(|_| DecodeError::OutOfRange),
            Item::Negative(v) => i16::try_from(v).map_err(|_| DecodeError::OutOfRange),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for i8 {
    type Error = DecodeError;

    fn try_from(item: Item) -> Result<Self, Self::Error> {
        match item {
            Item::Unsigned(v) => i8::try_from(v).map_err(|_| DecodeError::OutOfRange),
            Item::Negative(v) => i8::try_from(v).map_err(|_| DecodeError::OutOfRange),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl<'a> TryFrom<Item<'a>> for &'a [u8] {
    type Error = DecodeError;

    fn try_from(item: Item<'a>) -> Result<Self, Self::Error> {
        match item {
            Item::Bytes(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl<'a> TryFrom<Item<'a>> for &'a str {
    type Error = DecodeError;

    fn try_from(item: Item<'a>) -> Result<Self, Self::Error> {
        match item {
            Item::Text(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl<'a> TryFrom<Item<'a>> for Array<'a> {
    type Error = DecodeError;

    fn try_from(value: Item<'a>) -> Result<Self, Self::Error> {
        match value {
            Item::Array(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl<'a> TryFrom<Item<'a>> for Map<'a> {
    type Error = DecodeError;

    fn try_from(value: Item<'a>) -> Result<Self, Self::Error> {
        match value {
            Item::Map(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl<'a> TryFrom<Item<'a>> for TaggedItem<'a> {
    type Error = DecodeError;

    fn try_from(value: Item<'a>) -> Result<Self, Self::Error> {
        match value {
            Item::Tag(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

impl TryFrom<Item<'_>> for bool {
    type Error = DecodeError;

    fn try_from(value: Item) -> Result<Self, Self::Error> {
        match value {
            Item::Bool(v) => Ok(v),
            _ => Err(DecodeError::UnexpectedItem),
        }
    }
}

/// CBOR decoder over a byte slice.
///
/// `Decoder` provides two API families:
///
/// - **Typed direct methods** — [`uint`](Decoder::uint), [`neg_int`](Decoder::neg_int),
///   [`bytes`](Decoder::bytes), [`text`](Decoder::text), [`bool`](Decoder::bool),
///   [`null`](Decoder::null), [`array_len`](Decoder::array_len),
///   [`map_len`](Decoder::map_len): decode a single item directly into a Rust type,
///   returning an error if the next CBOR item has the wrong major type. These are
///   the preferred methods for use inside [`Decode`] implementations.
///
/// - **`next_*` convenience methods** — [`next`](Decoder::next),
///   [`next_uint`](Decoder::next_uint), [`next_bytes`](Decoder::next_bytes), etc.:
///   decode the next item into a [`Item`] enum (or an extracted field thereof) without
///   consuming it twice. Use these when the caller does not know the type in advance,
///   or when iterating mixed-type arrays.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{Decoder, Item, DecodeError};
///
/// fn main() -> Result<(), DecodeError> {
///   let data = [0x01];
///   let mut dec = Decoder::new(&data);
///   let item = dec.decode()?;
///   assert!(matches!(item, Item::Unsigned(1)));
///   Ok(())
/// }
/// ```
pub struct Decoder<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    /// Create a new decoder for the provided input.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0x01];
    /// let _dec = Decoder::new(&data);
    /// ```
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Current cursor position in bytes consumed from the input.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0x01];
    /// let mut dec = Decoder::new(&data);
    /// let _ = dec.next();
    /// assert_eq!(dec.pos(), 1);
    /// ```
    #[must_use]
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Returns `true` if no bytes have been consumed yet.
    #[must_use]
    pub const fn at_start(&self) -> bool {
        self.pos == 0
    }

    /// Decode exactly one item and reject trailing bytes.
    ///
    /// # Examples
    ///
    /// In this example, we want to collect items in the form of CBOR [`Item`] types.
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Item, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    ///
    /// let data = [0x01];
    /// let mut dec = Decoder::new(&data);
    /// let item = dec.decode()?;
    /// assert!(matches!(item, Item::Unsigned(1)));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// In this second example, we take advantage of the fact that there are implementations
    /// of [`TryFrom`] to perform conversions into Rust types.
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// use core::convert::TryFrom;
    ///
    /// # fn main() -> Result<(), DecodeError> {
    ///   let data = [0x18, 0x64];
    ///   let mut dec = Decoder::new(&data);
    ///   let value: i32 = dec.decode()?.try_into()?;
    ///   assert_eq!(value, 100i32);
    /// # Ok(())
    /// # }
    /// ```
    ///
    #[must_use = "returns the decoded item"]
    pub fn decode(&mut self) -> Result<Item<'a>, DecodeError> {
        let item = self.next()?;
        if self.pos != self.buf.len() {
            return Err(DecodeError::TrailingBytes);
        }
        Ok(item)
    }

    /// Decode exactly one item and reject trailing bytes, also returning the
    /// byte range (relative to this decoder's own buffer) that the item
    /// occupies.
    ///
    /// Requires the `spans` feature.
    #[cfg(feature = "spans")]
    #[must_use = "returns the decoded item and its span"]
    pub fn decode_with_span(&mut self) -> Result<(Item<'a>, core::ops::Range<usize>), DecodeError> {
        let (item, span) = self.next_with_span()?;
        if self.pos != self.buf.len() {
            return Err(DecodeError::TrailingBytes);
        }
        Ok((item, span))
    }

    /// Decode the next item into a concrete type.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    ///
    /// let data = [0x18, 0x2a];
    /// let mut dec = Decoder::new(&data);
    /// let value: u64 = dec.decode_item()?;
    /// assert_eq!(value, 42);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded value"]
    pub fn decode_item<T: Decode<'a>>(&mut self) -> Result<T, DecodeError> {
        T::decode(self)
    }

    /// Decode the next item from the input.
    ///
    /// Returns the item as a generic [`Item`] enum. Prefer the typed direct methods
    /// ([`uint`](Decoder::uint), [`bytes`](Decoder::bytes), etc.) inside [`Decode`]
    /// implementations where the expected type is known. Use `next()` when the item
    /// type must be determined at runtime, for example when iterating a heterogeneous
    /// array.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Item};
    ///
    /// let data = [0x01, 0x02];
    /// let mut dec = Decoder::new(&data);
    /// assert!(matches!(dec.next().unwrap(), Item::Unsigned(1)));
    /// assert!(matches!(dec.next().unwrap(), Item::Unsigned(2)));
    /// ```
    #[allow(clippy::should_implement_trait)]
    #[must_use = "returns the decoded item"]
    pub fn next(&mut self) -> Result<Item<'a>, DecodeError> {
        let (major, addl, value) = self.read_header_full()?;
        match major {
            MAJOR_UNSIGNED => Ok(Item::Unsigned(value)),
            MAJOR_NEGATIVE => {
                if value > i64::MAX as u64 {
                    return Err(DecodeError::InvalidAdditionalInfo);
                }
                Ok(Item::Negative(!(value as i64)))
            }
            MAJOR_BYTES => Ok(Item::Bytes(self.take_bytes(value)?)),
            MAJOR_TEXT => {
                let bytes = self.take_bytes(value)?;
                let text = str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
                Ok(Item::Text(text))
            }
            MAJOR_ARRAY => {
                let start = self.pos;
                self.skip_items(value)?;
                let end = self.pos;
                Ok(Item::Array(Array {
                    data: &self.buf[start..end],
                    len: value,
                }))
            }
            MAJOR_MAP => {
                let total = value
                    .checked_mul(2)
                    .ok_or(DecodeError::InvalidAdditionalInfo)?;
                let start = self.pos;
                self.skip_items(total)?;
                let end = self.pos;
                Ok(Item::Map(Map {
                    data: &self.buf[start..end],
                    len: value,
                }))
            }
            MAJOR_TAG => {
                let start = self.pos;
                self.skip_item()?;
                let end = self.pos;
                Ok(Item::Tag(TaggedItem {
                    tag: value,
                    data: &self.buf[start..end],
                }))
            }
            MAJOR_SIMPLE => match addl {
                0..=19 => Ok(Item::Simple(addl)),
                20 => Ok(Item::Bool(false)),
                21 => Ok(Item::Bool(true)),
                22 => Ok(Item::Null),
                23 => Ok(Item::Undefined),
                24 => {
                    let simple = value as u8;
                    if (0..=31).contains(&simple) {
                        Err(DecodeError::InvalidAdditionalInfo)
                    } else {
                        Ok(Item::Simple(simple))
                    }
                }
                25 => {
                    #[cfg(feature = "float")]
                    {
                        Ok(Item::Float(Float::F16(value as u16)))
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        Err(DecodeError::FloatDisabled)
                    }
                }
                26 => {
                    #[cfg(feature = "float")]
                    {
                        Ok(Item::Float(Float::F32(value as u32)))
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        Err(DecodeError::FloatDisabled)
                    }
                }
                27 => {
                    #[cfg(feature = "float")]
                    {
                        Ok(Item::Float(Float::F64(value)))
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        Err(DecodeError::FloatDisabled)
                    }
                }
                _ => Err(DecodeError::InvalidAdditionalInfo),
            },
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item from the input, also returning the byte range
    /// (relative to this decoder's own buffer) that the item occupies.
    ///
    /// Requires the `spans` feature. Useful for tooling that needs to map a
    /// decoded [`Item`] back to the exact bytes that produced it (e.g. a
    /// hex-view highlighter or a CDDL validator's trace log).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0x01, 0x02];
    /// let mut dec = Decoder::new(&data);
    /// let (_, span) = dec.next_with_span().unwrap();
    /// assert_eq!(span, 0..1);
    /// ```
    #[cfg(feature = "spans")]
    #[must_use = "returns the decoded item and its span"]
    pub fn next_with_span(&mut self) -> Result<(Item<'a>, core::ops::Range<usize>), DecodeError> {
        let start = self.pos;
        let item = self.next()?;
        let end = self.pos;
        Ok((item, start..end))
    }

    /// Decode the next item as an unsigned integer.
    ///
    /// Convenience wrapper around [`next`](Decoder::next) that extracts
    /// `Item::Unsigned`. For use inside [`Decode`] implementations prefer
    /// [`uint`](Decoder::uint) which avoids the intermediate [`Item`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0x18, 0x2a];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.next_uint().unwrap(), 42);
    /// ```
    #[must_use = "returns the decoded value"]
    pub fn next_uint(&mut self) -> Result<u64, DecodeError> {
        match self.next()? {
            Item::Unsigned(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as a signed integer.
    ///
    /// Accepts both CBOR unsigned and negative major types and returns an `i64`.
    /// Convenience wrapper around [`next`](Decoder::next). For use inside
    /// [`Decode`] implementations see the [`Decode`] impls on `i64` and `i32`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0x20];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.next_int().unwrap(), -1);
    /// ```
    #[must_use = "returns the decoded value"]
    pub fn next_int(&mut self) -> Result<i64, DecodeError> {
        match self.next()? {
            Item::Unsigned(value) => {
                if value > i64::MAX as u64 {
                    return Err(DecodeError::InvalidAdditionalInfo);
                }
                Ok(value as i64)
            }
            Item::Negative(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as a byte string.
    ///
    /// Convenience wrapper around [`next`](Decoder::next) that extracts
    /// `Item::Bytes`. For use inside [`Decode`] implementations prefer
    /// [`bytes`](Decoder::bytes) which avoids the intermediate [`Item`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0x43, 0x01, 0x02, 0x03];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.next_bytes().unwrap(), &[1, 2, 3]);
    /// ```
    #[must_use = "returns the decoded value"]
    pub fn next_bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        match self.next()? {
            Item::Bytes(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as a text string.
    ///
    /// Convenience wrapper around [`next`](Decoder::next) that extracts
    /// `Item::Text`. For use inside [`Decode`] implementations prefer
    /// [`text`](Decoder::text) which avoids the intermediate [`Item`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0x61, 0x61];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.next_text().unwrap(), "a");
    /// ```
    #[must_use = "returns the decoded value"]
    pub fn next_text(&mut self) -> Result<&'a str, DecodeError> {
        match self.next()? {
            Item::Text(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as an array.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Item};
    ///
    /// let data = [0x82, 0x01, 0x02];
    /// let mut dec = Decoder::new(&data);
    /// let array = dec.next_array().unwrap();
    /// let vals: Vec<u64> = array.iter()
    ///     .map(|r| match r.unwrap() { Item::Unsigned(v) => v, _ => 0 })
    ///     .collect();
    /// assert_eq!(vals, vec![1, 2]);
    /// ```
    #[must_use = "returns the decoded array"]
    pub fn next_array(&mut self) -> Result<Array<'a>, DecodeError> {
        match self.next()? {
            Item::Array(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as a map.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Item};
    ///
    /// let data = [0xa1, 0x01, 0x02];
    /// let mut dec = Decoder::new(&data);
    /// let map = dec.next_map().unwrap();
    /// let pairs: Vec<(u64, u64)> = map.iter()
    ///     .map(|r| {
    ///         let (k, v) = r.unwrap();
    ///         let k = match k { Item::Unsigned(v) => v, _ => 0 };
    ///         let v = match v { Item::Unsigned(v) => v, _ => 0 };
    ///         (k, v)
    ///     })
    ///     .collect();
    /// assert_eq!(pairs, vec![(1, 2)]);
    /// ```
    #[must_use = "returns the decoded map"]
    pub fn next_map(&mut self) -> Result<Map<'a>, DecodeError> {
        match self.next()? {
            Item::Map(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as a tag.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Item};
    ///
    /// let data = [0xc1, 0x01];
    /// let mut dec = Decoder::new(&data);
    /// let tagged = dec.next_tag().unwrap();
    /// assert_eq!(tagged.tag, 1);
    /// let inner = tagged.item().unwrap();
    /// assert!(matches!(inner, Item::Unsigned(1)));
    /// ```
    #[must_use = "returns the decoded tagged item"]
    pub fn next_tag(&mut self) -> Result<TaggedItem<'a>, DecodeError> {
        match self.next()? {
            Item::Tag(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as a standard tag.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "standard_tags")]
    /// # {
    /// use tps_minicbor::Decoder;
    /// use tps_minicbor::standard_tags::StandardTag;
    ///
    /// let data = [0xc0, 0x61, 0x61];
    /// let mut dec = Decoder::new(&data);
    /// let tag = dec.next_standard_tag().unwrap();
    /// assert!(matches!(tag, StandardTag::DateTime(_)));
    /// # }
    /// ```
    #[cfg(feature = "standard_tags")]
    pub fn next_standard_tag(&mut self) -> Result<standard_tags::StandardTag<'a>, DecodeError> {
        standard_tags::StandardTag::decode(self)
    }

    /// Decode the next item as a simple value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0xf0];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.next_simple().unwrap(), 16);
    /// ```
    #[must_use = "returns the decoded simple value"]
    pub fn next_simple(&mut self) -> Result<u8, DecodeError> {
        match self.next()? {
            Item::Simple(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as undefined.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Decoder;
    ///
    /// let data = [0xf7];
    /// let mut dec = Decoder::new(&data);
    /// dec.next_undefined().unwrap();
    /// ```
    #[must_use = "returns an error if the next item is not undefined"]
    pub fn next_undefined(&mut self) -> Result<(), DecodeError> {
        match self.next()? {
            Item::Undefined => Ok(()),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as a floating-point value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Float};
    ///
    /// let data = [0xfa, 0x47, 0xc3, 0x50, 0x00];
    /// let mut dec = Decoder::new(&data);
    /// assert!(matches!(dec.next_float().unwrap(), Float::F32(_)));
    /// ```
    #[cfg(feature = "float")]
    #[must_use = "returns the decoded float value"]
    pub fn next_float(&mut self) -> Result<Float, DecodeError> {
        match self.next()? {
            Item::Float(value) => Ok(value),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as an array and return its element count.
    ///
    /// This is the typed counterpart to [`Decoder::next_array`] for use inside
    /// [`Decode`] implementations where callers manage the iteration themselves.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0x82, 0x01, 0x02];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.array_len()?, 2);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded array length"]
    pub fn array_len(&mut self) -> Result<u64, DecodeError> {
        let (major, len) = self.read_header()?;
        if major != MAJOR_ARRAY {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        Ok(len)
    }

    /// Decode the next item as a map and return its pair count.
    ///
    /// This is the typed counterpart to [`Decoder::next_map`] for use inside
    /// [`Decode`] implementations where callers manage the iteration themselves.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0xa2, 0x01, 0x02, 0x03, 0x04];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.map_len()?, 2);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded map length"]
    pub fn map_len(&mut self) -> Result<u64, DecodeError> {
        let (major, len) = self.read_header()?;
        if major != MAJOR_MAP {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        Ok(len)
    }

    /// Decode the next item as an unsigned integer.
    ///
    /// This is the typed counterpart to [`Decoder::next_uint`] for use inside
    /// [`Decode`] implementations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0x18, 0x2a];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.uint()?, 42);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded value"]
    pub fn uint(&mut self) -> Result<u64, DecodeError> {
        let (major, value) = self.read_header()?;
        if major != MAJOR_UNSIGNED {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        Ok(value)
    }

    /// Decode the next item as a negative integer.
    ///
    /// Returns an `i64` in the range `[-2^63, -1]`. Values outside this range
    /// (i.e. CBOR negatives larger than `i64::MAX + 1`) yield [`DecodeError::OutOfRange`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0x20];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.neg_int()?, -1);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded value"]
    pub fn neg_int(&mut self) -> Result<i64, DecodeError> {
        let (major, value) = self.read_header()?;
        if major != MAJOR_NEGATIVE {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        if value > i64::MAX as u64 {
            return Err(DecodeError::OutOfRange);
        }
        Ok(!(value as i64))
    }

    /// Decode the next item as a byte string.
    ///
    /// This is the typed counterpart to [`Decoder::next_bytes`] for use inside
    /// [`Decode`] implementations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0x43, 0x01, 0x02, 0x03];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.bytes()?, &[1, 2, 3]);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded byte slice"]
    pub fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        self.bytes_with_major(MAJOR_BYTES)
    }

    /// Decode the next item as a text string.
    ///
    /// This is the typed counterpart to [`Decoder::next_text`] for use inside
    /// [`Decode`] implementations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0x61, 0x61];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.text()?, "a");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded text"]
    pub fn text(&mut self) -> Result<&'a str, DecodeError> {
        let bytes = self.bytes_with_major(MAJOR_TEXT)?;
        str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)
    }

    fn bytes_with_major(&mut self, major_expected: u8) -> Result<&'a [u8], DecodeError> {
        let (major, len) = self.read_header()?;
        if major != major_expected {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        let len = len as usize;
        let end = self.pos.checked_add(len).ok_or(DecodeError::UnexpectedEof)?;
        if end > self.buf.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    /// Decode the next item as a boolean.
    ///
    /// This is the typed counterpart to calling [`Decoder::next`] and matching
    /// `Item::Bool` for use inside [`Decode`] implementations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0xf5];
    /// let mut dec = Decoder::new(&data);
    /// assert_eq!(dec.bool()?, true);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded boolean"]
    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        let (major, value) = self.read_header()?;
        if major != MAJOR_SIMPLE {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        match value {
            20 => Ok(false),
            21 => Ok(true),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }

    /// Decode the next item as CBOR null.
    ///
    /// This is the typed counterpart to calling [`Decoder::next`] and matching
    /// `Item::Null` for use inside [`Decode`] implementations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0xf6];
    /// let mut dec = Decoder::new(&data);
    /// dec.null()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns an error if the next item is not null"]
    pub fn null(&mut self) -> Result<(), DecodeError> {
        let (major, value) = self.read_header()?;
        if major != MAJOR_SIMPLE || value != 22 {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        Ok(())
    }

    fn read_header(&mut self) -> Result<(u8, u64), DecodeError> {
        let (major, _, value) = self.read_header_full()?;
        Ok((major, value))
    }

    fn read_header_full(&mut self) -> Result<(u8, u8, u64), DecodeError> {
        let (major, addl, value, consumed) = parse_header(self.buf, self.pos)?;
        self.pos += consumed;
        Ok((major, addl, value))
    }

    fn peek_major(&self) -> Result<u8, DecodeError> {
        if self.pos >= self.buf.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        Ok(self.buf[self.pos] >> 5)
    }

    fn take_bytes(&mut self, len: u64) -> Result<&'a [u8], DecodeError> {
        let len = usize::try_from(len).map_err(|_| DecodeError::InvalidAdditionalInfo)?;
        let end = self.pos.checked_add(len).ok_or(DecodeError::UnexpectedEof)?;
        if end > self.buf.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn skip_bytes(&mut self, len: u64) -> Result<(), DecodeError> {
        let len = usize::try_from(len).map_err(|_| DecodeError::InvalidAdditionalInfo)?;
        let end = self.pos.checked_add(len).ok_or(DecodeError::UnexpectedEof)?;
        if end > self.buf.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        self.pos = end;
        Ok(())
    }

    fn skip_items(&mut self, count: u64) -> Result<(), DecodeError> {
        self.skip_items_at_depth(count, 0)
    }

    fn skip_items_at_depth(&mut self, count: u64, depth: usize) -> Result<(), DecodeError> {
        for _ in 0..count {
            self.skip_item_at_depth(depth)?;
        }
        Ok(())
    }

    fn skip_item(&mut self) -> Result<(), DecodeError> {
        self.skip_item_at_depth(0)
    }

    fn skip_item_at_depth(&mut self, depth: usize) -> Result<(), DecodeError> {
        if depth >= MAX_DECODE_DEPTH {
            return Err(DecodeError::NestingTooDeep);
        }
        let (major, addl, value) = self.read_header_full()?;
        match major {
            MAJOR_UNSIGNED | MAJOR_NEGATIVE => Ok(()),
            MAJOR_BYTES | MAJOR_TEXT => self.skip_bytes(value),
            MAJOR_ARRAY => self.skip_items_at_depth(value, depth + 1),
            MAJOR_MAP => {
                let total = value
                    .checked_mul(2)
                    .ok_or(DecodeError::InvalidAdditionalInfo)?;
                self.skip_items_at_depth(total, depth + 1)
            }
            MAJOR_TAG => self.skip_item_at_depth(depth + 1),
            MAJOR_SIMPLE => match addl {
                0..=23 => Ok(()),
                24 => {
                    let simple = value as u8;
                    if (0..=31).contains(&simple) {
                        return Err(DecodeError::InvalidAdditionalInfo);
                    }
                    Ok(())
                }
                25..=27 => {
                    #[cfg(feature = "float")]
                    {
                        Ok(())
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        Err(DecodeError::FloatDisabled)
                    }
                }
                _ => Err(DecodeError::InvalidAdditionalInfo),
            },
            _ => Err(DecodeError::InvalidAdditionalInfo),
        }
    }
    /// Write CBOR diagnostic notation for the next item.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic(&self, out: &mut [u8]) -> Result<usize, DiagnosticError> {
        let mut writer = SliceWriter::new(out);
        let res = self.write_diagnostic_fmt(&mut writer);
        if writer.overflowed {
            return Err(DiagnosticError::OutOfSpace);
        }
        res?;
        Ok(writer.len())
    }

    /// Write CBOR diagnostic notation for the next item and reject trailing bytes.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic_all(&self, out: &mut [u8]) -> Result<usize, DiagnosticError> {
        let mut writer = SliceWriter::new(out);
        let res = self.write_diagnostic_all_fmt(&mut writer);
        if writer.overflowed {
            return Err(DiagnosticError::OutOfSpace);
        }
        res?;
        Ok(writer.len())
    }

    /// Write indented, multi-line CBOR diagnostic notation for the next item.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic_pretty(&self, out: &mut [u8]) -> Result<usize, DiagnosticError> {
        let mut writer = SliceWriter::new(out);
        let res = self.write_diagnostic_pretty_fmt(&mut writer);
        if writer.overflowed {
            return Err(DiagnosticError::OutOfSpace);
        }
        res?;
        Ok(writer.len())
    }

    /// Write indented, multi-line CBOR diagnostic notation for the next item
    /// and reject trailing bytes.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic_pretty_all(&self, out: &mut [u8]) -> Result<usize, DiagnosticError> {
        let mut writer = SliceWriter::new(out);
        let res = self.write_diagnostic_pretty_all_fmt(&mut writer);
        if writer.overflowed {
            return Err(DiagnosticError::OutOfSpace);
        }
        res?;
        Ok(writer.len())
    }

    /// Write CBOR diagnostic notation for the next item to a formatter.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic_fmt<W: core::fmt::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut diag = DiagnosticDecoder::new(self.buf, self.pos);
        diag.write_item(out, None)
    }

    /// Write CBOR diagnostic notation for the next item and reject trailing bytes.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic_all_fmt<W: core::fmt::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut diag = DiagnosticDecoder::new(self.buf, self.pos);
        diag.write_item(out, None)?;
        if diag.pos != diag.buf.len() {
            return Err(DiagnosticError::Decode(DecodeError::TrailingBytes));
        }
        Ok(())
    }

    /// Write indented, multi-line CBOR diagnostic notation for the next item
    /// to a formatter. Array and map bodies are broken one item per line and
    /// indented two spaces per nesting level; scalars and tags are unaffected.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic_pretty_fmt<W: core::fmt::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut diag = DiagnosticDecoder::new(self.buf, self.pos);
        diag.write_item(out, Some(0))
    }

    /// Write indented, multi-line CBOR diagnostic notation for the next item
    /// and reject trailing bytes.
    #[cfg(feature = "diagnostic")]
    pub fn write_diagnostic_pretty_all_fmt<W: core::fmt::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut diag = DiagnosticDecoder::new(self.buf, self.pos);
        diag.write_item(out, Some(0))?;
        if diag.pos != diag.buf.len() {
            return Err(DiagnosticError::Decode(DecodeError::TrailingBytes));
        }
        Ok(())
    }

    /// Write indented, multi-line CBOR diagnostic notation for the next item,
    /// rejecting trailing bytes, and return a table mapping each visited
    /// item's CBOR byte range to its corresponding byte range in the written
    /// EDN text. The written text is byte-identical to
    /// [`write_diagnostic_pretty_all_fmt`](Decoder::write_diagnostic_pretty_all_fmt); this method purely adds an
    /// additional observation of the same traversal.
    #[cfg(all(feature = "diagnostic", feature = "spans"))]
    pub fn write_diagnostic_pretty_all_fmt_with_spans<W: core::fmt::Write>(
        &self,
        out: &mut W,
        pretty: Option<usize>,
    ) -> Result<Vec<DiagnosticSpan>, DiagnosticError> {
        let mut diag = DiagnosticDecoder::new(self.buf, self.pos);
        let mut counting = CountingWriter::new(out);
        let mut spans = Vec::new();
        diag.write_item_with_spans(&mut counting, pretty, &mut spans)?;
        if diag.pos != diag.buf.len() {
            return Err(DiagnosticError::Decode(DecodeError::TrailingBytes));
        }
        Ok(spans)
    }

    /// Write CBOR diagnostic notation for the next item to a std::io::Write sink.
    #[cfg(all(feature = "diagnostic", feature = "std"))]
    pub fn write_diagnostic_io<W: std::io::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut writer = IoWriter::new(out);
        self.write_diagnostic_fmt(&mut writer)
    }

    /// Write CBOR diagnostic notation for the next item and reject trailing bytes.
    #[cfg(all(feature = "diagnostic", feature = "std"))]
    pub fn write_diagnostic_all_io<W: std::io::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut writer = IoWriter::new(out);
        self.write_diagnostic_all_fmt(&mut writer)
    }

    /// Write indented, multi-line CBOR diagnostic notation for the next item
    /// to a std::io::Write sink.
    #[cfg(all(feature = "diagnostic", feature = "std"))]
    pub fn write_diagnostic_pretty_io<W: std::io::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut writer = IoWriter::new(out);
        self.write_diagnostic_pretty_fmt(&mut writer)
    }

    /// Write indented, multi-line CBOR diagnostic notation for the next item
    /// and reject trailing bytes.
    #[cfg(all(feature = "diagnostic", feature = "std"))]
    pub fn write_diagnostic_pretty_all_io<W: std::io::Write>(
        &self,
        out: &mut W,
    ) -> Result<(), DiagnosticError> {
        let mut writer = IoWriter::new(out);
        self.write_diagnostic_pretty_all_fmt(&mut writer)
    }
}

// Parse a CBOR item header from `buf` starting at `pos`.
// Returns (major, addl, value, bytes_consumed).
fn parse_header(buf: &[u8], pos: usize) -> Result<(u8, u8, u64, usize), DecodeError> {
    if pos >= buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let first = buf[pos];
    let major = first >> 5;
    let addl = first & 0x1f;
    let (value, extra) = match addl {
        0..=23 => (addl as u64, 0usize),
        24 => {
            if pos + 1 >= buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            (buf[pos + 1] as u64, 1)
        }
        25 => {
            if pos + 2 >= buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let v = u16::from_be_bytes([buf[pos + 1], buf[pos + 2]]) as u64;
            (v, 2)
        }
        26 => {
            if pos + 4 >= buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let v =
                u32::from_be_bytes([buf[pos + 1], buf[pos + 2], buf[pos + 3], buf[pos + 4]]) as u64;
            (v, 4)
        }
        27 => {
            if pos + 8 >= buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let v = u64::from_be_bytes([
                buf[pos + 1],
                buf[pos + 2],
                buf[pos + 3],
                buf[pos + 4],
                buf[pos + 5],
                buf[pos + 6],
                buf[pos + 7],
                buf[pos + 8],
            ]);
            (v, 8)
        }
        31 => return Err(DecodeError::IndefiniteLength),
        _ => return Err(DecodeError::InvalidAdditionalInfo),
    };
    Ok((major, addl, value, 1 + extra))
}

/// Trait for types that can decode themselves from a CBOR [`Decoder`].
///
/// Implement this trait to allow your type to be decoded directly via
/// [`Decoder::decode_item`] or the free [`decode`] function. All primitive
/// Rust integer types, `bool`, `&str`, `&[u8]`, and the float types (under
/// the `float` feature) have built-in implementations.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{Decode, Decoder, DecodeError};
///
/// struct Point { x: i32, y: i32 }
///
/// impl<'a> Decode<'a> for Point {
///     fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
///         let len = d.array_len()?;
///         if len != 2 { return Err(DecodeError::InvalidAdditionalInfo); }
///         let x = i32::decode(d)?;
///         let y = i32::decode(d)?;
///         Ok(Point { x, y })
///     }
/// }
/// ```
pub trait Decode<'a>: Sized {
    /// Decode a value of this type from `d`.
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError>;
}

impl<'a> Decode<'a> for u64 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        d.uint()
    }
}

impl<'a> Decode<'a> for i64 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        if d.peek_major()? == MAJOR_UNSIGNED {
            let v = d.uint()?;
            i64::try_from(v).map_err(|_| DecodeError::OutOfRange)
        } else {
            d.neg_int()
        }
    }
}

impl<'a> Decode<'a> for u32 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        let v = d.uint()?;
        u32::try_from(v).map_err(|_| DecodeError::OutOfRange)
    }
}

impl<'a> Decode<'a> for u16 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        let v = d.uint()?;
        u16::try_from(v).map_err(|_| DecodeError::OutOfRange)
    }
}

impl<'a> Decode<'a> for u8 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        let v = d.uint()?;
        u8::try_from(v).map_err(|_| DecodeError::OutOfRange)
    }
}

impl<'a> Decode<'a> for i32 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        if d.peek_major()? == MAJOR_UNSIGNED {
            let v = d.uint()?;
            i32::try_from(v).map_err(|_| DecodeError::OutOfRange)
        } else {
            let v = d.neg_int()?;
            i32::try_from(v).map_err(|_| DecodeError::OutOfRange)
        }
    }
}

impl<'a> Decode<'a> for i16 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        if d.peek_major()? == MAJOR_UNSIGNED {
            let v = d.uint()?;
            i16::try_from(v).map_err(|_| DecodeError::OutOfRange)
        } else {
            let v = d.neg_int()?;
            i16::try_from(v).map_err(|_| DecodeError::OutOfRange)
        }
    }
}

impl<'a> Decode<'a> for i8 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        if d.peek_major()? == MAJOR_UNSIGNED {
            let v = d.uint()?;
            i8::try_from(v).map_err(|_| DecodeError::OutOfRange)
        } else {
            let v = d.neg_int()?;
            i8::try_from(v).map_err(|_| DecodeError::OutOfRange)
        }
    }
}

impl<'a> Decode<'a> for &'a [u8] {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        d.bytes()
    }
}

impl<'a> Decode<'a> for () {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        d.null()
    }
}

impl<'a> Decode<'a> for bool {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        d.bool()
    }
}

#[cfg(feature = "float")]
impl<'a> Decode<'a> for f64 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        match d.next_float()? {
            Float::F16(bits) => Ok(half::f16::from_bits(bits).to_f64()),
            Float::F32(bits) => Ok(f32::from_bits(bits) as f64),
            Float::F64(bits) => Ok(f64::from_bits(bits)),
        }
    }
}

#[cfg(feature = "float")]
impl<'a> Decode<'a> for f32 {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        match d.next_float()? {
            Float::F16(bits) => Ok(half::f16::from_bits(bits).to_f32()),
            Float::F32(bits) => Ok(f32::from_bits(bits)),
            Float::F64(bits) => {
                let v = f64::from_bits(bits) as f32;
                Ok(v)
            }
        }
    }
}

impl<'a> Decode<'a> for &'a str {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        d.text()
    }
}

/// Decode a single CBOR value from a byte slice. Does not check for trailing bytes.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::decode;
///
/// let value: u64 = decode(&[0x18, 0x2a]).unwrap();
/// assert_eq!(value, 42);
/// ```
#[must_use = "returns the decoded value"]
pub fn decode<'a, T: Decode<'a>>(bytes: &'a [u8]) -> Result<T, DecodeError> {
    let mut d = Decoder::new(bytes);
    T::decode(&mut d)
}

/******************************************************************************
 * Array decoder implementation
 ******************************************************************************/
/// Borrowed CBOR array with iterator access.
#[derive(Debug, Clone, PartialEq)]
pub struct Array<'a> {
    data: &'a [u8],
    len: u64,
}

impl<'a> Array<'a> {
    /// Number of items in the array.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Returns true if the array is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over array items.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Item, DecodeError};
    ///
    /// fn main() -> Result<(), DecodeError> {
    ///   let data = [0x82, 0x01, 0x02];
    ///   let mut dec = Decoder::new(&data);
    ///   let item = dec.decode()?;
    ///   let arr = match item { Item::Array(arr) => arr, _ => return Err(DecodeError::UnexpectedItem) };
    ///   let items: Vec<u64> = arr.iter()
    ///       .map(|res| match res.unwrap() { Item::Unsigned(v) => v, _ => 0 })
    ///       .collect();
    ///   assert_eq!(items, vec![1, 2]);
    ///   Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn iter(&self) -> ArrayIter<'a> {
        ArrayIter {
            dec: Decoder::new(self.data),
            remaining: self.len,
            failed: false,
        }
    }

    /// Iterate over array items, also yielding each item's byte range
    /// relative to this array's own data (i.e. relative to the position
    /// immediately after the array's header).
    ///
    /// Requires the `spans` feature.
    #[cfg(feature = "spans")]
    #[must_use]
    pub fn iter_with_span(&self) -> ArraySpanIter<'a> {
        ArraySpanIter {
            dec: Decoder::new(self.data),
            remaining: self.len,
            failed: false,
        }
    }
}

/// Iterator over array items.
pub struct ArrayIter<'a> {
    dec: Decoder<'a>,
    remaining: u64,
    failed: bool,
}

impl<'a> Iterator for ArrayIter<'a> {
    type Item = Result<Item<'a>, DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        match self.dec.next() {
            Ok(item) => Some(Ok(item)),
            Err(err) => {
                self.failed = true;
                Some(Err(err))
            }
        }
    }
}

/// Iterator over array items that also yields each item's byte span.
#[cfg(feature = "spans")]
pub struct ArraySpanIter<'a> {
    dec: Decoder<'a>,
    remaining: u64,
    failed: bool,
}

#[cfg(feature = "spans")]
impl<'a> Iterator for ArraySpanIter<'a> {
    type Item = Result<(Item<'a>, core::ops::Range<usize>), DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        match self.dec.next_with_span() {
            Ok(item) => Some(Ok(item)),
            Err(err) => {
                self.failed = true;
                Some(Err(err))
            }
        }
    }
}

/******************************************************************************
 * Map decoder implementation
 ******************************************************************************/
/// Borrowed CBOR map with iterator access.
#[derive(Debug, Clone, PartialEq)]
pub struct Map<'a> {
    data: &'a [u8],
    len: u64,
}

impl<'a> Map<'a> {
    /// Number of key/value pairs in the map.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Returns true if the map is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over map pairs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{DecodeError, Decoder, Item};
    /// # fn main() -> Result<(), DecodeError> {
    /// let data = [0xa1, 0x01, 0x02];
    /// let mut dec = Decoder::new(&data);
    /// let item = dec.decode()?;
    /// let map = match item {
    ///     Item::Map(map) => map,
    ///      _ => return Err(DecodeError::UnexpectedItem)
    /// };
    /// let pairs: Vec<(u64, u64)> = map.iter()
    ///     .map(|res| {
    ///         let (k, v) = res.unwrap();
    ///         let k = match k { Item::Unsigned(v) => v, _ => 0 };
    ///         let v = match v { Item::Unsigned(v) => v, _ => 0 };
    ///         (k, v)
    ///     })
    ///     .collect();
    /// assert_eq!(pairs, vec![(1, 2)]);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn iter(&self) -> MapIter<'a> {
        MapIter {
            dec: Decoder::new(self.data),
            remaining: self.len,
            failed: false,
        }
    }

    /// Iterate over map pairs, also yielding each key's and value's byte
    /// range relative to this map's own data (i.e. relative to the position
    /// immediately after the map's header).
    ///
    /// Requires the `spans` feature.
    #[cfg(feature = "spans")]
    #[must_use]
    pub fn iter_with_span(&self) -> MapSpanIter<'a> {
        MapSpanIter {
            dec: Decoder::new(self.data),
            remaining: self.len,
            failed: false,
        }
    }
}

/// Iterator over map pairs.
pub struct MapIter<'a> {
    dec: Decoder<'a>,
    remaining: u64,
    failed: bool,
}

impl<'a> Iterator for MapIter<'a> {
    type Item = Result<(Item<'a>, Item<'a>), DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let key = match self.dec.next() {
            Ok(item) => item,
            Err(err) => {
                self.failed = true;
                return Some(Err(err));
            }
        };
        match self.dec.next() {
            Ok(value) => Some(Ok((key, value))),
            Err(err) => {
                self.failed = true;
                Some(Err(err))
            }
        }
    }
}

/// Iterator over map pairs that also yields each key's and value's byte span.
#[cfg(feature = "spans")]
pub struct MapSpanIter<'a> {
    dec: Decoder<'a>,
    remaining: u64,
    failed: bool,
}

#[cfg(feature = "spans")]
impl<'a> Iterator for MapSpanIter<'a> {
    #[allow(clippy::type_complexity)]
    type Item = Result<
        (
            (Item<'a>, core::ops::Range<usize>),
            (Item<'a>, core::ops::Range<usize>),
        ),
        DecodeError,
    >;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let key = match self.dec.next_with_span() {
            Ok(item) => item,
            Err(err) => {
                self.failed = true;
                return Some(Err(err));
            }
        };
        match self.dec.next_with_span() {
            Ok(value) => Some(Ok((key, value))),
            Err(err) => {
                self.failed = true;
                Some(Err(err))
            }
        }
    }
}

/******************************************************************************
 * Tag decoder implementation
 ******************************************************************************/
/// Decoded CBOR tag with a single encoded item.
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedItem<'a> {
    /// The CBOR tag number (Major Type 6).
    pub tag: u64,
    data: &'a [u8],
}

impl<'a> TaggedItem<'a> {
    /// Decode the tagged item.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::{Decoder, Item, DecodeError};
    /// # fn main() -> Result<(), DecodeError> {
    ///
    /// let data = [0xc1, 0x01];
    /// let mut dec = Decoder::new(&data);
    /// let item = dec.decode()?;
    /// let tagged = match item {
    ///     Item::Tag(tagged) => tagged,
    ///     _ => return Err(DecodeError::UnexpectedItem)
    /// };
    /// let inner = tagged.item()?;
    /// assert!(matches!(inner, Item::Unsigned(1)));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns the decoded inner item"]
    pub fn item(&self) -> Result<Item<'a>, DecodeError> {
        let mut dec = Decoder::new(self.data);
        dec.decode()
    }

    /// Decode the tagged item's inner value, also returning its byte range
    /// relative to this tagged item's own data (i.e. relative to the
    /// position immediately after the tag's header).
    ///
    /// Requires the `spans` feature.
    #[cfg(feature = "spans")]
    #[must_use = "returns the decoded inner item and its span"]
    pub fn item_with_span(&self) -> Result<(Item<'a>, core::ops::Range<usize>), DecodeError> {
        let mut dec = Decoder::new(self.data);
        dec.decode_with_span()
    }
}

/// Decoded floating-point value.
#[cfg(feature = "float")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Float {
    /// 16-bit half-precision float (raw bits).
    F16(u16),
    /// 32-bit single-precision float (raw bits).
    F32(u32),
    /// 64-bit double-precision float (raw bits).
    F64(u64),
}

#[cfg(feature = "diagnostic")]
struct SliceWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
    overflowed: bool,
}

#[cfg(feature = "diagnostic")]
impl<'a> SliceWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            overflowed: false,
        }
    }

    fn len(&self) -> usize {
        self.pos
    }
}

#[cfg(feature = "diagnostic")]
impl core::fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let end = self.pos + bytes.len();
        if end > self.buf.len() {
            self.overflowed = true;
            return Err(core::fmt::Error);
        }
        self.buf[self.pos..end].copy_from_slice(bytes);
        self.pos = end;
        Ok(())
    }
}

#[cfg(all(feature = "diagnostic", feature = "std"))]
struct IoWriter<'a, W: std::io::Write> {
    out: &'a mut W,
}

#[cfg(all(feature = "diagnostic", feature = "std"))]
impl<'a, W: std::io::Write> IoWriter<'a, W> {
    fn new(out: &'a mut W) -> Self {
        Self { out }
    }
}

#[cfg(all(feature = "diagnostic", feature = "std"))]
impl<W: std::io::Write> core::fmt::Write for IoWriter<'_, W> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.out
            .write_all(s.as_bytes())
            .map_err(|_| core::fmt::Error)
    }
}

#[cfg(all(feature = "diagnostic", feature = "spans"))]
struct CountingWriter<'a, W: core::fmt::Write> {
    out: &'a mut W,
    count: usize,
}

#[cfg(all(feature = "diagnostic", feature = "spans"))]
impl<'a, W: core::fmt::Write> CountingWriter<'a, W> {
    fn new(out: &'a mut W) -> Self {
        Self { out, count: 0 }
    }
}

#[cfg(all(feature = "diagnostic", feature = "spans"))]
impl<W: core::fmt::Write> core::fmt::Write for CountingWriter<'_, W> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.out.write_str(s)?;
        self.count += s.len();
        Ok(())
    }
}

#[cfg(feature = "diagnostic")]
struct DiagnosticDecoder<'a> {
    buf: &'a [u8],
    pos: usize,
}

#[cfg(feature = "diagnostic")]
impl<'a> DiagnosticDecoder<'a> {
    fn new(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }

    fn read_header_full(&mut self) -> Result<(u8, u8, u64), DecodeError> {
        let (major, addl, value, consumed) = parse_header(self.buf, self.pos)?;
        self.pos += consumed;
        Ok((major, addl, value))
    }

    fn take_bytes(&mut self, len: u64) -> Result<&'a [u8], DecodeError> {
        let len = usize::try_from(len).map_err(|_| DecodeError::InvalidAdditionalInfo)?;
        let end = self.pos.checked_add(len).ok_or(DecodeError::UnexpectedEof)?;
        if end > self.buf.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    /// Write the next CBOR item to `out`. `pretty` selects the layout: `None`
    /// writes everything on one line; `Some(depth)` indents array/map bodies
    /// one level (two spaces) beyond `depth`, recursing with `depth + 1` so
    /// nested structure is visible. Tags are always written inline with
    /// their content — only arrays and maps introduce newlines.
    fn write_item<W: core::fmt::Write>(
        &mut self,
        out: &mut W,
        pretty: Option<usize>,
    ) -> Result<(), DiagnosticError> {
        self.write_item_at_depth(out, pretty, 0)
    }

    fn write_item_at_depth<W: core::fmt::Write>(
        &mut self,
        out: &mut W,
        pretty: Option<usize>,
        recursion_depth: usize,
    ) -> Result<(), DiagnosticError> {
        if recursion_depth >= MAX_DECODE_DEPTH {
            return Err(DiagnosticError::Decode(DecodeError::NestingTooDeep));
        }
        let (major, addl, value) = self.read_header_full()?;
        match major {
            MAJOR_UNSIGNED => write!(out, "{value}")?,
            MAJOR_NEGATIVE => {
                let magnitude = (value as u128) + 1;
                out.write_str("-")?;
                write!(out, "{magnitude}")?;
            }
            MAJOR_BYTES => {
                let bytes = self.take_bytes(value)?;
                out.write_str("h'")?;
                write_hex(out, bytes)?;
                out.write_str("'")?;
            }
            MAJOR_TEXT => {
                let bytes = self.take_bytes(value)?;
                let text = str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
                out.write_str("\"")?;
                write_escaped_str(out, text)?;
                out.write_str("\"")?;
            }
            MAJOR_ARRAY if value == 0 => out.write_str("[]")?,
            MAJOR_ARRAY => match pretty {
                Some(depth) => {
                    out.write_str("[\n")?;
                    for i in 0..value {
                        write_indent(out, depth + 1)?;
                        self.write_item_at_depth(out, Some(depth + 1), recursion_depth + 1)?;
                        if i + 1 < value {
                            out.write_str(",")?;
                        }
                        out.write_str("\n")?;
                    }
                    write_indent(out, depth)?;
                    out.write_str("]")?;
                }
                None => {
                    out.write_str("[")?;
                    for i in 0..value {
                        if i > 0 {
                            out.write_str(",")?;
                        }
                        self.write_item_at_depth(out, None, recursion_depth + 1)?;
                    }
                    out.write_str("]")?;
                }
            },
            MAJOR_MAP if value == 0 => out.write_str("{}")?,
            MAJOR_MAP => match pretty {
                Some(depth) => {
                    out.write_str("{\n")?;
                    for i in 0..value {
                        write_indent(out, depth + 1)?;
                        self.write_item_at_depth(out, Some(depth + 1), recursion_depth + 1)?;
                        out.write_str(": ")?;
                        self.write_item_at_depth(out, Some(depth + 1), recursion_depth + 1)?;
                        if i + 1 < value {
                            out.write_str(",")?;
                        }
                        out.write_str("\n")?;
                    }
                    write_indent(out, depth)?;
                    out.write_str("}")?;
                }
                None => {
                    out.write_str("{")?;
                    for i in 0..value {
                        if i > 0 {
                            out.write_str(",")?;
                        }
                        self.write_item_at_depth(out, None, recursion_depth + 1)?;
                        out.write_str(":")?;
                        self.write_item_at_depth(out, None, recursion_depth + 1)?;
                    }
                    out.write_str("}")?;
                }
            },
            MAJOR_TAG => {
                if value == 2 || value == 3 {
                    let inner_pos = self.pos;
                    match self.read_header_full() {
                        Ok((inner_major, _, inner_len)) => {
                            if inner_major == MAJOR_BYTES {
                                let bytes = self.take_bytes(inner_len)?;
                                write_bignum(out, bytes, value == 3)?;
                                return Ok(());
                            }
                            self.pos = inner_pos;
                        }
                        Err(err) => return Err(DiagnosticError::Decode(err)),
                    }
                }
                write!(out, "{value}")?;
                out.write_str("(")?;
                self.write_item_at_depth(out, pretty, recursion_depth + 1)?;
                out.write_str(")")?;
            }
            MAJOR_SIMPLE => match addl {
                0..=19 => {
                    write!(out, "simple({addl})")?;
                }
                20 => out.write_str("false")?,
                21 => out.write_str("true")?,
                22 => out.write_str("null")?,
                23 => out.write_str("undefined")?,
                24 => {
                    let simple = value as u8;
                    if (20..=31).contains(&simple) {
                        return Err(DiagnosticError::Decode(DecodeError::InvalidAdditionalInfo));
                    }
                    write!(out, "simple({simple})")?;
                }
                25 => {
                    #[cfg(feature = "float")]
                    {
                        write_float(out, value as u16, FloatWidth::F16)?;
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        return Err(DiagnosticError::Decode(DecodeError::FloatDisabled));
                    }
                }
                26 => {
                    #[cfg(feature = "float")]
                    {
                        write_float(out, value as u32, FloatWidth::F32)?;
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        return Err(DiagnosticError::Decode(DecodeError::FloatDisabled));
                    }
                }
                27 => {
                    #[cfg(feature = "float")]
                    {
                        write_float(out, value, FloatWidth::F64)?;
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        return Err(DiagnosticError::Decode(DecodeError::FloatDisabled));
                    }
                }
                _ => return Err(DiagnosticError::Decode(DecodeError::InvalidAdditionalInfo)),
            },
            _ => return Err(DiagnosticError::Decode(DecodeError::InvalidAdditionalInfo)),
        }
        Ok(())
    }

    /// Identical traversal to [`write_item`](Self::write_item), additionally
    /// recording, for every item visited (including nested array/map
    /// elements and tag contents), its CBOR byte range (relative to
    /// `self.buf`) alongside the EDN text byte range written for it.
    #[cfg(all(feature = "diagnostic", feature = "spans"))]
    fn write_item_with_spans<W: core::fmt::Write>(
        &mut self,
        out: &mut CountingWriter<'_, W>,
        pretty: Option<usize>,
        spans: &mut Vec<DiagnosticSpan>,
    ) -> Result<(), DiagnosticError> {
        self.write_item_with_spans_at_depth(out, pretty, spans, 0)
    }

    #[cfg(all(feature = "diagnostic", feature = "spans"))]
    fn write_item_with_spans_at_depth<W: core::fmt::Write>(
        &mut self,
        out: &mut CountingWriter<'_, W>,
        pretty: Option<usize>,
        spans: &mut Vec<DiagnosticSpan>,
        recursion_depth: usize,
    ) -> Result<(), DiagnosticError> {
        if recursion_depth >= MAX_DECODE_DEPTH {
            return Err(DiagnosticError::Decode(DecodeError::NestingTooDeep));
        }
        let cbor_start = self.pos;
        let edn_start = out.count;
        let (major, addl, value) = self.read_header_full()?;
        match major {
            MAJOR_UNSIGNED => write!(out, "{value}")?,
            MAJOR_NEGATIVE => {
                let magnitude = (value as u128) + 1;
                out.write_str("-")?;
                write!(out, "{magnitude}")?;
            }
            MAJOR_BYTES => {
                let bytes = self.take_bytes(value)?;
                out.write_str("h'")?;
                write_hex(out, bytes)?;
                out.write_str("'")?;
            }
            MAJOR_TEXT => {
                let bytes = self.take_bytes(value)?;
                let text = str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
                out.write_str("\"")?;
                write_escaped_str(out, text)?;
                out.write_str("\"")?;
            }
            MAJOR_ARRAY if value == 0 => out.write_str("[]")?,
            MAJOR_ARRAY => match pretty {
                Some(depth) => {
                    out.write_str("[\n")?;
                    for i in 0..value {
                        write_indent(out, depth + 1)?;
                        self.write_item_with_spans_at_depth(
                            out,
                            Some(depth + 1),
                            spans,
                            recursion_depth + 1,
                        )?;
                        if i + 1 < value {
                            out.write_str(",")?;
                        }
                        out.write_str("\n")?;
                    }
                    write_indent(out, depth)?;
                    out.write_str("]")?;
                }
                None => {
                    out.write_str("[")?;
                    for i in 0..value {
                        if i > 0 {
                            out.write_str(",")?;
                        }
                        self.write_item_with_spans_at_depth(
                            out,
                            None,
                            spans,
                            recursion_depth + 1,
                        )?;
                    }
                    out.write_str("]")?;
                }
            },
            MAJOR_MAP if value == 0 => out.write_str("{}")?,
            MAJOR_MAP => match pretty {
                Some(depth) => {
                    out.write_str("{\n")?;
                    for i in 0..value {
                        write_indent(out, depth + 1)?;
                        self.write_item_with_spans_at_depth(
                            out,
                            Some(depth + 1),
                            spans,
                            recursion_depth + 1,
                        )?;
                        out.write_str(": ")?;
                        self.write_item_with_spans_at_depth(
                            out,
                            Some(depth + 1),
                            spans,
                            recursion_depth + 1,
                        )?;
                        if i + 1 < value {
                            out.write_str(",")?;
                        }
                        out.write_str("\n")?;
                    }
                    write_indent(out, depth)?;
                    out.write_str("}")?;
                }
                None => {
                    out.write_str("{")?;
                    for i in 0..value {
                        if i > 0 {
                            out.write_str(",")?;
                        }
                        self.write_item_with_spans_at_depth(
                            out,
                            None,
                            spans,
                            recursion_depth + 1,
                        )?;
                        out.write_str(":")?;
                        self.write_item_with_spans_at_depth(
                            out,
                            None,
                            spans,
                            recursion_depth + 1,
                        )?;
                    }
                    out.write_str("}")?;
                }
            },
            MAJOR_TAG => {
                if value == 2 || value == 3 {
                    let inner_pos = self.pos;
                    match self.read_header_full() {
                        Ok((inner_major, _, inner_len)) => {
                            if inner_major == MAJOR_BYTES {
                                let bytes = self.take_bytes(inner_len)?;
                                write_bignum(out, bytes, value == 3)?;
                                spans.push((cbor_start..self.pos, edn_start..out.count));
                                return Ok(());
                            }
                            self.pos = inner_pos;
                        }
                        Err(err) => return Err(DiagnosticError::Decode(err)),
                    }
                }
                write!(out, "{value}")?;
                out.write_str("(")?;
                self.write_item_with_spans_at_depth(out, pretty, spans, recursion_depth + 1)?;
                out.write_str(")")?;
            }
            MAJOR_SIMPLE => match addl {
                0..=19 => {
                    write!(out, "simple({addl})")?;
                }
                20 => out.write_str("false")?,
                21 => out.write_str("true")?,
                22 => out.write_str("null")?,
                23 => out.write_str("undefined")?,
                24 => {
                    let simple = value as u8;
                    if (20..=31).contains(&simple) {
                        return Err(DiagnosticError::Decode(DecodeError::InvalidAdditionalInfo));
                    }
                    write!(out, "simple({simple})")?;
                }
                25 => {
                    #[cfg(feature = "float")]
                    {
                        write_float(out, value as u16, FloatWidth::F16)?;
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        return Err(DiagnosticError::Decode(DecodeError::FloatDisabled));
                    }
                }
                26 => {
                    #[cfg(feature = "float")]
                    {
                        write_float(out, value as u32, FloatWidth::F32)?;
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        return Err(DiagnosticError::Decode(DecodeError::FloatDisabled));
                    }
                }
                27 => {
                    #[cfg(feature = "float")]
                    {
                        write_float(out, value, FloatWidth::F64)?;
                    }
                    #[cfg(not(feature = "float"))]
                    {
                        return Err(DiagnosticError::Decode(DecodeError::FloatDisabled));
                    }
                }
                _ => return Err(DiagnosticError::Decode(DecodeError::InvalidAdditionalInfo)),
            },
            _ => return Err(DiagnosticError::Decode(DecodeError::InvalidAdditionalInfo)),
        }
        spans.push((cbor_start..self.pos, edn_start..out.count));
        Ok(())
    }
}

#[cfg(feature = "diagnostic")]
fn write_hex<W: core::fmt::Write>(out: &mut W, bytes: &[u8]) -> Result<(), DiagnosticError> {
    for b in bytes {
        write!(out, "{:02x}", b)?;
    }
    Ok(())
}

#[cfg(feature = "diagnostic")]
fn write_indent<W: core::fmt::Write>(out: &mut W, depth: usize) -> Result<(), DiagnosticError> {
    for _ in 0..depth {
        out.write_str("  ")?;
    }
    Ok(())
}

#[cfg(feature = "diagnostic")]
fn write_bignum<W: core::fmt::Write>(
    out: &mut W,
    bytes: &[u8],
    negative: bool,
) -> Result<(), DiagnosticError> {
    let mut digits = bignum_to_digits(bytes);
    if negative {
        add_one_digits(&mut digits);
        out.write_str("-")?;
    }
    write_digits(out, &digits)
}

#[cfg(feature = "diagnostic")]
fn bignum_to_digits(bytes: &[u8]) -> Vec<u8> {
    let mut digits = vec![0u8];
    for &byte in bytes {
        let mut carry = byte as u32;
        for digit in digits.iter_mut() {
            let value = (*digit as u32) * 256 + carry;
            *digit = (value % 10) as u8;
            carry = value / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    while digits.len() > 1 && *digits.last().unwrap() == 0 {
        digits.pop();
    }
    digits
}

#[cfg(feature = "diagnostic")]
fn add_one_digits(digits: &mut Vec<u8>) {
    let mut carry = 1u8;
    for digit in digits.iter_mut() {
        if carry == 0 {
            break;
        }
        let value = *digit + carry;
        if value >= 10 {
            *digit = value - 10;
            carry = 1;
        } else {
            *digit = value;
            carry = 0;
        }
    }
    if carry == 1 {
        digits.push(1);
    }
}

#[cfg(feature = "diagnostic")]
fn write_digits<W: core::fmt::Write>(out: &mut W, digits: &[u8]) -> Result<(), DiagnosticError> {
    for &digit in digits.iter().rev() {
        let ch = (b'0' + digit) as char;
        out.write_char(ch)?;
    }
    Ok(())
}

#[cfg(feature = "diagnostic")]
fn write_escaped_str<W: core::fmt::Write>(out: &mut W, text: &str) -> Result<(), DiagnosticError> {
    for ch in text.chars() {
        match ch {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\u{00}'..='\u{1f}' | '\u{7f}' => {
                let v = ch as u32;
                write!(out, "\\u{:04x}", v)?;
            }
            _ if ch.is_ascii() => {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                out.write_str(s)?;
            }
            _ => {
                let mut buf = [0u16; 2];
                let encoded = ch.encode_utf16(&mut buf);
                for unit in encoded {
                    write!(out, "\\u{:04x}", unit)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(all(feature = "diagnostic", feature = "float"))]
enum FloatWidth {
    F16,
    F32,
    F64,
}

#[cfg(all(feature = "diagnostic", feature = "float"))]
fn write_float<W: core::fmt::Write, T: Into<u64>>(
    out: &mut W,
    bits: T,
    width: FloatWidth,
) -> Result<(), DiagnosticError> {
    let value = match width {
        FloatWidth::F16 => {
            let bits = bits.into() as u16;
            half::f16::from_bits(bits).to_f64()
        }
        FloatWidth::F32 => {
            let bits = bits.into() as u32;
            f32::from_bits(bits) as f64
        }
        FloatWidth::F64 => {
            let bits = bits.into();
            f64::from_bits(bits)
        }
    };

    if value.is_nan() {
        out.write_str("NaN")?;
        return Ok(());
    }
    if value.is_infinite() {
        if value.is_sign_negative() {
            out.write_str("-Infinity")?;
        } else {
            out.write_str("Infinity")?;
        }
        return Ok(());
    }

    let mut buf = ryu::Buffer::new();
    let s = buf.format(value);
    if let Some(exp_pos) = s.find('e') {
        let (mantissa, exp) = s.split_at(exp_pos);
        if mantissa.contains('.') {
            out.write_str(mantissa)?;
        } else {
            out.write_str(mantissa)?;
            out.write_str(".0")?;
        }
        let exp = &exp[1..];
        out.write_str("e")?;
        if !exp.starts_with('-') {
            out.write_str("+")?;
        }
        out.write_str(exp)?;
    } else {
        if s.contains('.') {
            out.write_str(s)?;
        } else {
            out.write_str(s)?;
            out.write_str(".0")?;
        }
    }
    Ok(())
}
