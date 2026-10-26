/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![no_std]
//! # tps_minicbor
//!
//! A CBOR encoder/decoder supporting deterministic encoding and optimized for
//! embedded and no_std targets. Compliance with RFC8949 has been a significant
//! focus in this implementation, with the intent that it can be used in most
//! applications of CBOR. The key exception to this rule is that indefinite
//! length encoding is not supported.
//!
//! # Features
//!
//! The default configuration of tps_minicbor does not require an allocator and is
//! largely intended to support `no_std` targets. It performs serialization to
//! and from provided byte buffers (which will usually be arrays on `no_std`
//! targets, but can be anything slice-like).
//!
//! The encoder performs preferred serialization as defined in RFC8949,
//! section 4.1.
//!
//! The following features from RFC8949 are supported on `no_std` targets:
//!
//! - Integer values are encoded in their smallest representation as major
//!   type 0 or 1. There are conversion functions to and from all Rust
//!   integer types for values that fit within the ranges defined for major
//!   types 0 and 1 (i.e. some u128 and i128 values cannot be serialized).
//!   - Negative integers are managed as a Rust `i64` types. This means that
//!     there are some infelicities with using the highest value negative
//!     CBOR values.
//! - Integer, `tstr`, and `bstr` values are all supported as map keys, encoded
//!   in the shortest form for integers.
//! - Simple values (range-checked at call-time).
//! - Support for CBOR tags. There is no special support for the IANA allocated
//!   _invalid tag_ numbers. Applications will need to manage these where
//!   relevant (if you do not know what this is, you probably don't care).
//! - Support for CBOR array and map types.
//!   - Arrays and maps are constructed using a builder pattern, with the
//!     length-related parameters being calculated and serialized automatically
//!     when `.end()` is called.
//!   - Arrays and maps can be nested to a user-controllable depth. There is
//!     a convenience typedef that allocates for support of a depth of four
//!     levels as a trade-off between convenience and memory size requirements.
//!
//! > Note: The way that automatic fix-up of array and map sizes is performed,
//! > in conjunction with support for nested structures, means that the encoding
//! > buffer provided needs to be slightly larger than the final encoded size of
//! > the CBOR structure.
//!
//! # Optional Features
//!
//! The following features are gated behind compile-time features.
//!
//! ## When `std` is enabled
//!
//! Under the _std_ feature.
//!
//! - `std::error::Error` implementations for [`EncodeError`] and [`DecodeError`].
//! - Required by, and implied by, the `deterministic_encoding`, `diagnostic`, and
//!   `edn` features.
//!
//! ## Standard tag values
//!
//! Under the _standard_tags_ feature:
//!
//! - Support for encoding and decoding standard tagged types defined in
//!   RFC 8949. Values are **not** checked on encode or decode. Upper layers
//!   are expected to manage this.
//!
//! ## Deterministic encoding
//!
//! Under the _deterministic_encoding_ feature:
//!
//! - Support for _deterministic encoding_ as defined in RFC 8949 section 4.2:
//!   - Use of preferred serialization (always true in tps_minicbor)
//!   - No use of indefinite length values (not supported)
//!   - Map keys (of any type, including `bstr`) are sorted by pure bytewise
//!     lexicographic comparison of their encoded bytes, per the core
//!     deterministic encoding requirements of RFC 8949 section 4.2.1.
//!   - Duplicate map keys are detected when [`MapBuilder::end`] is called.
//!     Duplicate keys cause [`EncodeError::DuplicateKey`] to be returned.
//!   - NaN values are always canonicalized to the quiet NaN encoded in the
//!     shortest form (`f16::NAN`), regardless of the input payload or
//!     signaling bit, as a deliberate crate policy for deterministic output.
//!
//! ## Floating point support
//!
//! The _float_ feature enables support for all CBOR floating point types. These
//! are encoded in their shortest form, as required for both preferred and
//! deterministic encoding
//!
//! With the `float` feature enabled, floating-point values are encoded using
//! the smallest CBOR float width that preserves the value, and NaNs are
//! canonicalized to the preferred half-precision NaN encoding.
//!
//! The _float_ feature uses the _f16_ crate to provide support for the CBOR
//! `f16` type.
//!
//! ## Diagnostic output
//!
//! The _diagnostic_ feature enables support for decoding CBOR into the diagnostic
//! format described in RFC 8949, section 8.
//!
//! The `Encoder` type uses a const-generic nesting depth parameter. For a
//! convenient default (depth 4), use [`EncoderStd`].
//!
//! # Examples
//!
//! Simplest example: encode an integer value on a buffer. The [`encode`] function is
//! provided as a convenience to encode a single CBOR value. Mostly you will use the
//! [`Encoder`] trait which supports a builder-like pattern.
//!
//! ```rust
//! use tps_minicbor::{encode, Encoder};
//! # use tps_minicbor::EncodeError;
//! # fn main() -> Result<(), EncodeError> {
//! let mut buf = [0u8; 16];
//! let len = encode(&42u64, &mut buf)?;
//! assert_eq!(&buf[..len], &[0x18, 0x2a]);
//!
//! let mut encoder = Encoder::<1>::new(&mut buf);
//! encoder.insert(42u64)?;
//! assert_eq!(&buf[..len], &[0x18, 0x2a]);
//! # Ok(())
//! # }
//! ```
//!
//! Encode values into an array. The nesting of array inserts is for clarity - it is not
//! required from a functional perspective.
//!
//! ```rust
//! use tps_minicbor::Encoder;
//! # use tps_minicbor::EncodeError;
//! # fn main() -> Result<(), EncodeError> {
//! let mut buf = [0u8; 16];
//! let mut encoder = Encoder::<2>::new(&mut buf);
//! let mut array = encoder.array()?;
//! {
//!     array.insert(1u8)?;
//!     array.insert(2u8)?;
//!     array.insert(3u8)?;
//! }
//! array.end()?;
//! assert_eq!(encoder.encoded(), &[0x83, 0x01, 0x02, 0x03]);
//! # Ok(())
//! # }
//! ```
//!
//! Encode a floating point value, assuming that the "float" feature is defined.
//!
//! ```rust
//! # #[cfg(feature = "float")]
//! # {
//! use tps_minicbor::encode;
//! let mut buf = [0u8; 16];
//! let len = encode(&1.5f32, &mut buf).unwrap();
//! assert_eq!(&buf[..len], &[0xf9, 0x3e, 0x00]);
//! # }
//! ```
//!
//! Examples showing how to use nesting depth. [`EncoderStd`] is a typedef for a CBOR
//! encoder with a maximum nesting depth of 4. If you want a different depth, supply
//! the required depth as a constant when creating the [`Encoder`].
//!
//! ```rust
//! use tps_minicbor::{EncoderStd, Encoder};
//!
//! let mut buf = [0u8; 16];
//! let mut enc = EncoderStd::new(&mut buf);
//! enc.insert(1u8).unwrap();
//! assert_eq!(enc.encoded(), &[0x01]);
//!
//! let mut buf = [0u8; 16];
//! let mut enc = Encoder::<8>::new(&mut buf);
//! enc.insert(2u8).unwrap();
//! assert_eq!(enc.encoded(), &[0x02]);
//! ```
//!
//! A simple example of decoding data:
//!
//! ```rust
//! use tps_minicbor::{Decoder, Item};
//! # use tps_minicbor::DecodeError;
//! # fn main() -> Result<(), DecodeError> {
//! let data = [0x01];
//! let mut dec = Decoder::new(&data);
//! let item = dec.decode()?;
//! assert!(matches!(item, Item::Unsigned(1)));
//! # Ok(())
//! # }
//! ```

// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set).
#[cfg(any(feature = "std", test))]
extern crate std;

#[cfg(feature = "float")]
pub use decoder::Float;

#[cfg(feature = "standard_tags")]
pub use tags::standard_tags;

#[cfg(feature = "float")]
pub use encoder::FloatType;
#[cfg(feature = "deterministic_encoding")]
pub use encoder::encode_deterministic;
pub use encoder::{
    ArrayBuilder, EncodeItem, Encoder, MapBuilder, MapValueBuilder, Simple, Sizer, Tag, TagBuilder,
    Undefined, encode, encoded_len,
};

pub use decoder::{Array, ArrayIter, Decode, Decoder, Item, Map, MapIter, TaggedItem, decode};

use core::ops::{Deref, DerefMut};

// CBOR major types
pub(crate) const MAJOR_UNSIGNED: u8 = 0;
pub(crate) const MAJOR_NEGATIVE: u8 = 1;
pub(crate) const MAJOR_BYTES: u8 = 2;
pub(crate) const MAJOR_TEXT: u8 = 3;
pub(crate) const MAJOR_ARRAY: u8 = 4;
pub(crate) const MAJOR_MAP: u8 = 5;
pub(crate) const MAJOR_TAG: u8 = 6;
pub(crate) const MAJOR_SIMPLE: u8 = 7;

// Largest possible length of a CBOR header (Major Type + Additional Information + Length)
pub(crate) const MAX_HEADER_LEN: usize = 9;

/// Default nesting depth used by [`EncoderStd`].
pub const DEFAULT_NESTING_DEPTH: usize = 4;

/// Maximum array/map/tag nesting depth [`Decoder`] will follow while
/// validating or traversing untrusted CBOR input, bounding stack usage
/// against maliciously deep input. Exceeding it returns
/// [`DecodeError::NestingTooDeep`].
pub(crate) const MAX_DECODE_DEPTH: usize = 64;
mod decoder;
#[cfg(feature = "edn")]
mod edn;
mod encoder;
#[cfg(feature = "standard_tags")]
mod tags;
#[cfg(feature = "edn")]
pub use edn::{EdnError, parse_edn};

/******************************************************************************
 * Error Types
 ******************************************************************************/

/// Errors that can occur during CBOR encoding.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum EncodeError {
    /// The provided buffer is not large enough to hold the encoded CBOR structure. The value is
    /// the exact number of bytes the complete output would have required.
    #[error("encode buffer too small: needed {0} bytes")]
    OutOfSpace(usize),
    /// The value you are trying to encode is invalid or not supported.
    #[error("invalid or unsupported value")]
    InvalidValue,
    /// You have attempted to nest arrays and/or maps too deeply. Increase the nesting depth.
    #[error("array/map nesting depth exceeded")]
    NestingTooDeep,
    /// You have attempted to put more than one CBOR item into a tag. CBOR doesn't allow this.
    #[error("CBOR tag must wrap exactly one item")]
    TagRequiresSingleItem,
    /// A duplicate map key was detected during deterministic encoding.
    #[error("duplicate map key")]
    DuplicateKey,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
/// Errors returned while decoding CBOR data.
pub enum DecodeError {
    /// End of buffer reached before fully decoding CBOR item.
    #[error("unexpected end of CBOR input")]
    UnexpectedEof,
    /// The Additional Info field contains an invalid or unexpected value.
    #[error("invalid additional info field")]
    InvalidAdditionalInfo,
    /// CBOR contains indefinite length constructions, which are not supported.
    #[error("indefinite-length encoding not supported")]
    IndefiniteLength,
    /// A CBOR tstr contains an invalid UTF8 encoding.
    #[error("text string contains invalid UTF-8")]
    InvalidUtf8,
    /// Unexpected trailing bytes in the decode buffer.
    #[error("unexpected trailing bytes")]
    TrailingBytes,
    /// Floating point values found and feature "float" is not enabled.
    #[error("float values require the 'float' feature")]
    FloatDisabled,
    /// Unexpected item type found during decode
    #[error("unexpected CBOR item type")]
    UnexpectedItem,
    /// Out of range for conversion
    #[error("value out of range for target type")]
    OutOfRange,
    /// Array/map/tag nesting exceeded the maximum depth the decoder will follow.
    #[error("array/map/tag nesting depth exceeded")]
    NestingTooDeep,
}

#[cfg(feature = "diagnostic")]
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
/// Errors returned while formatting CBOR diagnostic notation.
pub enum DiagnosticError {
    /// An error was found in the CBOR ([`DecodeError`] gives more info.
    #[error("CBOR decode error: {0}")]
    Decode(#[from] DecodeError),
    /// The output buffer provided is too small to hold the diagnostic output.
    #[error("output buffer too small for diagnostic notation")]
    OutOfSpace,
    /// Error in text formatting function, generally result of a core::fmt::FmtError.
    #[error("formatting error")]
    FmtError,
}

#[cfg(feature = "diagnostic")]
impl From<core::fmt::Error> for DiagnosticError {
    fn from(_: core::fmt::Error) -> Self {
        Self::FmtError
    }
}

/******************************************************************************
 * Exported CBOR Encoder API
 ******************************************************************************/

/// Convenience encoder with the default nesting depth.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::{EncoderStd, Encoder};
///
/// let mut buf = [0u8; 16];
/// let mut enc = EncoderStd::new(&mut buf);
/// enc.insert(1u8).unwrap();
/// assert_eq!(enc.encoded(), &[0x01]);
/// ```
pub struct EncoderStd<'a>(Encoder<'a, DEFAULT_NESTING_DEPTH>);

impl<'a> EncoderStd<'a> {
    /// Create an [`EncoderStd`] wrapping the provided buffer.
    #[must_use]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self(Encoder::<DEFAULT_NESTING_DEPTH>::new(buf))
    }

    /// Create an [`EncoderStd`] that reports the exact number of bytes an overflowing encode
    /// would have required. See [`Encoder::new_with_overflow`] for details.
    #[must_use]
    pub fn new_with_overflow(buf: &'a mut [u8], overflow_scratch: &'a mut [u8]) -> Self {
        Self(Encoder::<DEFAULT_NESTING_DEPTH>::new_with_overflow(
            buf,
            overflow_scratch,
        ))
    }

    /// Create an [`EncoderStd`] with deterministic map key ordering.
    ///
    /// The scratch buffer must not alias the output buffer and must be at least as
    /// large as the output buffer to handle worst-case maps. Returns
    /// [`EncodeError::OutOfSpace`] if the scratch buffer is smaller.
    #[cfg(feature = "deterministic_encoding")]
    #[must_use = "returns an error if the scratch buffer is too small"]
    pub fn new_deterministic(
        buf: &'a mut [u8],
        scratch: &'a mut [u8],
    ) -> Result<Self, EncodeError> {
        Ok(Self(Encoder::<DEFAULT_NESTING_DEPTH>::new_deterministic(
            buf, scratch,
        )?))
    }

    /// Unwrap to the underlying [`Encoder`].
    #[must_use]
    pub fn into_inner(self) -> Encoder<'a, DEFAULT_NESTING_DEPTH> {
        self.0
    }
}

impl<'a> Deref for EncoderStd<'a> {
    type Target = Encoder<'a, DEFAULT_NESTING_DEPTH>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> DerefMut for EncoderStd<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
