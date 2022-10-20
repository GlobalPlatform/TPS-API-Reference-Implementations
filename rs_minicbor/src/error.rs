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
 * rs_minicbor CBOR ErrorAPI
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
use std::result;

#[cfg(any(feature = "std", test))]
use thiserror::Error;

pub type Result<T> = result::Result<T, CBORError>;

/// `CBORError` provides information about errors converting CBOR types to/from other types
#[cfg_attr(any(feature = "std", test), derive(Copy, Clone, Error, Debug))]
#[cfg_attr(all(not(feature = "std"), not(test)), derive(Copy, Clone, Debug))]
pub enum CBORError {
    #[cfg_attr(
        any(feature = "std", test),
        error("Overflow or underflow in number conversion")
    )]
    OutOfRange,
    #[cfg_attr(
        any(feature = "std", test),
        error("Attempt to convert an item of incompatible type")
    )]
    IncompatibleType,
    #[cfg_attr(
        any(feature = "std", test),
        error("Slice length is incompatible with the target type conversion")
    )]
    BadSliceLength,
    #[cfg_attr(
        any(feature = "std", test),
        error("Buffer insufficient to process the next item")
    )]
    EndOfBuffer,
    #[cfg_attr(
        any(feature = "std", test),
        error("A tstr contains an invalid UTF8 sequence")
    )]
    UTF8Error,
    #[cfg_attr(
        any(feature = "std", test),
        error("The item was not expecting this AI encoding. Probably malformed")
    )]
    AIError,
    #[cfg_attr(
        any(feature = "std", test),
        error("Encoding is illegal or unsupported")
    )]
    MalformedEncoding,
    #[cfg_attr(
        any(feature = "std", test),
        error("The protocol feature is not supported")
    )]
    NotImplemented,
    #[cfg_attr(
        any(feature = "std", test),
        error("No next item possible as end of buffer - this is usually recoverable")
    )]
    NoMoreItems(usize),
    #[cfg_attr(any(feature = "std", test), error("Expected EOF"))]
    EofExpected,
    #[cfg_attr(any(feature = "std", test), error("Did not match expected CBOR type"))]
    ExpectedType(&'static str),
    #[cfg_attr(any(feature = "std", test), error("Failed predicate"))]
    FailedPredicate,
    #[cfg_attr(any(feature = "std", test), error("Unexpected Tag"))]
    ExpectedTag(u64),
    #[cfg_attr(
        any(feature = "std", test),
        error("Map does not contain the requested key")
    )]
    KeyNotPresent,
    #[cfg_attr(
        any(feature = "std", test),
        error("Map does not contain a value for the found key")
    )]
    ValueNotPresent,
    #[cfg_attr(any(feature = "std", test), error("Range underflow"))]
    RangeUnderflow(usize),
    #[cfg_attr(any(feature = "std", test), error("Bad Date/Time value"))]
    BadDateTime,
    #[cfg_attr(any(feature = "std", test), error("Type not allowed here"))]
    NotAllowed,
}
