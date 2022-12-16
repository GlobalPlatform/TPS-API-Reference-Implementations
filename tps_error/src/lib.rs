/***************************************************************************************************
 * Copyright (c) 2022 Jeremy O'Donoghue. All rights reserved.
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
 * Common Error Handling for TPS Crates
 **************************************************************************************************/
extern crate tps_client_common;

use thiserror::Error;
use tps_client_common::c_errors::*;

/// Set of errors used in all TPS-related APIs.
///
/// Each error has a corresponding error constant in the `tps_client_common` crate. The error
/// descriptions should be pretty self-explanatory.
#[derive(Error, Debug)]
pub enum TPSError {
    #[error("Generic error - unspecified issue found")]
    GenericError,
    #[error("Error due to insufficient access privileges.")]
    AccessDenied,
    #[error("The operation was cancelled.")]
    Cancel,
    #[error("Error due to incorrectly formatted input data.")]
    BadFormat,
    #[error("The requested operation is specified and should exist, but is not yet implemented.")]
    NotImplemented,
    #[error("The requested operation is valid, but not supported by this implementation.")]
    NotSupported,
    #[error("The requested operation failed because expected data was missing.")]
    NoData,
    #[error("The requested operation failed because the system ran out of memory resources.")]
    OutOfMemory,
    #[error("The requested operation failed because the system was busy.")]
    Busy,
    #[error("The requested operation failed due to a communication error with the service implementation.")]
    CommunicationError,
    #[error(
        "A security fault was detected. The integrity of the returned value cannot be guaranteed."
    )]
    SecurityError,
    #[error(
        "The supplied buffer is too small to contain the requested data. Minimum size returned"
    )]
    ShortBuffer(usize),
    #[error("The called API is deprecated. Caller can assume that the returned result is valid and correct.")]
    Deprecated,
    #[error("The supplied UUID is not recognised for the requested usage.")]
    BadIdentifier,
    #[error("A NULL pointer was passed by the caller where a valid pointer is required.")]
    NullPointer,
    #[error("API was called in the wrong state.")]
    BadState,
}

/// Convert TPSError values into the corresponding numerical error code used over the C language
/// APIs.
///
/// > Note: the buffer size error information is lost in this conversion. Caller should manage this
/// > separately
impl Into<u32> for TPSError {
    fn into(self) -> u32 {
        match self {
            Self::GenericError => ERROR_GENERIC,
            Self::AccessDenied => ERROR_ACCESS_DENIED,
            Self::Cancel => ERROR_CANCEL,
            Self::BadFormat => ERROR_BAD_FORMAT,
            Self::NotImplemented => ERROR_NOT_IMPLEMENTED,
            Self::NotSupported => ERROR_NOT_SUPPORTED,
            Self::NoData => ERROR_NO_DATA,
            Self::OutOfMemory => ERROR_OUT_OF_MEMORY,
            Self::Busy => ERROR_BUSY,
            Self::CommunicationError => ERROR_COMMUNICATION,
            Self::SecurityError => ERROR_SECURITY,
            Self::ShortBuffer(_) => ERROR_SHORT_BUFFER,
            Self::Deprecated => ERROR_DEPRECATED,
            Self::BadIdentifier => ERROR_BAD_IDENTIFIER,
            Self::NullPointer => ERROR_NULL_POINTER,
            Self::BadState => ERROR_BAD_STATE,
        }
    }
}

/// Convert from one of the C language error codes in `tps_client_common` crate into a TPSError.
///
/// While it would have been nice to make this an instance of `From` or `TryFrom`, there are a
/// couple of characteristics that are important and led away from those approaches. I wanted
/// conversion to handle `SUCCESS` case and also to handle `SHORT_BUFFER`, and these requirements
/// led to:
///   
/// - Wish to return a Result (to cover success nicely)
/// - Wish to ensure that SHORT_BUFFER always contains a length
pub fn from_c_error_code(item: u32, buf_size: Option<usize>) -> Result<(), TPSError> {
    match item {
        SUCCESS => Ok(()),
        ERROR_ACCESS_DENIED => Err(TPSError::AccessDenied),
        ERROR_CANCEL => Err(TPSError::Cancel),
        ERROR_BAD_FORMAT => Err(TPSError::BadFormat),
        ERROR_NOT_IMPLEMENTED => Err(TPSError::NotImplemented),
        ERROR_NOT_SUPPORTED => Err(TPSError::NotSupported),
        ERROR_NO_DATA => Err(TPSError::NoData),
        ERROR_OUT_OF_MEMORY => Err(TPSError::OutOfMemory),
        ERROR_BUSY => Err(TPSError::Busy),
        ERROR_COMMUNICATION => Err(TPSError::CommunicationError),
        ERROR_SECURITY => Err(TPSError::SecurityError),
        ERROR_SHORT_BUFFER => {
            if let Some(buf_len) = buf_size {
                Err(TPSError::ShortBuffer(buf_len))
            } else {
                Err(TPSError::BadState)
            }
        }
        ERROR_DEPRECATED => Err(TPSError::Deprecated),
        ERROR_BAD_IDENTIFIER => Err(TPSError::BadIdentifier),
        ERROR_NULL_POINTER => Err(TPSError::NullPointer),
        ERROR_BAD_STATE => Err(TPSError::BadState),
        _ => Err(TPSError::GenericError),
    }
}
