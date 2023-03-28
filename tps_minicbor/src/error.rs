/***************************************************************************************************
 * Copyright (c) 2021-2023 Qualcomm Innovation Center, Inc. All rights reserved.
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
 * rs_minicbor CBOR ErrorAPI
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
use std::result;

#[cfg(any(feature = "full", test))]
use thiserror::Error;

/// An alias for Result<T, CBORError> used throughout this crate.
pub type Result<T> = result::Result<T, CBORError>;

/// `CBORError` provides information about errors converting CBOR types to/from other types
//#[cfg_attr(all(not(feature = "full"), not(test)), derive(Copy, Clone, Debug))]
//#[cfg_attr(any(feature = "full", test), derive(Copy, Clone, Error, Debug))]
#[cfg_attr(any(feature="full", test), derive(Copy, Clone, Error, Debug))]
#[cfg_attr(all(not(feature="full"), not(test)), derive(Copy, Clone, Debug))]
pub enum CBORError {
    /// A number conversion has overflowed or underflowed.
    #[cfg_attr(any(feature="full", test), error("Overflow or underflow in number conversion"))]
    OutOfRange,
    /// Attempt to convert an item to an incompatible type.
    #[cfg_attr(any(feature="full", test), error("Attempt to convert an item of incompatible type"))]
    IncompatibleType,
    /// Slice length is incompatible with the target type conversion
    #[cfg_attr(any(feature="full", test), error("Slice length is incompatible with the target type conversion"))]
    BadSliceLength,
    /// Buffer too short to encode the next item
    #[cfg_attr(any(feature="full", test), error("Buffer insufficient to process the next item"))]
    EndOfBuffer,
    /// A tstr input contains an invalid UTF8 sequence
    #[cfg_attr(any(feature="full", test), error("A tstr contains an invalid UTF8 sequence"))]
    UTF8Error,
    /// The item was not expecting this Additional Information encoding. Probably malformed CBOR
    #[cfg_attr(any(feature="full", test), error("The item was not expecting this AI encoding. Probably malformed"))]
    AIError,
    /// Encoding is illegal or unsupported
    #[cfg_attr(any(feature="full", test), error("Encoding is illegal or unsupported"))]
    MalformedEncoding,
    /// The protocol feature is not supported
    #[cfg_attr(any(feature="full", test), error("The protocol feature is not supported"))]
    NotImplemented,
    /// No next item insertion possible as end of buffer reached.
    #[cfg_attr(any(feature="full", test), error("No next item possible as end of buffer - this is usually recoverable"))]
    NoMoreItems(usize),
    /// EOF marker was expected here.
    #[cfg_attr(any(feature="full", test), error("Expected EOF"))]
    EofExpected,
    /// The CBOR type indicated by the `str` was expected here.
    #[cfg_attr(any(feature="full", test), error("Did not match expected CBOR type"))]
    ExpectedType(&'static str),
    /// A predicate was not matched
    #[cfg_attr(any(feature="full", test), error("Failed predicate"))]
    FailedPredicate,
    /// The tag value was not expected here
    #[cfg_attr(any(feature="full", test), error("Unexpected Tag"))]
    ExpectedTag(u64),
    /// A CBOR map does not contain the requested key
    #[cfg_attr(any(feature="full", test), error("Map does not contain the requested key"))]
    KeyNotPresent,
    /// The requested array index was outside of the bounds of the encoded CBOR
    #[cfg_attr(any(feature="full", test), error("Array index out of bounds"))]
    IndexOutOfBounds,
    /// A Map contains a key, but no corresponding value was found. Malformed CBOR encoding.
    #[cfg_attr(any(feature="full", test), error("Map does not contain a value for the found key"))]
    ValueNotPresent,
    /// A range underflow was detected
    #[cfg_attr(any(feature="full", test), error("Range underflow"))]
    RangeUnderflow(usize),
    /// The provided value is not a legal Date/Time.
    #[cfg_attr(any(feature="full", test), error("Bad Date/Time value"))]
    BadDateTime,
    /// The type read is not allowed here.
    #[cfg_attr(any(feature="full", test), error("Type not allowed here"))]
    NotAllowed,
}
