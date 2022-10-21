/***************************************************************************************************
 * Copyright (c) 2020-2022 Qualcomm Innovation Center, Inc. All rights reserved.
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
 * CBOR Abstract Syntax Tree
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
use crate::array::ArrayBuf;
use crate::error::{CBORError, Result};
use crate::map::MapBuf;
use crate::tag::TagBuf;

use std::convert::TryFrom;
use std::mem::transmute;

#[cfg(feature = "float")]
use half::f16;

#[cfg(any(feature = "std_tags", test))]
use chrono::{DateTime, FixedOffset};

#[cfg(feature = "trace")]
use func_trace::trace;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/// The data type for CBOR Items. CBOR types may borrow immutably from an underlying buffer which
/// must therefore outlive the item itself - this is the 'buf lifetime.
///
/// CBOR item representations are as follows:
///
/// - Positive and negative integers are stored as a u64 with enum tags used to distinguish
///   positive (UInt) and negative (NInt) numbers
/// - The bstr and tstr types are held as immutable borrowed slices over the CBOR parse buffer
/// - Simple types are stored as a u8
/// - Arrays are stored as a number of items and an immutable borrowed slice over the contents of
///   the array
/// - Maps are stored as a number of pairs and an immutable borrowed slice over the contents of the
///   map
#[derive(PartialEq, Debug, Clone)]
#[cfg(any(feature = "std_tags", test))]
pub enum CBOR<'buf> {
    UInt(u64),
    NInt(u64),
    Float64(f64),
    Float32(f32),
    Float16(f16),
    Bstr(&'buf [u8]),
    Tstr(&'buf str),
    Array(ArrayBuf<'buf>),
    Map(MapBuf<'buf>),
    Tag(TagBuf<'buf>),
    Simple(u8),
    False,
    True,
    Null,
    Undefined,
    Eof,
    // The following are the std_tags extensions
    DateTime(DateTime<FixedOffset>),
    Epoch(i64),
}

// Manual implementation needed as there is no Copy instance for BigInt
#[cfg(any(feature = "std_tags", test))]
impl<'buf> Copy for CBOR<'buf> {}

#[derive(PartialEq, Debug, Copy, Clone)]
#[cfg(all(feature = "float", not(feature = "std_tags"), not(test)))]
pub enum CBOR<'buf> {
    UInt(u64),
    NInt(u64),
    Float64(f64),
    Float32(f32),
    Float16(f16),
    Bstr(&'buf [u8]),
    Tstr(&'buf str),
    Array(ArrayBuf<'buf>),
    Map(MapBuf<'buf>),
    Tag(TagBuf<'buf>),
    Simple(u8),
    False,
    True,
    Null,
    Undefined,
    Eof,
}

// This variant used when Floating point operations are not included
#[derive(PartialEq, Debug, Copy, Clone)]
#[cfg(all(not(feature = "float"), not(test)))]
pub enum CBOR<'buf> {
    UInt(u64),
    NInt(u64),
    Bstr(&'buf [u8]),
    Tstr(&'buf str),
    Array(ArrayBuf<'buf>),
    Map(MapBuf<'buf>),
    Tag(TagBuf<'buf>),
    Simple(u8),
    False,
    True,
    Null,
    Undefined,
    Eof,
}

impl<'buf> CBOR<'buf> {
    /// Attempt to convert CBOR into u8
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_u8(self) -> Result<u8> {
        if let Self::UInt(v) = self {
            if v <= u8::MAX as u64 {
                Ok(v as u8)
            } else {
                Err(CBORError::OutOfRange)
            }
        } else {
            Err(CBORError::IncompatibleType)
        }
    }

    /// Attempt to convert CBOR into u16
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_u16(self) -> Result<u16> {
        if let Self::UInt(v) = self {
            if v <= u16::MAX as u64 {
                Ok(v as u16)
            } else {
                Err(CBORError::OutOfRange)
            }
        } else {
            Err(CBORError::IncompatibleType)
        }
    }

    /// Attempt to convert CBOR into u32
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_u32(self) -> Result<u32> {
        if let Self::UInt(v) = self {
            if v <= u32::MAX as u64 {
                Ok(v as u32)
            } else {
                Err(CBORError::OutOfRange)
            }
        } else {
            Err(CBORError::IncompatibleType)
        }
    }

    /// Attempt to convert CBOR into u64
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_u64(self) -> Result<u64> {
        if let Self::UInt(v) = self {
            Ok(v)
        } else {
            Err(CBORError::IncompatibleType)
        }
    }

    /// Attempt to convert CBOR into u64
    ///
    /// This will always succeed for integer values as CBOR only supports values over 64 bits
    /// which all fit on 128 bits.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_i128(self) -> Result<i128> {
        match self {
            // Positive integer.
            Self::UInt(v) => Ok(v as i128),
            // Negative integer. Add one to the stored uint
            Self::NInt(v) => Ok(-1 - (v as i128)),
            _ => Err(CBORError::IncompatibleType),
        }
    }

    /// Attempt to convert CBOR into i64
    ///
    /// This will fail, for unsigned values, if n > i64::MAX
    /// This will fail, for signed values, if n < i64::MIN
    ///
    /// For positive values it is sufficient to check the MSB is not set (MSB used for 2's
    /// complement sign)
    ///
    /// For negative values it is also sufficient to check that the MSB is not set. This is because
    /// it gives us a minimum value of -1 - (2^(n-1) - 1), for example, if we have the value -128
    /// (i8::MIN), it is represented as 1 - 127. Similar rules apply for all signed types.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_i64(self) -> Result<i64> {
        match self {
            // Positive integer.
            Self::UInt(val) => {
                if val & 1 << 63 == 0 {
                    // Good case. Transmute relies on internal representation of integers
                    Ok(unsafe { transmute::<u64, i64>(val) })
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer. Add one to the stored uint
            Self::NInt(val) => {
                if val & 1 << 63 == 0 {
                    // 2's complement (only the complement required as store -1 -n
                    let v = !val;
                    Ok(unsafe { transmute::<u64, i64>(v) })
                } else {
                    // Overflow
                    Err(CBORError::OutOfRange)
                }
            }
            _ => Err(CBORError::IncompatibleType),
        }
    }

    /// Attempt to convert CBOR into i32
    ///
    /// This will fail, for unsigned values, if n > i32::MAX
    /// This will fail, for signed values, if n < i32::MIN
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_i32(self) -> Result<i32> {
        match self {
            // Positive integer.
            Self::UInt(val) => {
                if val <= i32::MAX as u64 {
                    Ok(val as i32)
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer.
            Self::NInt(val) => {
                // Unsigned value is checked against i32::MAX as encoding 1 - val
                if val <= i32::MAX as u64 {
                    Ok(-1 - (val as i32))
                } else {
                    Err(CBORError::OutOfRange)
                }
            }
            _ => Err(CBORError::IncompatibleType),
        }
    }

    /// Attempt to convert CBOR into i16
    /// This will fail, for unsigned values, if n > i16::MAX
    /// This will fail, for signed values, if n < i16::MIN
    ///
    /// For positive values it is sufficient to check the MSB is not set (MSB used for 2's
    /// complement sign)
    ///
    /// For negative values it is also sufficient to check that the MSB is not set. This is because
    /// it gives us a minimum value of -1 - (2^(n-1) - 1), for example, if we have the value -128
    /// (i8::MIN), it is represented as 1 - 127. Similar rules apply for all signed types.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_i16(self) -> Result<i16> {
        match self {
            // Positive integer.
            Self::UInt(val) => {
                if val <= i16::MAX as u64 {
                    Ok(val as i16)
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer.
            Self::NInt(val) => {
                // Unsigned value is checked against i32::MAX as encoding 1 - val
                if val <= i16::MAX as u64 {
                    Ok(-1 - (val as i16))
                } else {
                    Err(CBORError::OutOfRange)
                }
            }
            _ => Err(CBORError::IncompatibleType),
        }
    }

    /// Attempt to convert CBOR into i8
    ///
    /// This will fail, for unsigned values, if n > i8::MAX
    /// This will fail, for signed values, if n < i8::MIN
    ///
    /// For positive values it is sufficient to check the MSB is not set (MSB used for 2's
    /// complement sign)
    ///
    /// For negative values it is also sufficient to check that the MSB is not set. This is because
    /// it gives us a minimum value of -1 - (2^(n-1) - 1), for example, if we have the value -128
    /// (i8::MIN), it is represented as 1 - 127. Similar rules apply for all signed types.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_i8(self) -> Result<i8> {
        match self {
            // Positive integer.
            Self::UInt(val) => {
                if val <= i8::MAX as u64 {
                    Ok(val as i8)
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer.
            Self::NInt(val) => {
                // Unsigned value is checked against i32::MAX as encoding 1 - val
                if val <= i8::MAX as u64 {
                    Ok(-1 - (val as i8))
                } else {
                    Err(CBORError::OutOfRange)
                }
            }
            _ => Err(CBORError::IncompatibleType),
        }
    }

    /// Read a str slice.
    #[inline]
    pub fn try_into_str(&self) -> Result<&'buf str> {
        match self {
            Self::Tstr(s) => Ok(s),
            _ => Err(CBORError::IncompatibleType),
        }
    }

    /// Read a [u8] slice.
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn try_into_u8slice(&self) -> Result<&'buf [u8]> {
        match self {
            Self::Bstr(bytes) => Ok(*bytes),
            _ => Err(CBORError::IncompatibleType),
        }
    }

    /// Turn `u8` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_u8(v: u8) -> Self {
        Self::UInt(v as u64)
    }

    /// Turn `u16` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_u16(v: u16) -> Self {
        Self::UInt(v as u64)
    }

    /// Turn `u32` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_u32(v: u32) -> Self {
        Self::UInt(v as u64)
    }

    /// Turn `u64` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_u64(v: u64) -> Self {
        Self::UInt(v)
    }

    /// Turn `i8` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_i8(v: i8) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }

    /// Turn `i16` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_i16(v: i16) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }

    /// Turn `i32` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_i32(v: i32) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }

    /// Turn `i64` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_i64(v: i64) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }

    /// Turn `&str` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_str(v: &'buf str) -> Self {
        Self::Tstr(v)
    }

    /// Turn `&[u8]` into `CBOR`
    #[cfg_attr(feature = "trace", trace)]
    #[inline]
    pub fn from_u8slice(v: &'buf [u8]) -> Self {
        Self::Bstr(v)
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for u8 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_u8()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for u16 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_u16()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for u32 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_u32()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for u64 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_u64()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for i8 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_i8()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for i16 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_i16()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for i32 {
    type Error = CBORError;

    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_i32()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for i64 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_i64()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for i128 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_i128()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for &'buf str {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_str()
    }
}

impl<'buf> TryFrom<&'buf CBOR<'buf>> for &'buf [u8] {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: &'buf CBOR) -> core::result::Result<Self, Self::Error> {
        (*value).try_into_u8slice()
    }
}
