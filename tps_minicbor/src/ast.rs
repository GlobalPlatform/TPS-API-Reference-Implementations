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
use crate::error::CBORError;
use crate::map::MapBuf;
use crate::tag::TagBuf;

use std::convert::TryFrom;
use std::mem::transmute;

#[cfg(feature = "float")]
use half::f16;

#[cfg(any(feature = "full", test))]
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
#[cfg(any(feature = "full", test))]
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
    // The following are the full extensions
    DateTime(DateTime<FixedOffset>),
    Epoch(i64),
}

// Manual implementation needed as there is no Copy instance for BigInt
#[cfg(any(feature = "full", test))]
impl<'buf> Copy for CBOR<'buf> {}

#[derive(PartialEq, Debug, Copy, Clone)]
#[cfg(all(feature = "float", not(feature = "full"), not(test)))]
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

/***************************************************************************************************
 * Standard Trait Implementations: From value to CBOR. Always succeeds
 **************************************************************************************************/

/// Convert a bool into CBOR
impl<'buf> From<bool> for CBOR<'buf> {
    #[inline(always)]
    fn from(v: bool) -> Self {
        if v {
            Self::True
        } else {
            Self::False
        }
    }
}

/// Convert a u8 into CBOR
impl<'buf> From<u8> for CBOR<'buf> {
    #[inline(always)]
    fn from(v: u8) -> Self {
        Self::UInt(v as u64)
    }
}

/// Convert a u16 into CBOR
impl<'buf> From<u16> for CBOR<'buf> {
    #[inline(always)]
    fn from(v: u16) -> Self {
        Self::UInt(v as u64)
    }
}

/// Convert a u32 into CBOR
impl<'buf> From<u32> for CBOR<'buf> {
    #[inline(always)]
    fn from(v: u32) -> Self {
        Self::UInt(v as u64)
    }
}

/// Convert a u64 into CBOR
impl<'buf> From<u64> for CBOR<'buf> {
    #[inline(always)]
    fn from(v: u64) -> Self {
        Self::UInt(v)
    }
}

/// Convert an i8 into CBOR
impl<'buf> From<i8> for CBOR<'buf> {
    #[inline]
    fn from(v: i8) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }
}

/// Convert an i16 into CBOR
impl<'buf> From<i16> for CBOR<'buf> {
    #[inline]
    fn from(v: i16) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }
}

/// Convert an i32 into CBOR
impl<'buf> From<i32> for CBOR<'buf> {
    #[inline]
    fn from(v: i32) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }
}

/// Convert an i64 into CBOR
impl<'buf> From<i64> for CBOR<'buf> {
    #[inline]
    fn from(v: i64) -> Self {
        if v < 0 {
            Self::NInt((-1 - (v as i64)) as u64)
        } else {
            Self::UInt(v as u64)
        }
    }
}

/// Convert an &str into CBOR.
///
/// # Lifetime
///
/// The str reference *must* last at least as long as the CBOR item. If the
/// item is later encoded, it will be copied, but only at encode time.
impl<'buf> From<&'buf str> for CBOR<'buf> {
    #[inline]
    fn from(v: &'buf str) -> Self {
        Self::Tstr(v)
    }
}

/// Convert an &[u8] into CBOR.
///
/// # Lifetime
///
/// The str reference *must* last at least as long as the CBOR item. If the
/// item is later encoded, it will be copied, but only at encode time.
impl<'buf> From<&'buf [u8]> for CBOR<'buf> {
    #[inline]
    fn from(v: &'buf [u8]) -> Self {
        Self::Bstr(v)
    }
}

/***************************************************************************************************
 * Standard Trait Implementations: Try to convert CBOR into a value. Always fallible
 **************************************************************************************************/

/// Attempt to convert CBOR into bool
impl<'buf> TryFrom<CBOR<'buf>> for bool {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        if let CBOR::True = value {
            Ok(true)
        } else if let CBOR::False = value {
            Ok(false)
        } else {
            Err(CBORError::IncompatibleType)
        }
    }
}


/// Attempt to convert CBOR into u8
impl<'buf> TryFrom<CBOR<'buf>> for u8 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        if let CBOR::UInt(v) = value {
            if v <= u8::MAX as u64 {
                Ok(v as u8)
            } else {
                Err(CBORError::OutOfRange)
            }
        } else {
            Err(CBORError::IncompatibleType)
        }
    }
}

/// Attempt to convert CBOR into u16
impl<'buf> TryFrom<CBOR<'buf>> for u16 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        if let CBOR::UInt(v) = value {
            if v <= u16::MAX as u64 {
                Ok(v as u16)
            } else {
                Err(CBORError::OutOfRange)
            }
        } else {
            Err(CBORError::IncompatibleType)
        }
    }
}

/// Attempt to convert CBOR into u32
impl<'buf> TryFrom<CBOR<'buf>> for u32 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        if let CBOR::UInt(v) = value {
            if v <= u32::MAX as u64 {
                Ok(v as u32)
            } else {
                Err(CBORError::OutOfRange)
            }
        } else {
            Err(CBORError::IncompatibleType)
        }
    }
}

/// Attempt to convert CBOR into u64
impl<'buf> TryFrom<CBOR<'buf>> for u64 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        if let CBOR::UInt(v) = value {
            Ok(v)
        } else {
            Err(CBORError::IncompatibleType)
        }
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
impl<'buf> TryFrom<CBOR<'buf>> for i8 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        match value {
            // Positive integer.
            CBOR::UInt(val) => {
                if val <= i8::MAX as u64 {
                    Ok(val as i8)
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer.
            CBOR::NInt(val) => {
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
impl<'buf> TryFrom<CBOR<'buf>> for i16 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        match value {
            // Positive integer.
            CBOR::UInt(val) => {
                if val <= i16::MAX as u64 {
                    Ok(val as i16)
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer.
            CBOR::NInt(val) => {
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
}

/// Attempt to convert CBOR into i32
///
/// This will fail, for unsigned values, if n > i32::MAX
/// This will fail, for signed values, if n < i32::MIN
impl<'buf> TryFrom<CBOR<'buf>> for i32 {
    type Error = CBORError;

    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        match value {
            // Positive integer.
            CBOR::UInt(val) => {
                if val <= i32::MAX as u64 {
                    Ok(val as i32)
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer.
            CBOR::NInt(val) => {
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
/// (i8::MIN), it is represented as 1 - 127. Similar rules apply for all signed types.impl<'buf> TryFrom<CBOR<'buf>> for i64 {
impl<'buf> TryFrom<CBOR<'buf>> for i64 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        match value {
            // Positive integer.
            CBOR::UInt(val) => {
                if val & 1 << 63 == 0 {
                    // Good case. Transmute relies on internal representation of integers
                    Ok(unsafe { transmute::<u64, i64>(val) })
                } else {
                    // Overflow case
                    Err(CBORError::OutOfRange)
                }
            }
            // Negative integer. Add one to the stored uint
            CBOR::NInt(val) => {
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
}

/// Attempt to convert CBOR into i128
///
/// This will always succeed for integer values as CBOR only supports values over 64 bits
/// which all fit on 128 bits.
impl<'buf> TryFrom<CBOR<'buf>> for i128 {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR) -> core::result::Result<Self, Self::Error> {
        match value {
            // Positive integer.
            CBOR::UInt(v) => Ok(v as i128),
            // Negative integer. Add one to the stored uint
            CBOR::NInt(v) => Ok(-1 - (v as i128)),
            _ => Err(CBORError::IncompatibleType),
        }
    }
}

/// Attempt to convert a CBOR value into a &str
///
/// # Lifetime
///
/// The lifetime of the str will be the lifetime of the underlying buffer
/// on which the CBOR item is bounded.
impl<'buf> TryFrom<CBOR<'buf>> for &'buf str {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR<'buf>) -> core::result::Result<Self, Self::Error> {
        match value {
            CBOR::Tstr(s) => Ok(s),
            _ => Err(CBORError::IncompatibleType),
        }
    }
}

/// Attempt to convert a CBOR item into a &[u8]
///
/// # Lifetime
///
/// The lifetime of the &[u8] will be the lifetime of the underlying buffer
/// on which the CBOR item is bounded.
impl<'buf> TryFrom<CBOR<'buf>> for &'buf [u8] {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR<'buf>) -> core::result::Result<Self, Self::Error> {
        match value {
            CBOR::Bstr(bytes) => Ok(bytes),
            _ => Err(CBORError::IncompatibleType),
        }
    }
}

/// Attempt to convert a CBOR item into an ArrayBuf
impl<'buf> TryFrom<CBOR<'buf>> for ArrayBuf<'buf> {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR<'buf>) -> Result<Self, Self::Error> {
        match value {
            CBOR::Array(ab) => Ok(ab),
            _ => Err(CBORError::IncompatibleType)
        }
    }
}

/// Attempt to turn a CBOR item into a MapBuf
impl<'buf> TryFrom<CBOR<'buf>> for MapBuf<'buf> {
    type Error = CBORError;

    #[cfg_attr(feature = "trace", trace)]
    fn try_from(value: CBOR<'buf>) -> Result<Self, Self::Error> {
        match value {
            CBOR::Map(mb) => Ok(mb),
            _ => Err(CBORError::IncompatibleType)
        }
    }
}
