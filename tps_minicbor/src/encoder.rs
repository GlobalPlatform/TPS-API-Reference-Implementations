/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! # CBOR Encoder module.
//!
//! A CBOR encoder complying closely with RFC 8949, with the exception that there is no support for
//! indefinite length items.
//!
//! - The implementation uses a [`Sizer`] struct to manage position in the buffer.
//! - The [`EncodeItem`] trait provides implementations for encoding many Rust types

use crate::{
    DEFAULT_NESTING_DEPTH, EncodeError, MAJOR_ARRAY, MAJOR_BYTES, MAJOR_MAP, MAJOR_NEGATIVE,
    MAJOR_SIMPLE, MAJOR_TAG, MAJOR_TEXT, MAJOR_UNSIGNED, MAX_HEADER_LEN,
};

#[cfg(feature = "float")]
use half::f16;

use core::convert::TryFrom;
use core::default::Default;
use core::option::Option;

/******************************************************************************
 * LogicalBuf helpers
 ******************************************************************************/
// Read the byte at logical index `idx`, mapped to `buf[idx]` when `idx < buf.len()`, else to
// `overflow_scratch[idx - buf.len()]` if present, else unavailable.
fn logical_read_at(buf: &[u8], overflow_scratch: Option<&[u8]>, idx: usize) -> Option<u8> {
    if idx < buf.len() {
        Some(buf[idx])
    } else if let Some(scratch) = overflow_scratch {
        scratch.get(idx - buf.len()).copied()
    } else {
        None
    }
}

// Write `byte` at logical index `idx`. Returns `false` (byte is lost) if `idx` is not backed by
// `buf` or `overflow_scratch`.
fn logical_write_at(buf: &mut [u8], overflow_scratch: Option<&mut [u8]>, idx: usize, byte: u8) -> bool {
    if idx < buf.len() {
        buf[idx] = byte;
        true
    } else if let Some(scratch) = overflow_scratch {
        if let Some(slot) = scratch.get_mut(idx - buf.len()) {
            *slot = byte;
            true
        } else {
            false
        }
    } else {
        false
    }
}

/******************************************************************************
 * Encoder implementation
 ******************************************************************************/

/// CBOR encoder with a maximum depth determined by the generic constant parameter.
///
/// See also [`crate::EncoderStd`] which is a typedef for an encoder with a maximum
/// depth of 4 (it is identical to `Encoder::<4>`).
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::Encoder;
///
/// let mut buf = [0u8; 16];
/// let mut enc = Encoder::<4>::new(&mut buf);
/// enc.insert(1u8).unwrap();
/// assert_eq!(enc.encoded(), &[0x01]);
/// ```
pub struct Encoder<'a, const N: usize = DEFAULT_NESTING_DEPTH> {
    buf: &'a mut [u8],
    // Overflow scratch used to preserve the real bytes of any content that is logically
    // positioned past the end of `buf`, so that `pos` can be reported exactly even when a
    // mid-chain overflow is later "rescued" by a shrinking container header. See
    // `new_with_overflow`.
    overflow_scratch: Option<&'a mut [u8]>,
    // Logical position: may exceed `buf.len()` (and even `buf.len() + overflow_scratch.len()`)
    // while a container is open, since an ancestor's header may still shrink and rescue the
    // total back under the buffer limit by the time the outermost container closes.
    pos: usize,
    // Set once any byte has been written to a logical position with no backing storage (past
    // both `buf` and `overflow_scratch`). Sticky for the life of the encoder: every byte ever
    // written remains part of the final output if the overall operation succeeds, so a single
    // lost byte always means the "success" a header shrink appears to produce is actually
    // corrupted data, not a genuine rescue.
    overflow_lost: bool,
    frame_stack: [ContainerFrame; N],
    depth: usize,
    max_depth: usize,
    #[cfg(feature = "deterministic_encoding")]
    deterministic_scratch: Option<&'a mut [u8]>,
}

impl<'a, const N: usize> Encoder<'a, N> {
    /// Create a new instance encoder with maximum nesting depth `N`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    ///
    /// let mut buf = [0u8; 16];
    /// // Maximum nesting depth below is 8
    /// let mut enc = Encoder::<8>::new(&mut buf);
    /// enc.array().unwrap().end().unwrap();
    /// assert_eq!(enc.encoded(), &[0x80]);
    /// ```
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            overflow_scratch: None,
            pos: 0,
            overflow_lost: false,
            frame_stack: [ContainerFrame::empty(); N],
            depth: 0,
            max_depth: N,
            #[cfg(feature = "deterministic_encoding")]
            deterministic_scratch: None,
        }
    }

    /// Create an encoder that reports the exact number of bytes an overflowing encode would
    /// have required, even when the overflow first occurs mid-way through a nested builder
    /// chain (rather than only at the byte offset where the local write first failed).
    ///
    /// `overflow_scratch` holds the real bytes of any content that is logically positioned past
    /// the end of `buf`. This allows an ancestor array/map's header to shrink correctly and
    /// "rescue" a transient mid-chain overflow, and allows duplicate-key detection to remain
    /// exact even when a map's content temporarily extends past `buf.len()`. Size it to
    /// comfortably exceed the largest single overflow this encoder is expected to see; if it is
    /// itself exhausted, [`EncodeError::OutOfSpace`]'s reported size remains exact, but rescue
    /// and duplicate-key detection are no longer possible for the unrepresented tail.
    ///
    /// Without this constructor (i.e. using [`Encoder::new`]), the reported
    /// [`EncodeError::OutOfSpace`] size is still exact, but rescue can never complete (there are
    /// no real bytes to copy back), and duplicate-key detection is skipped for any map whose
    /// content extends past `buf.len()` — a documented limitation that is harmless in practice
    /// since the overall encode always fails in that scenario regardless of which error variant
    /// would otherwise have been reported.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    /// # use tps_minicbor::EncodeError;
    /// # fn main() -> Result<(), EncodeError> {
    ///
    /// let mut buf = [0u8; 4];
    /// let mut overflow = [0u8; 16];
    /// let mut enc = Encoder::<4>::new_with_overflow(&mut buf, &mut overflow);
    /// let mut map = enc.map()?;
    /// map.insert_key_value("aa", 1u8)?;
    /// map.insert_key_value("bb", 2u8)?;
    /// let err = map.end().unwrap_err();
    /// assert_eq!(err, EncodeError::OutOfSpace(9));
    /// # Ok(())
    /// # }
    /// ```
    pub fn new_with_overflow(buf: &'a mut [u8], overflow_scratch: &'a mut [u8]) -> Self {
        Self {
            buf,
            overflow_scratch: Some(overflow_scratch),
            pos: 0,
            overflow_lost: false,
            frame_stack: [ContainerFrame::empty(); N],
            depth: 0,
            max_depth: N,
            #[cfg(feature = "deterministic_encoding")]
            deterministic_scratch: None,
        }
    }

    #[cfg(feature = "deterministic_encoding")]
    /// Create an encoder that deterministically orders map keys using a provided scratch buffer.
    ///
    /// The scratch buffer must not alias the output buffer and must be at least as
    /// large as the output buffer to handle worst-case maps. Returns
    /// [`EncodeError::OutOfSpace`] if the scratch buffer is smaller than the output buffer.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    ///
    /// let mut buf = [0u8; 32];
    /// let mut scratch = [0u8; 32];
    /// let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
    /// let mut map = enc.map().unwrap();
    /// map.insert_key_value("b", 2u8).unwrap();
    /// map.insert_key_value("a", 1u8).unwrap();
    /// map.end().unwrap();
    /// assert_eq!(enc.encoded(), &[0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02]);
    /// ```
    pub fn new_deterministic(
        buf: &'a mut [u8],
        scratch: &'a mut [u8],
    ) -> Result<Self, EncodeError> {
        if scratch.len() < buf.len() {
            return Err(EncodeError::OutOfSpace(buf.len()));
        }
        Ok(Self {
            buf,
            overflow_scratch: None,
            pos: 0,
            overflow_lost: false,
            frame_stack: [ContainerFrame::empty(); N],
            depth: 0,
            max_depth: N,
            deterministic_scratch: Some(scratch),
        })
    }

    /// Current encoded length in bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    ///
    /// let mut buf = [0u8; 16];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// enc.insert(10u8).unwrap();
    /// assert_eq!(enc.len(), 1);
    /// ```
    #[must_use]
    pub const fn len(&self) -> usize {
        self.pos
    }

    /// Returns `true` if the Encoder buffer is currently empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.pos == 0
    }

    /// Encode a single value using the [`EncodeItem`] trait.
    pub fn insert<T: EncodeItem>(&mut self, value: T) -> Result<&mut Self, EncodeError> {
        self.can_accept_item()?;
        let parent_before = if self.depth == 0 {
            None
        } else {
            Some(self.frame_stack[self.depth - 1])
        };
        value.encode(self)?;
        if let Some(parent_before) = parent_before {
            let parent_after = self.frame_stack[self.depth - 1];
            if parent_after.count != parent_before.count
                || parent_after.map_expect_key != parent_before.map_expect_key
            {
                return Ok(self);
            }
        }
        self.record_item_for_parent()?;
        if self.depth == 0 {
            self.check_complete()?;
        }
        Ok(self)
    }

    /// Insert a single reference using the [`EncodeItem`] trait.
    pub fn insert_ref<T: EncodeItem + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<&mut Self, EncodeError> {
        self.can_accept_item()?;
        let parent_before = if self.depth == 0 {
            None
        } else {
            Some(self.frame_stack[self.depth - 1])
        };
        value.encode(self)?;
        if let Some(parent_before) = parent_before {
            let parent_after = self.frame_stack[self.depth - 1];
            if parent_after.count != parent_before.count
                || parent_after.map_expect_key != parent_before.map_expect_key
            {
                return Ok(self);
            }
        }
        self.record_item_for_parent()?;
        if self.depth == 0 {
            self.check_complete()?;
        }
        Ok(self)
    }

    /// Borrow the encoded output as a `u8` slice.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    ///
    /// let mut buf = [0u8; 8];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// enc.insert(1u8).unwrap();
    /// let out = enc.encoded();
    /// assert_eq!(out, &[0x01]);
    /// ```
    #[must_use]
    pub fn encoded(&self) -> &[u8] {
        &self.buf[..self.pos]
    }

    /// Begin an array of unknown length (calculated later) and compute its length on `end`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    ///
    /// let mut buf = [0u8; 16];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// let mut array = enc.array().unwrap();
    /// array.insert(1u8).unwrap();
    /// array.insert(2u8).unwrap();
    /// array.end().unwrap();
    /// assert_eq!(enc.encoded(), &[0x82, 0x01, 0x02]);
    /// ```
    pub fn array(&mut self) -> Result<ArrayBuilder<'a, '_, N>, EncodeError> {
        self.begin_array(None)?;
        Ok(ArrayBuilder {
            enc: self,
            close_tag: false,
        })
    }

    /// Begin an array with a known and fixed length (which will be checked).
    ///
    /// `InvalidValue` is returned on an attempt to insert too few or too many items in the array.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    /// # use tps_minicbor::EncodeError;
    /// # fn main() -> Result<(), EncodeError> {
    ///
    /// let mut buf = [0u8; 16];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// let mut array = enc.array_with_len(2)?;
    /// array.insert(1u8)?;
    /// array.insert(2u8)?;
    /// array.end()?;
    /// assert_eq!(enc.encoded(), &[0x82, 0x01, 0x02]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn array_with_len(&mut self, len: u64) -> Result<ArrayBuilder<'a, '_, N>, EncodeError> {
        self.begin_array(Some(len))?;
        Ok(ArrayBuilder {
            enc: self,
            close_tag: false,
        })
    }

    /// Begin a map and compute its length on `end`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    /// # use tps_minicbor::EncodeError;
    /// # fn main() -> Result<(), EncodeError> {
    ///
    /// let mut buf = [0u8; 16];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// let mut map = enc.map()?;
    /// map.insert_key_value("a", 1u8)?;
    /// map.end()?;
    /// assert_eq!(enc.encoded(), &[0xa1, 0x61, 0x61, 0x01]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn map(&mut self) -> Result<MapBuilder<'a, '_, N>, EncodeError> {
        self.begin_map(None)?;
        Ok(MapBuilder {
            enc: self,
            close_tag: false,
        })
    }

    /// Begin a map which has an expected number of key-value pairs, check on `end`.
    ///
    /// `EncodeError::InvalidValue` is returned if the number of entries does not match the value
    /// given when the map was created.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    /// # use tps_minicbor::EncodeError;
    /// # fn main() -> Result<(), EncodeError> {
    ///
    /// let mut buf = [0u8; 16];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// let mut map = enc.map_with_len(1)?;
    /// map.insert_key_value("a", 1u8)?;
    /// map.end()?;
    /// assert_eq!(enc.encoded(), &[0xa1, 0x61, 0x61, 0x01]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn map_with_len(&mut self, len: u64) -> Result<MapBuilder<'a, '_, N>, EncodeError> {
        self.begin_map(Some(len))?;
        Ok(MapBuilder {
            enc: self,
            close_tag: false,
        })
    }

    /// Tag a CBOR item. It is an error to attempt to tag more than one item.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    /// # use tps_minicbor::EncodeError;
    /// # fn main() -> Result<(), EncodeError> {
    ///
    /// let mut buf = [0u8; 24];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// enc.tag(1)?.item(1_363_896_240u64)?;
    /// assert_eq!(enc.encoded(), &[0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn tag(&mut self, tag: u64) -> Result<TagBuilder<'a, '_, N>, EncodeError> {
        self.begin_tag(tag)?;
        Ok(TagBuilder { enc: self })
    }

    // Write Major Type 0 and encode an unsigned integer value.
    fn uint(&mut self, value: u64) -> Result<(), EncodeError> {
        self.write_header(MAJOR_UNSIGNED, value)
    }

    // Write Major Type 1 and encode a negative integer value.
    // !value as u64 computes -v-1 without overflow, handling i64::MIN correctly.
    fn neg_int(&mut self, value: i64) -> Result<(), EncodeError> {
        if value >= 0 {
            return Err(EncodeError::InvalidValue);
        }
        self.write_header(MAJOR_NEGATIVE, !value as u64)
    }

    /// Write a bytestring (Major Type 3).
    pub fn bytes(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        self.write_header(MAJOR_BYTES, bytes.len() as u64)?;
        for b in bytes {
            self.write_byte(*b)?;
        }
        if self.depth == 0 {
            self.check_complete()?;
        }
        Ok(())
    }

    /// Write a text string (Major Type 4).
    ///
    /// The written value is not checked to be valid UTF-8 as Rust str type does not allow creation
    /// of invalid values. Additional checks would thus be superfluous.
    pub fn text(&mut self, text: &str) -> Result<(), EncodeError> {
        self.write_header(MAJOR_TEXT, text.len() as u64)?;
        for b in text.as_bytes() {
            self.write_byte(*b)?;
        }
        if self.depth == 0 {
            self.check_complete()?;
        }
        Ok(())
    }

    // Encode a boolean value (as a Simple).
    fn bool(&mut self, value: bool) -> Result<(), EncodeError> {
        let v = if value { 0xf5 } else { 0xf4 };
        self.write_byte(v)
    }

    // Encode a null value (as a Simple).
    fn null(&mut self) -> Result<(), EncodeError> {
        self.write_byte(0xf6)
    }

    // Encode the undefined value as a Simple.
    fn undefined(&mut self) -> Result<(), EncodeError> {
        self.write_byte(0xf7)
    }

    // Encode a simple numeric value (checked to be 0..19, 32..255).
    fn simple(&mut self, value: u8) -> Result<(), EncodeError> {
        match value {
            0..=19 => self.write_byte((MAJOR_SIMPLE << 5) | value),
            // RFC 8949 section 3.3 / Appendix F: values 20..=31 have no well-formed
            // encoding (20..=23 collide with the true/false/null/undefined one-byte
            // simple values; 24..=31 are reserved two-byte encodings).
            20..=31 => Err(EncodeError::InvalidValue),
            _ => {
                self.write_byte((MAJOR_SIMPLE << 5) | 24)?;
                self.write_byte(value)
            }
        }
    }

    // Write the header value for a tagged item.
    fn write_tag_header(&mut self, tag: u64) -> Result<(), EncodeError> {
        self.write_header(MAJOR_TAG, tag)
    }

    #[cfg(feature = "float")]
    fn write_float16(&mut self, bits: u16) -> Result<(), EncodeError> {
        self.write_byte(0xf9)?;
        self.write_be(bits as u64, 2)
    }

    #[cfg(feature = "float")]
    fn write_float32(&mut self, bits: u32) -> Result<(), EncodeError> {
        self.write_byte(0xfa)?;
        self.write_be(bits as u64, 4)
    }

    #[cfg(feature = "float")]
    fn write_float64(&mut self, bits: u64) -> Result<(), EncodeError> {
        self.write_byte(0xfb)?;
        self.write_be(bits, 8)
    }

    // Write the CBOR header
    fn write_header(&mut self, major: u8, value: u64) -> Result<(), EncodeError> {
        if value <= 23 {
            self.write_byte((major << 5) | value as u8)?;
            Ok(())
        } else if value <= 0xff {
            self.write_byte((major << 5) | 24)?;
            self.write_byte(value as u8)?;
            Ok(())
        } else if value <= 0xffff {
            self.write_byte((major << 5) | 25)?;
            self.write_be(value, 2)?;
            Ok(())
        } else if value <= 0xffff_ffff {
            self.write_byte((major << 5) | 26)?;
            self.write_be(value, 4)?;
            Ok(())
        } else {
            self.write_byte((major << 5) | 27)?;
            self.write_be(value, 8)?;
            Ok(())
        }
    }

    // Write a single byte at the current logical position (private). Never fails: writes past
    // `buf.len()` land in `overflow_scratch` if one was supplied (see `new_with_overflow`), or
    // are counted but not stored otherwise. Whether the overall operation succeeded is decided
    // later, at `check_complete`, once the complete shape of the request is known.
    fn write_byte(&mut self, byte: u8) -> Result<(), EncodeError> {
        if !logical_write_at(self.buf, self.overflow_scratch.as_deref_mut(), self.pos, byte) {
            self.overflow_lost = true;
        }
        self.pos += 1;
        Ok(())
    }

    // Reserve space for an item header. Initially we reserve the maximum size
    // and then move items around later, if (usually) required. Never fails, for the same
    // reason as `write_byte`.
    fn reserve_header_space(&mut self) -> usize {
        let start = self.pos;
        let end = start + MAX_HEADER_LEN;
        // Zero-fill only the portion actually backed by `buf`. The header is always fully
        // overwritten by `end_array`/`end_map` before it could otherwise be read, so there is no
        // need to fill any portion that lands in `overflow_scratch` or beyond.
        if start < self.buf.len() {
            let fill_end = end.min(self.buf.len());
            self.buf[start..fill_end].fill(0);
        }
        self.pos = end;
        start
    }

    // Read the logical byte at absolute position `idx`, if it is backed by `buf` or
    // `overflow_scratch`.
    fn logical_read(&self, idx: usize) -> Option<u8> {
        logical_read_at(self.buf, self.overflow_scratch.as_deref(), idx)
    }

    // Shift a logical byte range left (dest <= src_start is required, as used by header
    // shrink-on-close patch-up).
    fn logical_copy_within(&mut self, src_start: usize, src_end: usize, dest_start: usize) {
        for i in 0..src_end - src_start {
            let byte = self.logical_read(src_start + i).unwrap_or(0);
            if !logical_write_at(
                self.buf,
                self.overflow_scratch.as_deref_mut(),
                dest_start + i,
                byte,
            ) {
                self.overflow_lost = true;
            }
        }
    }

    // Write a real byte slice starting at logical position `start`.
    fn logical_write_slice(&mut self, start: usize, data: &[u8]) {
        for (i, &b) in data.iter().enumerate() {
            if !logical_write_at(self.buf, self.overflow_scratch.as_deref_mut(), start + i, b) {
                self.overflow_lost = true;
            }
        }
    }

    // Called whenever nesting depth returns to zero, i.e. once the complete shape of the
    // top-level item being encoded is known. Reports the exact total number of bytes required
    // if the logical position ended up past the real buffer, or if any byte was permanently
    // lost to an exhausted (or absent) overflow scratch buffer along the way.
    fn check_complete(&self) -> Result<(), EncodeError> {
        if self.overflow_lost || self.pos > self.buf.len() {
            Err(EncodeError::OutOfSpace(self.pos))
        } else {
            Ok(())
        }
    }

    fn can_accept_item(&self) -> Result<(), EncodeError> {
        if self.depth == 0 {
            return Ok(());
        }
        let parent = &self.frame_stack[self.depth - 1];
        if parent.kind == ContainerKind::Tag && parent.count == 1 {
            return Err(EncodeError::TagRequiresSingleItem);
        }
        Ok(())
    }

    fn record_item_for_parent(&mut self) -> Result<(), EncodeError> {
        if self.depth == 0 {
            return Ok(());
        }
        let parent = &mut self.frame_stack[self.depth - 1];
        match parent.kind {
            ContainerKind::Array => {
                parent.count = parent
                    .count
                    .checked_add(1)
                    .ok_or(EncodeError::InvalidValue)?;
            }
            ContainerKind::Map => {
                if parent.map_expect_key {
                    parent.map_expect_key = false;
                } else {
                    parent.map_expect_key = true;
                    parent.count = parent
                        .count
                        .checked_add(1)
                        .ok_or(EncodeError::InvalidValue)?;
                }
            }
            ContainerKind::Tag => {
                if parent.count == 1 {
                    return Err(EncodeError::TagRequiresSingleItem);
                }
                parent.count = parent
                    .count
                    .checked_add(1)
                    .ok_or(EncodeError::InvalidValue)?;
            }
        }
        Ok(())
    }

    // Write a u64 as a big-endian value.
    fn write_be(&mut self, value: u64, bytes: usize) -> Result<(), EncodeError> {
        let be = value.to_be_bytes();
        for b in &be[8 - bytes..] {
            self.write_byte(*b)?;
        }
        Ok(())
    }

    // Begin creation of an array. reserve space for a header and start a new nesting level.
    fn begin_array(&mut self, expected_len: Option<u64>) -> Result<(), EncodeError> {
        if self.depth == self.max_depth {
            return Err(EncodeError::NestingTooDeep);
        }
        self.can_accept_item()?;
        let (start, header_len, header_reserved) = if let Some(len) = expected_len {
            let header_len = header_len(len);
            let start = self.pos;
            self.write_header(MAJOR_ARRAY, len)?;
            (start, header_len, false)
        } else {
            let start = self.reserve_header_space();
            (start, 0, true)
        };
        if let Err(err) = self.record_item_for_parent() {
            self.pos = start;
            return Err(err);
        }
        self.frame_stack[self.depth] = ContainerFrame {
            start,
            header_len,
            header_reserved,
            count: 0,
            kind: ContainerKind::Array,
            map_expect_key: true,
            expected_len,
        };
        self.depth += 1;
        Ok(())
    }

    // End an array. Fix up header for correct number of items, check that the number of items
    // matches the expected number for arrays of fixed size and decrease the nesting levels.
    fn end_array(&mut self) -> Result<(), EncodeError> {
        if self.depth == 0 {
            return Err(EncodeError::InvalidValue);
        }
        self.depth -= 1;
        let frame = self.frame_stack[self.depth];
        if frame.kind != ContainerKind::Array {
            return Err(EncodeError::InvalidValue);
        }
        if let Some(expected) = frame.expected_len
            && expected != frame.count
        {
            return Err(EncodeError::InvalidValue);
        }
        if frame.header_reserved {
            let (header, header_len) = encode_header_bytes(MAJOR_ARRAY, frame.count);
            let data_start = frame.start + MAX_HEADER_LEN;
            let data_end = self.pos;
            let shift = MAX_HEADER_LEN - header_len;
            if shift > 0 {
                self.logical_copy_within(data_start, data_end, frame.start + header_len);
            }
            self.logical_write_slice(frame.start, &header[..header_len]);
            self.pos = data_end - shift;
        }
        if self.depth == 0 {
            self.check_complete()?;
        }
        Ok(())
    }

    // Begin creation of a CBOR map. Start a new nesting level and reserve header space.
    fn begin_map(&mut self, expected_len: Option<u64>) -> Result<(), EncodeError> {
        if self.depth == self.max_depth {
            return Err(EncodeError::NestingTooDeep);
        }
        self.can_accept_item()?;
        let (start, header_len, header_reserved) = if let Some(len) = expected_len {
            let header_len = header_len(len);
            let start = self.pos;
            self.write_header(MAJOR_MAP, len)?;
            (start, header_len, false)
        } else {
            let start = self.reserve_header_space();
            (start, 0, true)
        };
        if let Err(err) = self.record_item_for_parent() {
            self.pos = start;
            return Err(err);
        }
        self.frame_stack[self.depth] = ContainerFrame {
            start,
            header_len,
            header_reserved,
            count: 0,
            kind: ContainerKind::Map,
            map_expect_key: true,
            expected_len,
        };
        self.depth += 1;
        Ok(())
    }

    // Fix up headers and perform checks when all key-value pairs in a map have been defined
    // at a given nesting level.
    fn end_map(&mut self) -> Result<(), EncodeError> {
        if self.depth == 0 {
            return Err(EncodeError::InvalidValue);
        }
        self.depth -= 1;
        let frame = self.frame_stack[self.depth];
        if frame.kind != ContainerKind::Map {
            return Err(EncodeError::InvalidValue);
        }
        if !frame.map_expect_key {
            return Err(EncodeError::InvalidValue);
        }
        if let Some(expected) = frame.expected_len
            && expected != frame.count
        {
            return Err(EncodeError::InvalidValue);
        }
        let data_start = if frame.header_reserved {
            frame.start + MAX_HEADER_LEN
        } else {
            frame.start + frame.header_len
        };
        let data_end = self.pos;
        // Duplicate-key/sort checks read through the logical view, so they remain exact even
        // when the map's content extends past `buf.len()` into `overflow_scratch`. Without an
        // overflow scratch buffer, any logically-out-of-range byte reads as unavailable and the
        // checks are skipped for spans that reach past `buf.len()` — documented as a narrow,
        // harmless limitation on `Encoder::new_with_overflow`, since the overall encode always
        // fails in that scenario regardless of which error variant would otherwise be reported.
        #[cfg(feature = "deterministic_encoding")]
        if let Some(scratch) = self.deterministic_scratch.as_deref_mut() {
            // sort_map_entries also detects duplicates. Deterministic encoding is only usable
            // with `Encoder::new_deterministic`, which never has an overflow scratch, so a
            // logically-overflowing map here always errors; skip the sort/dup check just as
            // `check_duplicate_keys` below would.
            if data_end <= self.buf.len() {
                sort_map_entries(self.buf, data_start, data_end, frame.count, scratch)?;
            }
        }
        #[cfg(not(feature = "deterministic_encoding"))]
        self.check_duplicate_keys(data_start, data_end, frame.count)?;
        #[cfg(feature = "deterministic_encoding")]
        if self.deterministic_scratch.is_none() {
            self.check_duplicate_keys(data_start, data_end, frame.count)?;
        }
        if frame.header_reserved {
            let (header, header_len) = encode_header_bytes(MAJOR_MAP, frame.count);
            let shift = MAX_HEADER_LEN - header_len;
            if shift > 0 {
                self.logical_copy_within(data_start, data_end, frame.start + header_len);
            }
            self.logical_write_slice(frame.start, &header[..header_len]);
            self.pos = data_end - shift;
        }
        if self.depth == 0 {
            self.check_complete()?;
        }
        Ok(())
    }

    // Create a tag. Increase nesting level.
    fn begin_tag(&mut self, tag: u64) -> Result<(), EncodeError> {
        if self.depth == self.max_depth {
            return Err(EncodeError::NestingTooDeep);
        }
        self.can_accept_item()?;
        self.write_tag_header(tag)?;
        self.record_item_for_parent()?;

        self.frame_stack[self.depth] = ContainerFrame {
            start: 0,
            header_len: 0,
            header_reserved: false,
            count: 0,
            kind: ContainerKind::Tag,
            map_expect_key: true,
            expected_len: None,
        };
        self.depth += 1;
        Ok(())
    }

    // End a tagged value. Update the tag value to correct length and ensure that only a single
    // item was tagged - then decrease nesting level.
    fn end_tag(&mut self) -> Result<(), EncodeError> {
        if self.depth == 0 {
            return Err(EncodeError::InvalidValue);
        }
        self.depth -= 1;
        let frame = self.frame_stack[self.depth];
        if frame.kind != ContainerKind::Tag {
            return Err(EncodeError::InvalidValue);
        }
        if frame.count != 1 {
            return Err(EncodeError::TagRequiresSingleItem);
        }
        if self.depth == 0 {
            self.check_complete()?;
        }
        Ok(())
    }
}

/******************************************************************************
 * ArrayBuilder - Builder pattern for arrays.
 ******************************************************************************/

/// Builder pattern for CBOR arrays.
///
/// Typically, the library user will not directly construct an instance of [`ArrayBuilder`]
/// but will instead obtain one from an [`Encoder`] instance. This ensures that the array
/// size, bounds and contents are properly constructed and mapped in the buffer when the
/// `end` method is called.
///
/// The `ArrayBuilder` obtained by calling [`Encoder::array()`] is mutable because the underlying
/// (borrowed) buffer gets mutated during calls to `ArrayBuilder` methods.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::Encoder;
/// # use tps_minicbor::EncodeError;
/// # fn main() -> Result<(), EncodeError> {
///
/// let mut buf = [0u8; 16];
/// let mut enc = Encoder::<4>::new(&mut buf);
/// // obtain a correctly initialized instance of `ArrayBuilder`.
/// let mut array = enc.array()?;
/// array.insert(1u8)?;
/// array.insert(2u8)?;
/// array.end();
/// assert_eq!(enc.encoded(), &[0x82, 0x01, 0x02]);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub struct ArrayBuilder<'a, 'b, const N: usize = DEFAULT_NESTING_DEPTH> {
    enc: &'b mut Encoder<'a, N>,
    // Set when this array is the sole item of an enclosing tag, so that `end()` also closes
    // the tag frame.
    close_tag: bool,
}

/// Implementation of the builder pattern for CBOR arrays.
impl<'a, 'b, const N: usize> ArrayBuilder<'a, 'b, N> {
    /// Insert by value into a CBOR array. The inserted value can be any type that implements
    /// the [`EncodeItem`] trait.
    pub fn insert<T: EncodeItem>(&mut self, value: T) -> Result<&mut Self, EncodeError> {
        self.enc.insert(value)?;
        Ok(self)
    }

    /// Insert by reference in a CBOR array. The inserted reference is any type that
    /// implements the [`EncodeItem`] trait.
    pub fn insert_ref<T: EncodeItem + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<&mut Self, EncodeError> {
        self.enc.insert_ref(value)?;
        Ok(self)
    }

    /// Insert a CBOR array into an array.
    ///
    /// This returns a new instance of [`ArrayBuilder`] and increases the nesting depth, the total
    /// of which must not exceed the maximum depth defined for the underlying [`Encoder`] instance
    /// when it was created.
    ///
    /// When nesting arrays and maps, it is an error not to close all nesting levels before
    /// returning the encoded item.
    pub fn array(&mut self) -> Result<ArrayBuilder<'a, '_, N>, EncodeError> {
        self.enc.array()
    }

    /// Insert a CBOR array with a known fixed length into an array.
    ///
    /// This returns a new instance of [`ArrayBuilder`] and increases the nesting depth, the total
    /// of which must not exceed the maximum depth defined for the underlying [`Encoder`] instance
    /// when it was created.
    ///
    /// When nesting arrays and maps, it is an error not to close all nesting levels before
    /// returning the encoded item.
    ///
    /// The number of items in the array is checked for correctness when [`ArrayBuilder::end`]
    /// is called.
    pub fn array_len(&mut self, len: u64) -> Result<ArrayBuilder<'a, '_, N>, EncodeError> {
        self.enc.array_with_len(len)
    }

    /// Insert a CBOR map into an array.
    ///
    /// This returns a new instance of [`MapBuilder`] and increases the nesting depth, the total
    /// of which must not exceed the maximum depth defined for the underlying [`Encoder`] instance
    /// when it was created.
    ///
    /// When nesting arrays and maps, it is an error not to close all nesting levels before
    /// returning the encoded item.
    pub fn map(&mut self) -> Result<MapBuilder<'a, '_, N>, EncodeError> {
        self.enc.map()
    }

    /// Insert a CBOR map with a known fixed number of key-value pairs into an array.
    ///
    /// This returns a new instance of [`MapBuilder`] and increases the nesting depth, the total
    /// of which must not exceed the maximum depth defined for the underlying [`Encoder`] instance
    /// when it was created.
    ///
    /// When nesting arrays and maps, it is an error not to close all nesting levels before
    /// returning the encoded item.
    ///
    /// The number of items in the array is checked for correctness when [`ArrayBuilder::end`]
    /// is called.
    pub fn map_len(&mut self, len: u64) -> Result<MapBuilder<'a, '_, N>, EncodeError> {
        self.enc.map_with_len(len)
    }

    /// Insert a CBOR tag into an array. The Tag must have a single item associated with it.
    ///
    /// This returns a new instance of [`TagBuilder`] and increases the nesting depth, the total
    /// of which must not exceed the maximum depth defined for the underlying [`Encoder`] instance
    /// when it was created.
    pub fn tag(&mut self, tag: u64) -> Result<TagBuilder<'a, '_, N>, EncodeError> {
        self.enc.tag(tag)
    }

    /// End the construction of a CBOR array.
    ///
    /// This function will update the item header with number of items in the array, raising an
    /// error if a fixed-size array has the wrong number of entries. It will also decrease the
    /// nesting depth by one.
    ///
    /// If this array was created via [`TagBuilder::array`] or [`TagBuilder::array_len`], this
    /// also closes the enclosing tag.
    pub fn end(self) -> Result<(), EncodeError> {
        self.enc.end_array()?;
        if self.close_tag {
            self.enc.end_tag()?;
        }
        Ok(())
    }
}

/******************************************************************************
 * MapBuilder - Builder pattern for maps.
 ******************************************************************************/

/// Builder pattern for CBOR maps.
///
/// Typically, the library user will not directly construct an instance of [`MapBuilder`]
/// but will instead obtain one from an [`Encoder`] instance or one of the other builders.
/// This ensures that the map size, bounds and contents are properly constructed and mapped
/// in the buffer when the `end` method is called.
///
/// `MapBuilder` is always mutable because the underlying (borrowed) buffer gets mutated during
/// calls to `MapBuilder` methods.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::Encoder;
/// # use tps_minicbor::EncodeError;
/// # fn main() -> Result<(), EncodeError> {
///
/// let mut buf = [0u8; 64];
/// let mut encoder = Encoder::<4>::new(&mut buf);
///
/// let mut map = encoder.map()?;
/// map.insert_key_value("a", 1u8)?;
/// map.insert_key_value("b", "c")?;
/// map.end();
///
/// let expected = [0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x61, 0x63];
/// assert_eq!(encoder.encoded(), expected);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub struct MapBuilder<'a, 'b, const N: usize = DEFAULT_NESTING_DEPTH> {
    enc: &'b mut Encoder<'a, N>,
    // Set when this map is the sole item of an enclosing tag, so that `end()` also closes
    // the tag frame.
    close_tag: bool,
}

impl<'a, 'b, const N: usize> MapBuilder<'a, 'b, N> {
    /// Insert a key-value pair into a map. Both the key and value implement the [`EncodeItem`] trait.
    ///
    /// This function is useful for simple cases. [`MapBuilder::key`] must be used for more complex
    /// map structures (e.g. nested arrays, tags and the like).
    pub fn insert_key_value<K: EncodeItem, V: EncodeItem>(
        &mut self,
        key: K,
        value: V,
    ) -> Result<&mut Self, EncodeError> {
        self.enc.insert(key)?;
        self.enc.insert(value)?;
        Ok(self)
    }

    /// Insert key and value by reference.
    ///
    /// This function is useful for simple cases. [`MapBuilder::key`] must be used for more complex
    /// map structures (e.g. nested arrays, tags and the like).
    pub fn insert_key_value_ref<K: EncodeItem + ?Sized, V: EncodeItem + ?Sized>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<&mut Self, EncodeError> {
        self.enc.insert_ref(key)?;
        self.enc.insert_ref(value)?;
        Ok(self)
    }

    /// Insert a key value, returning a [`MapValueBuilder`], which allows construction of more
    /// complex CBOR maps that include e.g. nested arrays, maps, tagged items and the like.
    pub fn key<K: EncodeItem>(
        &mut self,
        key: K,
    ) -> Result<MapValueBuilder<'a, '_, N>, EncodeError> {
        self.enc.insert(key)?;
        Ok(MapValueBuilder { enc: self.enc })
    }

    /// End the construction of a CBOR map.
    ///
    /// This function will update the item header with number of key-value pairs in the map,
    /// raising an error if a fixed-size array has the wrong number of entries. It will also
    /// decrease the nesting depth by one.
    ///
    /// If this map was created via [`TagBuilder::map`] or [`TagBuilder::map_len`], this also
    /// closes the enclosing tag.
    pub fn end(self) -> Result<(), EncodeError> {
        self.enc.end_map()?;
        if self.close_tag {
            self.enc.end_tag()?;
        }
        Ok(())
    }
}

/// Builder for CBOR map values.
#[must_use]
pub struct MapValueBuilder<'a, 'b, const N: usize = DEFAULT_NESTING_DEPTH> {
    enc: &'b mut Encoder<'a, N>,
}

impl<'a, 'b, const N: usize> MapValueBuilder<'a, 'b, N> {
    /// Insert a single value as the map value.
    pub fn insert<V: EncodeItem>(self, value: V) -> Result<(), EncodeError> {
        self.enc.insert(value)?;
        Ok(())
    }

    /// Insert a CBOR array as the map value, returning an [`ArrayBuilder`].
    pub fn array(self) -> Result<ArrayBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_array(None)?;
        Ok(ArrayBuilder {
            enc: self.enc,
            close_tag: false,
        })
    }

    /// Insert a fixed-length CBOR array as the map value, returning an [`ArrayBuilder`].
    pub fn array_len(self, len: u64) -> Result<ArrayBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_array(Some(len))?;
        Ok(ArrayBuilder {
            enc: self.enc,
            close_tag: false,
        })
    }

    /// Insert a CBOR map as the map value, returning a [`MapBuilder`].
    pub fn map(self) -> Result<MapBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_map(None)?;
        Ok(MapBuilder {
            enc: self.enc,
            close_tag: false,
        })
    }

    /// Insert a fixed-length CBOR map as the map value, returning a [`MapBuilder`].
    pub fn map_len(self, len: u64) -> Result<MapBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_map(Some(len))?;
        Ok(MapBuilder {
            enc: self.enc,
            close_tag: false,
        })
    }

    /// Insert a CBOR tag as the map value, returning a [`TagBuilder`].
    pub fn tag(self, tag: u64) -> Result<TagBuilder<'a, 'b, N>, EncodeError> {
        self.enc.tag(tag)
    }
}

/******************************************************************************
 * Tags
 ******************************************************************************/

/// CBOR tag wrapper for a single value.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{Encoder, Tag};
///
/// let mut buf = [0u8; 16];
/// let mut enc = Encoder::<4>::new(&mut buf);
/// enc.insert(Tag { tag: 24, value: &b"dIETF"[..] }).unwrap();
/// assert_eq!(enc.encoded(), &[0xd8, 0x18, 0x45, 0x64, 0x49, 0x45, 0x54, 0x46]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tag<T> {
    /// The CBOR tag number (Major Type 6).
    pub tag: u64,
    /// The tagged value.
    pub value: T,
}

/// Builder for CBOR tags.
#[must_use]
pub struct TagBuilder<'a, 'b, const N: usize = DEFAULT_NESTING_DEPTH> {
    enc: &'b mut Encoder<'a, N>,
}

impl<'a, 'b, const N: usize> TagBuilder<'a, 'b, N> {
    /// Add a value to a previously constructed tag. The value must implement
    /// the [`EncodeItem`] trait.
    pub fn item<T: EncodeItem>(self, value: T) -> Result<(), EncodeError> {
        self.enc.insert(value)?;
        self.enc.end_tag()
    }

    /// Add a value by reference to a previously constructed tag. The value must implement
    /// the [`EncodeItem`] trait.
    pub fn item_ref<T: EncodeItem + ?Sized>(self, value: &T) -> Result<(), EncodeError> {
        self.enc.insert_ref(value)?;
        self.enc.end_tag()
    }

    /// Tag a CBOR array, returning an [`ArrayBuilder`] for the array's contents.
    ///
    /// The tag is closed automatically when [`ArrayBuilder::end`] is called on the
    /// returned builder.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    /// # use tps_minicbor::EncodeError;
    /// # fn main() -> Result<(), EncodeError> {
    ///
    /// let mut buf = [0u8; 16];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// let mut array = enc.tag(1)?.array()?;
    /// array.insert(1u8)?;
    /// array.end()?;
    /// assert_eq!(enc.encoded(), &[0xc1, 0x81, 0x01]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn array(self) -> Result<ArrayBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_array(None)?;
        Ok(ArrayBuilder {
            enc: self.enc,
            close_tag: true,
        })
    }

    /// Tag a CBOR array with a known fixed length, returning an [`ArrayBuilder`] for the
    /// array's contents.
    ///
    /// The tag is closed automatically when [`ArrayBuilder::end`] is called on the
    /// returned builder.
    pub fn array_len(self, len: u64) -> Result<ArrayBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_array(Some(len))?;
        Ok(ArrayBuilder {
            enc: self.enc,
            close_tag: true,
        })
    }

    /// Tag a CBOR map, returning a [`MapBuilder`] for the map's contents.
    ///
    /// The tag is closed automatically when [`MapBuilder::end`] is called on the
    /// returned builder.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tps_minicbor::Encoder;
    /// # use tps_minicbor::EncodeError;
    /// # fn main() -> Result<(), EncodeError> {
    ///
    /// let mut buf = [0u8; 16];
    /// let mut enc = Encoder::<4>::new(&mut buf);
    /// let mut map = enc.tag(1)?.map()?;
    /// map.insert_key_value("a", 1u8)?;
    /// map.end()?;
    /// assert_eq!(enc.encoded(), &[0xc1, 0xa1, 0x61, 0x61, 0x01]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn map(self) -> Result<MapBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_map(None)?;
        Ok(MapBuilder {
            enc: self.enc,
            close_tag: true,
        })
    }

    /// Tag a CBOR map with a known fixed number of key-value pairs, returning a
    /// [`MapBuilder`] for the map's contents.
    ///
    /// The tag is closed automatically when [`MapBuilder::end`] is called on the
    /// returned builder.
    pub fn map_len(self, len: u64) -> Result<MapBuilder<'a, 'b, N>, EncodeError> {
        self.enc.begin_map(Some(len))?;
        Ok(MapBuilder {
            enc: self.enc,
            close_tag: true,
        })
    }
}

/******************************************************************************
 * Checked Simple values (encode onto a single byte)
 ******************************************************************************/

/// CBOR simple value (0..=19, 32..=255).
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{Encoder, Simple};
/// # use tps_minicbor::EncodeError;
/// # fn main() -> Result<(), EncodeError> {
/// let mut buf = [0u8; 1];
/// let mut enc = Encoder::<1>::new(&mut buf);
/// let simple = Simple::new(16)?;
/// enc.insert(simple)?;
/// assert_eq!(enc.encoded(), &[0xf0]);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Simple(u8);

/// Empty structure used to represent the undefined CBOR Simple value in
/// conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Undefined;

impl Simple {
    /// Create a Simple value from a u8.
    pub fn new(value: u8) -> Result<Self, EncodeError> {
        match value {
            0..=19 | 32..=255 => Ok(Self(value)),
            _ => Err(EncodeError::InvalidValue),
        }
    }

    /// Return the internal u8 representation of a Simple.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// CBOR undefined value.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{Encoder, Undefined};
///
/// let mut buf = [0u8; 8];
/// let mut enc = Encoder::<4>::new(&mut buf);
/// enc.insert(Undefined).unwrap();
/// assert_eq!(enc.encoded(), &[0xf7]);
/// ```
impl TryFrom<u8> for Simple {
    type Error = EncodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/******************************************************************************
 * Encode trait and its implementations
 ******************************************************************************/
/// Encode a value as a CBOR item.
///
/// The trait defines the two operations required for encoding a CBOR item:
///
/// - The encoded size of the item, provided with the help of a [`Sizer`] instance.
/// - A function to encode the instance into a CBOR buffer.
pub trait EncodeItem {
    /// Compute the encoded byte length into a [`Sizer`].
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError>;
    /// Write the CBOR encoding into `e`.
    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError>;
}

impl EncodeItem for u128 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        if *self > u64::MAX as u128 {
            return Err(EncodeError::InvalidValue);
        }
        s.uint(*self as u64)
    }

    /// Encode a u128 value. Note that in fact this only encodes the range 0..u64::MAX.
    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        if *self > u64::MAX as u128 {
            return Err(EncodeError::InvalidValue);
        }
        e.uint(*self as u64)
    }
}

impl EncodeItem for u64 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.uint(*self)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.uint(*self)
    }
}

impl EncodeItem for u32 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.uint(*self as u64)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.uint(*self as u64)
    }
}

impl EncodeItem for u16 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.uint(*self as u64)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.uint(*self as u64)
    }
}

impl EncodeItem for u8 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.uint(*self as u64)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.uint(*self as u64)
    }
}

impl EncodeItem for i128 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        if *self >= 0 {
            let value = *self as u128;
            if value > u64::MAX as u128 {
                return Err(EncodeError::InvalidValue);
            }
            s.uint(value as u64)
        } else {
            let n = (-1i128)
                .checked_sub(*self)
                .ok_or(EncodeError::InvalidValue)?;
            if n > u64::MAX as i128 {
                return Err(EncodeError::InvalidValue);
            }
            s.add(header_len(n as u64))
        }
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        if *self >= 0 {
            let value = *self as u128;
            if value > u64::MAX as u128 {
                return Err(EncodeError::InvalidValue);
            }
            e.uint(value as u64)
        } else {
            let n = (-1i128)
                .checked_sub(*self)
                .ok_or(EncodeError::InvalidValue)?;
            if n > u64::MAX as i128 {
                return Err(EncodeError::InvalidValue);
            }
            e.write_header(MAJOR_NEGATIVE, n as u64)
        }
    }
}

impl EncodeItem for i64 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        if *self >= 0 {
            s.uint(*self as u64)
        } else {
            s.nint(*self)
        }
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        if *self >= 0 {
            e.uint(*self as u64)
        } else {
            e.neg_int(*self)
        }
    }
}

impl EncodeItem for i32 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        (*self as i64).size(s)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        (*self as i64).encode(e)
    }
}

impl EncodeItem for i16 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        (*self as i64).size(s)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        (*self as i64).encode(e)
    }
}

impl EncodeItem for i8 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        (*self as i64).size(s)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        (*self as i64).encode(e)
    }
}

impl EncodeItem for bool {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.add(1)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.bool(*self)
    }
}

impl EncodeItem for () {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.add(1)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.null()
    }
}

impl EncodeItem for Undefined {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.add(1)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.undefined()
    }
}

impl EncodeItem for Simple {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        match self.0 {
            0..=19 => s.add(1),
            _ => s.add(2),
        }
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.simple(self.0)
    }
}

impl<T: EncodeItem> EncodeItem for Tag<T> {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.add(header_len(self.tag))?;
        self.value.size(s)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.tag(self.tag)?.item_ref(&self.value)
    }
}

impl EncodeItem for &[u8] {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.bytes(self)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.bytes(self)
    }
}

impl EncodeItem for [u8] {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.bytes(self)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.bytes(self)
    }
}

impl EncodeItem for &str {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.text(self)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.text(self)
    }
}

impl EncodeItem for str {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.text(self)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        e.text(self)
    }
}

/******************************************************************************
 * Floating point support (featurized)
 ******************************************************************************/

/// Internal float selection helper — represents the narrowest float type that preserves a value.
#[cfg(feature = "float")]
pub enum FloatType {
    /// Value fits in a 16-bit float.
    F16(f16),
    /// Value fits in a 32-bit float.
    F32(f32),
    /// Value requires a 64-bit float.
    F64(f64),
}

#[cfg(feature = "float")]
impl EncodeItem for f64 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        match select_float(*self) {
            FloatType::F16(_) => s.add(3),
            FloatType::F32(_) => s.add(5),
            FloatType::F64(_) => s.add(9),
        }
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        match select_float(*self) {
            FloatType::F16(value) => e.write_float16(value.to_bits()),
            FloatType::F32(value) => e.write_float32(value.to_bits()),
            FloatType::F64(value) => e.write_float64(value.to_bits()),
        }
    }
}

#[cfg(feature = "float")]
impl EncodeItem for f32 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        (*self as f64).size(s)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        (*self as f64).encode(e)
    }
}

#[cfg(feature = "float")]
impl EncodeItem for f16 {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        self.to_f64().size(s)
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        self.to_f64().encode(e)
    }
}

// Write the float in the smallest possible form.
#[cfg(feature = "float")]
pub(crate) fn select_float(value: f64) -> FloatType {
    if value.is_nan() {
        return FloatType::F16(f16::NAN);
    }

    let value_bits = value.to_bits();
    let f16_val = f16::from_f64(value);
    if f16_val.to_f64().to_bits() == value_bits {
        return FloatType::F16(f16_val);
    }

    let f32_val = value as f32;
    if (f32_val as f64).to_bits() == value_bits {
        return FloatType::F32(f32_val);
    }

    FloatType::F64(value)
}

/******************************************************************************
 * Sizer implementation
 ******************************************************************************/

/// Calculates the encoded byte length of a CBOR value without writing to a buffer.
///
/// Construct with [`Sizer::new`], call the same method family as [`EncodeItem`], then
/// read the result with [`Sizer::len`].
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{Sizer, EncodeItem};
///
/// let mut s = Sizer::new();
/// 42u64.size(&mut s).unwrap();
/// assert_eq!(s.len(), 2);
/// ```
pub struct Sizer {
    len: usize,
}

impl Default for Sizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sizer {
    /// Create a new zeroed `Sizer`.
    #[must_use]
    pub const fn new() -> Self {
        Self { len: 0 }
    }

    /// Return the accumulated byte count.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Return `true` if no bytes have been accounted for.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Accumulate `n` raw bytes.
    pub fn add(&mut self, n: usize) -> Result<(), EncodeError> {
        self.len = self.len.checked_add(n).ok_or(EncodeError::InvalidValue)?;
        Ok(())
    }

    /// Accumulate the header bytes for an array of `len` items.
    pub fn array_len(&mut self, len: u64) -> Result<(), EncodeError> {
        self.add(header_len(len))?;
        Ok(())
    }

    /// Accumulate the header bytes for a map of `len` pairs.
    pub fn map_len(&mut self, len: u64) -> Result<(), EncodeError> {
        self.add(header_len(len))?;
        Ok(())
    }

    /// Accumulate the encoded size of a CBOR unsigned integer.
    pub fn uint(&mut self, value: u64) -> Result<(), EncodeError> {
        self.add(header_len(value))
    }

    /// Accumulate the encoded size of a CBOR negative integer.
    pub fn nint(&mut self, value: i64) -> Result<(), EncodeError> {
        if value >= 0 {
            return Err(EncodeError::InvalidValue);
        }
        self.add(header_len(!value as u64))
    }

    /// Accumulate the encoded size of a CBOR byte string.
    pub fn bytes(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        self.add(header_len(bytes.len() as u64))?;
        self.add(bytes.len())
    }

    /// Accumulate the encoded size of a CBOR text string.
    pub fn text(&mut self, text: &str) -> Result<(), EncodeError> {
        self.add(header_len(text.len() as u64))?;
        self.add(text.len())
    }

    /// Accumulate the size of an [`EncodeItem`].
    pub fn insert<T: EncodeItem>(&mut self, value: T) -> Result<(), EncodeError> {
        value.size(self)
    }
}

/******************************************************************************
 * ContainerFrame implementation
 ******************************************************************************/

/// The `ContainerFrame` structure is used to hold information to enable the correct calculation
/// of length/size information when using nested structures (arrays, maps and tags), since this
/// information can ony be known at each level after that level has been completed.
#[derive(Clone, Copy)]
struct ContainerFrame {
    /// Starting point in buffer for encoding this container.
    start: usize,
    /// Length of header required to encode this item.
    header_len: usize,
    header_reserved: bool,
    count: u64,
    /// Type of container referred to by this frame.
    kind: ContainerKind,
    /// Are we expecting a map key next? (Only valid for maps where keys and values must be paired)
    map_expect_key: bool,
    expected_len: Option<u64>,
}

/// Used in the `kind` field of a [`ContainerFrame`] to indicate the type of container nested
/// by that frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    Array,
    Map,
    Tag,
}

impl ContainerFrame {
    /// Create an empty `ContainerFrame`.
    const fn empty() -> Self {
        Self {
            start: 0,
            header_len: 0,
            header_reserved: true,
            count: 0,
            kind: ContainerKind::Array,
            map_expect_key: true,
            expected_len: None,
        }
    }
}

/// Compute the encoded length for a CBOR value.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::encoded_len;
///
/// let len = encoded_len(&42u64).unwrap();
/// assert_eq!(len, 2);
/// ```
#[must_use = "returns the computed byte length"]
pub fn encoded_len<T: EncodeItem>(value: &T) -> Result<usize, EncodeError> {
    let mut s = Sizer::new();
    value.size(&mut s)?;
    Ok(s.len())
}

/// Encode a value into the provided buffer.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::encode;
///
/// let mut buf = [0u8; 16];
/// let len = encode(&42u64, &mut buf).unwrap();
/// assert_eq!(&buf[..len], &[0x18, 0x2a]);
/// ```
#[must_use = "returns the encoded byte count"]
pub fn encode<T: EncodeItem>(value: &T, out: &mut [u8]) -> Result<usize, EncodeError> {
    let mut s = Sizer::new();
    value.size(&mut s)?;
    if s.len() > out.len() {
        return Err(EncodeError::OutOfSpace(s.len()));
    }
    let mut e = Encoder::<DEFAULT_NESTING_DEPTH>::new(out);
    value.encode(&mut e)?;
    if e.len() != s.len() {
        return Err(EncodeError::InvalidValue);
    }
    Ok(e.len())
}

/// Encode a single CBOR value with deterministic map ordering.
///
/// This is a convenience wrapper around [`Encoder::new_deterministic`] for encoding
/// a single value. All maps within the value (including nested maps) are sorted
/// by key using RFC 8949 bytewise lexicographic ordering.
///
/// The scratch buffer must not alias the output buffer and should be at least as
/// large as the output buffer to handle worst-case maps.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{encode_deterministic, Encoder};
///
/// let mut buf = [0u8; 32];
/// let mut scratch = [0u8; 32];
/// let mut enc = Encoder::<4>::new_deterministic(&mut buf, &mut scratch).unwrap();
/// let mut map = enc.map().unwrap();
/// map.insert_key_value("b", 2u8).unwrap();
/// map.insert_key_value("a", 1u8).unwrap();
/// map.end().unwrap();
/// assert_eq!(enc.encoded(), &[0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02]);
/// ```
#[cfg(feature = "deterministic_encoding")]
#[must_use = "returns the encoded byte count"]
pub fn encode_deterministic<T: EncodeItem>(
    value: &T,
    out: &mut [u8],
    scratch: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut s = Sizer::new();
    value.size(&mut s)?;
    if s.len() > out.len() {
        return Err(EncodeError::OutOfSpace(s.len()));
    }
    let mut e = Encoder::<DEFAULT_NESTING_DEPTH>::new_deterministic(out, scratch)?;
    value.encode(&mut e)?;
    if e.len() != s.len() {
        return Err(EncodeError::InvalidValue);
    }
    Ok(e.len())
}

pub(crate) fn header_len(value: u64) -> usize {
    if value <= 23 {
        1
    } else if value <= 0xff {
        2
    } else if value <= 0xffff {
        3
    } else if value <= 0xffff_ffff {
        5
    } else {
        MAX_HEADER_LEN
    }
}

pub(crate) fn encode_header_bytes(major: u8, value: u64) -> ([u8; MAX_HEADER_LEN], usize) {
    let mut out = [0u8; MAX_HEADER_LEN];
    if value <= 23 {
        out[0] = (major << 5) | value as u8;
        (out, 1)
    } else if value <= 0xff {
        out[0] = (major << 5) | 24;
        out[1] = value as u8;
        (out, 2)
    } else if value <= 0xffff {
        out[0] = (major << 5) | 25;
        out[1] = (value >> 8) as u8;
        out[2] = value as u8;
        (out, 3)
    } else if value <= 0xffff_ffff {
        out[0] = (major << 5) | 26;
        out[1] = (value >> 24) as u8;
        out[2] = (value >> 16) as u8;
        out[3] = (value >> 8) as u8;
        out[4] = value as u8;
        (out, 5)
    } else {
        out[0] = (major << 5) | 27;
        out[1] = (value >> 56) as u8;
        out[2] = (value >> 48) as u8;
        out[3] = (value >> 40) as u8;
        out[4] = (value >> 32) as u8;
        out[5] = (value >> 24) as u8;
        out[6] = (value >> 16) as u8;
        out[7] = (value >> 8) as u8;
        out[8] = value as u8;
        (out, 9)
    }
}

/******************************************************************************
 * Functions supporting deterministic encoding
 ******************************************************************************/
// Read a header value as a u64 value.
#[cfg(feature = "deterministic_encoding")]
fn read_be_bytes(data: &[u8], offset: usize, len: usize) -> Result<u64, EncodeError> {
    if data.len() < offset + len {
        return Err(EncodeError::InvalidValue);
    }
    let mut out = 0u64;
    for i in 0..len {
        out = (out << 8) | (data[offset + i] as u64);
    }
    Ok(out)
}

// Determine the number of items following.
#[cfg(feature = "deterministic_encoding")]
fn item_len(data: &[u8]) -> Result<usize, EncodeError> {
    if data.is_empty() {
        return Err(EncodeError::InvalidValue);
    }
    let first = data[0];
    let major = first >> 5;
    let addl = first & 0x1f;
    let (value, header_len) = match addl {
        0..=23 => (addl as u64, 1usize),
        24 => (read_be_bytes(data, 1, 1)?, 2),
        25 => (read_be_bytes(data, 1, 2)?, 3),
        26 => (read_be_bytes(data, 1, 4)?, 5),
        27 => (read_be_bytes(data, 1, 8)?, 9),
        31 => return Err(EncodeError::InvalidValue),
        _ => return Err(EncodeError::InvalidValue),
    };
    match major {
        MAJOR_UNSIGNED | MAJOR_NEGATIVE => Ok(header_len),
        MAJOR_BYTES | MAJOR_TEXT => {
            let len = usize::try_from(value).map_err(|_| EncodeError::InvalidValue)?;
            let total = header_len
                .checked_add(len)
                .ok_or(EncodeError::InvalidValue)?;
            if data.len() < total {
                return Err(EncodeError::InvalidValue);
            }
            Ok(total)
        }
        MAJOR_ARRAY => {
            let mut offset = header_len;
            for _ in 0..value {
                let item = item_len(&data[offset..])?;
                offset = offset.checked_add(item).ok_or(EncodeError::InvalidValue)?;
            }
            Ok(offset)
        }
        MAJOR_MAP => {
            let total = value.checked_mul(2).ok_or(EncodeError::InvalidValue)?;
            let mut offset = header_len;
            for _ in 0..total {
                let item = item_len(&data[offset..])?;
                offset = offset.checked_add(item).ok_or(EncodeError::InvalidValue)?;
            }
            Ok(offset)
        }
        MAJOR_TAG => {
            let inner = item_len(&data[header_len..])?;
            let total = header_len
                .checked_add(inner)
                .ok_or(EncodeError::InvalidValue)?;
            Ok(total)
        }
        MAJOR_SIMPLE => match addl {
            0..=23 => Ok(header_len),
            24 => Ok(header_len),
            25 => Ok(3),
            26 => Ok(5),
            27 => Ok(9),
            _ => Err(EncodeError::InvalidValue),
        },
        _ => Err(EncodeError::InvalidValue),
    }
}

// Check that no two map keys in the encoded key-value pairs are bytewise equal.
// O(n^2) in pair_count, acceptable for the small maps typical in embedded use.
impl<'a, const N: usize> Encoder<'a, N> {
    // Total number of logical positions actually backed by storage (`buf` plus, if supplied,
    // `overflow_scratch`).
    fn logical_available_len(&self) -> usize {
        self.buf
            .len()
            .saturating_add(self.overflow_scratch.as_ref().map_or(0, |s| s.len()))
    }

    fn logical_read_be(&self, offset: usize, len: usize) -> Result<u64, EncodeError> {
        let mut out = 0u64;
        for i in 0..len {
            let b = self.logical_read(offset + i).ok_or(EncodeError::InvalidValue)?;
            out = (out << 8) | (b as u64);
        }
        Ok(out)
    }

    fn logical_bytes_eq(&self, a_start: usize, b_start: usize, len: usize) -> bool {
        for i in 0..len {
            if self.logical_read(a_start + i) != self.logical_read(b_start + i) {
                return false;
            }
        }
        true
    }

    // Analogous to the free function `item_len`, but reads through the logical view so it
    // remains correct even when part of the item is stored in `overflow_scratch` rather than
    // `buf`. `limit` bounds bstr/tstr length validation, mirroring `item_len`'s use of
    // `data.len()`.
    fn logical_item_len(&self, offset: usize, limit: usize) -> Result<usize, EncodeError> {
        let first = self.logical_read(offset).ok_or(EncodeError::InvalidValue)?;
        let major = first >> 5;
        let addl = first & 0x1f;
        let (value, header_len) = match addl {
            0..=23 => (addl as u64, 1usize),
            24 => (self.logical_read_be(offset + 1, 1)?, 2),
            25 => (self.logical_read_be(offset + 1, 2)?, 3),
            26 => (self.logical_read_be(offset + 1, 4)?, 5),
            27 => (self.logical_read_be(offset + 1, 8)?, 9),
            _ => return Err(EncodeError::InvalidValue),
        };
        match major {
            MAJOR_UNSIGNED | MAJOR_NEGATIVE => Ok(header_len),
            MAJOR_BYTES | MAJOR_TEXT => {
                let len = usize::try_from(value).map_err(|_| EncodeError::InvalidValue)?;
                let total = header_len
                    .checked_add(len)
                    .ok_or(EncodeError::InvalidValue)?;
                let end = offset.checked_add(total).ok_or(EncodeError::InvalidValue)?;
                if end > limit {
                    return Err(EncodeError::InvalidValue);
                }
                Ok(total)
            }
            MAJOR_ARRAY => {
                let mut off = offset + header_len;
                for _ in 0..value {
                    let item = self.logical_item_len(off, limit)?;
                    off = off.checked_add(item).ok_or(EncodeError::InvalidValue)?;
                }
                off.checked_sub(offset).ok_or(EncodeError::InvalidValue)
            }
            MAJOR_MAP => {
                let total = value.checked_mul(2).ok_or(EncodeError::InvalidValue)?;
                let mut off = offset + header_len;
                for _ in 0..total {
                    let item = self.logical_item_len(off, limit)?;
                    off = off.checked_add(item).ok_or(EncodeError::InvalidValue)?;
                }
                off.checked_sub(offset).ok_or(EncodeError::InvalidValue)
            }
            MAJOR_TAG => {
                let inner = self.logical_item_len(offset + header_len, limit)?;
                header_len
                    .checked_add(inner)
                    .ok_or(EncodeError::InvalidValue)
            }
            MAJOR_SIMPLE => match addl {
                0..=24 => Ok(header_len),
                25 => Ok(3),
                26 => Ok(5),
                27 => Ok(9),
                _ => Err(EncodeError::InvalidValue),
            },
            _ => Err(EncodeError::InvalidValue),
        }
    }

    // Check that no two map keys in the encoded key-value pairs (spanning the logical range
    // `data_start..data_end`) are bytewise equal. O(n^2) in pair_count, acceptable for the small
    // maps typical in embedded use.
    //
    // If the span extends past every byte actually backed by storage (`buf` plus, if supplied,
    // `overflow_scratch`), the check is skipped: `check_complete` will report
    // `EncodeError::OutOfSpace` for the encode regardless, so which error variant would
    // otherwise have been reported does not affect the correctness of any (non-existent)
    // successful output.
    fn check_duplicate_keys(
        &self,
        data_start: usize,
        data_end: usize,
        pair_count: u64,
    ) -> Result<(), EncodeError> {
        let mut key_starts: [usize; 64] = [0; 64];
        let mut key_lens: [usize; 64] = [0; 64];
        let n = usize::try_from(pair_count).map_err(|_| EncodeError::InvalidValue)?;
        if n > 64 {
            // Fallback: skip check for very large maps; deterministic_encoding covers this case.
            return Ok(());
        }
        if data_end > self.logical_available_len() {
            return Ok(());
        }
        let mut pos = data_start;
        for i in 0..n {
            let key_len = self.logical_item_len(pos, data_end)?;
            key_starts[i] = pos;
            key_lens[i] = key_len;
            pos = pos.checked_add(key_len).ok_or(EncodeError::InvalidValue)?;
            let val_len = self.logical_item_len(pos, data_end)?;
            pos = pos.checked_add(val_len).ok_or(EncodeError::InvalidValue)?;
        }
        for i in 0..n {
            for j in i + 1..n {
                if key_lens[i] == key_lens[j]
                    && self.logical_bytes_eq(key_starts[i], key_starts[j], key_lens[i])
                {
                    return Err(EncodeError::DuplicateKey);
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "deterministic_encoding")]
fn compare_keys(left: &[u8], right: &[u8]) -> core::cmp::Ordering {
    left.cmp(right)
}

// Sort the entries in a map by key
#[cfg(feature = "deterministic_encoding")]
fn sort_map_entries(
    buf: &mut [u8],
    entries_start: usize,
    entries_end: usize,
    pair_count: u64,
    scratch: &mut [u8],
) -> Result<(), EncodeError> {
    let entries_len = entries_end
        .checked_sub(entries_start)
        .ok_or(EncodeError::InvalidValue)?;
    if scratch.len() < entries_len {
        return Err(EncodeError::OutOfSpace(entries_len));
    }
    let src = &buf[entries_start..entries_end];
    let mut src_pos = 0usize;
    let mut scratch_len = 0usize;

    for _ in 0..pair_count {
        let key_len = item_len(&src[src_pos..])?;
        let key_start = src_pos;
        src_pos = src_pos
            .checked_add(key_len)
            .ok_or(EncodeError::InvalidValue)?;
        let val_len = item_len(&src[src_pos..])?;
        src_pos = src_pos
            .checked_add(val_len)
            .ok_or(EncodeError::InvalidValue)?;
        let entry_len = key_len
            .checked_add(val_len)
            .ok_or(EncodeError::InvalidValue)?;
        let entry = &src[key_start..key_start + entry_len];
        let key = &src[key_start..key_start + key_len];

        let mut insert_pos = scratch_len;
        let mut scan_pos = 0usize;
        while scan_pos < scratch_len {
            let existing_key_len = item_len(&scratch[scan_pos..scratch_len])?;
            let existing_key = &scratch[scan_pos..scan_pos + existing_key_len];
            let existing_val_len = item_len(&scratch[scan_pos + existing_key_len..scratch_len])?;
            let existing_entry_len = existing_key_len
                .checked_add(existing_val_len)
                .ok_or(EncodeError::InvalidValue)?;

            match compare_keys(key, existing_key) {
                core::cmp::Ordering::Less => {
                    insert_pos = scan_pos;
                    break;
                }
                core::cmp::Ordering::Equal => return Err(EncodeError::DuplicateKey),
                core::cmp::Ordering::Greater => {}
            }
            scan_pos = scan_pos
                .checked_add(existing_entry_len)
                .ok_or(EncodeError::InvalidValue)?;
        }

        if insert_pos < scratch_len {
            let new_len = scratch_len
                .checked_add(entry_len)
                .ok_or(EncodeError::InvalidValue)?;
            if new_len > scratch.len() {
                return Err(EncodeError::OutOfSpace(new_len));
            }
            scratch.copy_within(insert_pos..scratch_len, insert_pos + entry_len);
        }
        scratch[insert_pos..insert_pos + entry_len].copy_from_slice(entry);
        scratch_len = scratch_len
            .checked_add(entry_len)
            .ok_or(EncodeError::InvalidValue)?;
    }

    if src_pos != entries_len || scratch_len != entries_len {
        return Err(EncodeError::InvalidValue);
    }
    buf[entries_start..entries_end].copy_from_slice(&scratch[..entries_len]);
    Ok(())
}
