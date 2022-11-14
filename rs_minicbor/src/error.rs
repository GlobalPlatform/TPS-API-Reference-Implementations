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

//#[cfg(any(feature = "full", test))]
use thiserror::Error;

pub type Result<T> = result::Result<T, CBORError>;

/// `CBORError` provides information about errors converting CBOR types to/from other types
//#[cfg_attr(all(not(feature = "full"), not(test)), derive(Copy, Clone, Debug))]
//#[cfg_attr(any(feature = "full", test), derive(Copy, Clone, Error, Debug))]
#[derive(Copy, Clone, Error, Debug)]
pub enum CBORError {
    #[error("Overflow or underflow in number conversion")]
    OutOfRange,
    #[error("Attempt to convert an item of incompatible type")]
    IncompatibleType,
    #[error("Slice length is incompatible with the target type conversion")]
    BadSliceLength,
    #[error("Buffer insufficient to process the next item")]
    EndOfBuffer,
    #[error("A tstr contains an invalid UTF8 sequence")]
    UTF8Error,
    #[error("The item was not expecting this AI encoding. Probably malformed")]
    AIError,
    #[error("Encoding is illegal or unsupported")]
    MalformedEncoding,
    #[error("The protocol feature is not supported")]
    NotImplemented,
    #[error("No next item possible as end of buffer - this is usually recoverable")]
    NoMoreItems(usize),
    #[error("Expected EOF")]
    EofExpected,
    #[error("Did not match expected CBOR type")]
    ExpectedType(&'static str),
    #[error("Failed predicate")]
    FailedPredicate,
    #[error("Unexpected Tag")]
    ExpectedTag(u64),
    #[error("Map does not contain the requested key")]
    KeyNotPresent,
    #[error("Map does not contain a value for the found key")]
    ValueNotPresent,
    #[error("Range underflow")]
    RangeUnderflow(usize),
    #[error("Bad Date/Time value")]
    BadDateTime,
    #[error("Type not allowed here")]
    NotAllowed,
}
