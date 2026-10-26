/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/***************************************************************************************************
 * Common Error Handling for TPS Crates
 **************************************************************************************************/
#![no_std]

//! Common error handling for TPS Client API crates.
//!
//! [`TPSError`] is the shared Rust-side error type used across the TPS Client API
//! implementation; [`from_c_error_code`] and `From<TPSError> for u32` convert to and
//! from the numerical error codes defined in `tps_client_common::c_errors`.

#[cfg(any(feature = "std", test))]
extern crate std;
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

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
    #[error("The requested operation could not complete within the allotted time.")]
    Timeout,
    #[error("The requested operation failed due to an error in the underlying platform.")]
    Platform,
    #[error("The requested operation failed due to an unexpected runtime error.")]
    RuntimeError,
}

/// Convert TPSError values into the corresponding numerical error code used over the C language
/// APIs.
///
/// > Note: the buffer size error information is lost in this conversion. Caller should manage this
/// > separately
impl From<TPSError> for u32 {
    fn from(err: TPSError) -> u32 {
        match err {
            TPSError::GenericError => ERROR_GENERIC,
            TPSError::AccessDenied => ERROR_ACCESS_DENIED,
            TPSError::Cancel => ERROR_CANCEL,
            TPSError::BadFormat => ERROR_BAD_FORMAT,
            TPSError::NotImplemented => ERROR_NOT_IMPLEMENTED,
            TPSError::NotSupported => ERROR_NOT_SUPPORTED,
            TPSError::NoData => ERROR_NO_DATA,
            TPSError::OutOfMemory => ERROR_OUT_OF_MEMORY,
            TPSError::Busy => ERROR_BUSY,
            TPSError::CommunicationError => ERROR_COMMUNICATION,
            TPSError::SecurityError => ERROR_SECURITY,
            TPSError::ShortBuffer(_) => ERROR_SHORT_BUFFER,
            TPSError::Deprecated => ERROR_DEPRECATED,
            TPSError::BadIdentifier => ERROR_BAD_IDENTIFIER,
            TPSError::NullPointer => ERROR_NULL_POINTER,
            TPSError::BadState => ERROR_BAD_STATE,
            TPSError::Timeout => ERROR_TIMEOUT,
            TPSError::Platform => ERROR_PLATFORM,
            TPSError::RuntimeError => ERROR_RUNTIME_ERROR,
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
///
/// Spec §3.3.4 mandates that an implementation supply a buffer-size value alongside
/// `ERROR_SHORT_BUFFER`. If `item` is `ERROR_SHORT_BUFFER` and `buf_size` is `None`, the caller
/// is non-compliant with that requirement, so this returns [`TPSError::RuntimeError`] rather
/// than [`TPSError::ShortBuffer`].
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
                // Spec §3.3.4 SHALL-mandates that an implementation provide a
                // buffer-size value alongside `ERROR_SHORT_BUFFER`; a caller
                // omitting it is non-compliant, so this is a fatal error
                // rather than a recoverable one (Stage 1 Finding 4 ruling).
                Err(TPSError::RuntimeError)
            }
        }
        ERROR_DEPRECATED => Err(TPSError::Deprecated),
        ERROR_BAD_IDENTIFIER => Err(TPSError::BadIdentifier),
        ERROR_NULL_POINTER => Err(TPSError::NullPointer),
        ERROR_BAD_STATE => Err(TPSError::BadState),
        ERROR_TIMEOUT => Err(TPSError::Timeout),
        ERROR_PLATFORM => Err(TPSError::Platform),
        ERROR_RUNTIME_ERROR => Err(TPSError::RuntimeError),
        _ => Err(TPSError::GenericError),
    }
}

/***************************************************************************************************
 * Regression tests: pin every error code's exact numeric value against the normative
 * Table 4-2 "API Return Code Constants" of the GPP TPS Client API Spec v1.0
 * (specifications/spec.txt lines ~1846-1898). Per E2 in TPS_CLIENT_API_PLAN.md, Table 4-2 (not
 * the informative Annex A.4.1, which has a typo: 0xF0000004 instead of 0xF0090004 for
 * TPSC_ERROR_NOT_IMPLEMENTED) is the ruling source of truth.
 *
 * Table 4-2 values (for reference):
 *   TPSC_SUCCESS               0x00000000
 *   TPSC_ERROR_GENERIC         0xF0090000
 *   TPSC_ERROR_ACCESS_DENIED   0xF0090001
 *   TPSC_ERROR_CANCEL          0xF0090002
 *   TPSC_ERROR_BAD_FORMAT      0xF0090003
 *   TPSC_ERROR_NOT_IMPLEMENTED 0xF0090004
 *   TPSC_ERROR_NOT_SUPPORTED   0xF0090005
 *   TPSC_ERROR_NO_DATA         0xF0090006
 *   TPSC_ERROR_OUT_OF_MEMORY   0xF0090007
 *   TPSC_ERROR_BUSY            0xF0090008
 *   TPSC_ERROR_COMMUNICATION   0xF0090009
 *   TPSC_ERROR_SECURITY        0xF009000A
 *   TPSC_ERROR_SHORT_BUFFER    0xF009000B
 *   TPSC_ERROR_DEPRECATED      0xF009000C
 *   TPSC_ERROR_BAD_IDENTIFIER  0xF009000D
 *   TPSC_ERROR_NULL_POINTER    0xF009000E
 *   TPSC_ERROR_BAD_STATE       0xF009000F
 *   TPSC_ERROR_TIMEOUT         0xF0090010
 *   TPSC_ERROR_PLATFORM        0xF0090011
 *   TPSC_ERROR_RUNTIME_ERROR   0xF0090012
 **************************************************************************************************/
#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the exact numeric value that `tps_client_common::c_errors` exposes to this crate,
    /// against the normative Table 4-2 value from the spec. This test protects against a future
    /// accidental edit to `tps_client_common::c_errors` (e.g. a transposed hex digit) going
    /// unnoticed, since `tps_error` only re-exports/consumes these constants and would otherwise
    /// have no test coverage of the raw values it relies on.
    #[test]
    fn c_error_constants_match_spec_table_4_2() {
        assert_eq!(SUCCESS, 0x0000_0000);
        assert_eq!(ERROR_GENERIC, 0xF009_0000);
        assert_eq!(ERROR_ACCESS_DENIED, 0xF009_0001);
        assert_eq!(ERROR_CANCEL, 0xF009_0002);
        assert_eq!(ERROR_BAD_FORMAT, 0xF009_0003);
        // E2 ruling: Table 4-2 (normative) is 0xF0090004, NOT Annex A.4.1's 0xF0000004.
        assert_eq!(ERROR_NOT_IMPLEMENTED, 0xF009_0004);
        assert_eq!(ERROR_NOT_SUPPORTED, 0xF009_0005);
        assert_eq!(ERROR_NO_DATA, 0xF009_0006);
        assert_eq!(ERROR_OUT_OF_MEMORY, 0xF009_0007);
        assert_eq!(ERROR_BUSY, 0xF009_0008);
        assert_eq!(ERROR_COMMUNICATION, 0xF009_0009);
        assert_eq!(ERROR_SECURITY, 0xF009_000A);
        assert_eq!(ERROR_SHORT_BUFFER, 0xF009_000B);
        assert_eq!(ERROR_DEPRECATED, 0xF009_000C);
        assert_eq!(ERROR_BAD_IDENTIFIER, 0xF009_000D);
        assert_eq!(ERROR_NULL_POINTER, 0xF009_000E);
        assert_eq!(ERROR_BAD_STATE, 0xF009_000F);
        assert_eq!(ERROR_TIMEOUT, 0xF009_0010);
        assert_eq!(ERROR_PLATFORM, 0xF009_0011);
        assert_eq!(ERROR_RUNTIME_ERROR, 0xF009_0012);
    }

    /// Exhaustive pin of `Into<u32> for TPSError`: every variant must convert to exactly the
    /// Table 4-2 value for the corresponding `TPSC_ERROR_*` constant. A `match` with no wildcard
    /// arm is used so that adding a new `TPSError` variant without updating this test causes a
    /// compile error here, not a silent gap in coverage.
    #[test]
    fn tps_error_into_u32_matches_spec_values() {
        let cases: &[(TPSError, u32)] = &[
            (TPSError::GenericError, 0xF009_0000),
            (TPSError::AccessDenied, 0xF009_0001),
            (TPSError::Cancel, 0xF009_0002),
            (TPSError::BadFormat, 0xF009_0003),
            (TPSError::NotImplemented, 0xF009_0004),
            (TPSError::NotSupported, 0xF009_0005),
            (TPSError::NoData, 0xF009_0006),
            (TPSError::OutOfMemory, 0xF009_0007),
            (TPSError::Busy, 0xF009_0008),
            (TPSError::CommunicationError, 0xF009_0009),
            (TPSError::SecurityError, 0xF009_000A),
            (TPSError::ShortBuffer(1234), 0xF009_000B),
            (TPSError::Deprecated, 0xF009_000C),
            (TPSError::BadIdentifier, 0xF009_000D),
            (TPSError::NullPointer, 0xF009_000E),
            (TPSError::BadState, 0xF009_000F),
            (TPSError::Timeout, 0xF009_0010),
            (TPSError::Platform, 0xF009_0011),
            (TPSError::RuntimeError, 0xF009_0012),
        ];
        for (variant, expected) in cases {
            // Re-derive expected via exhaustive match too, so a newly-added variant that isn't
            // added to `cases` above still gets checked against `Into<u32>`'s own exhaustive
            // match (which the compiler forces to be complete).
            let actual: u32 = match variant {
                TPSError::GenericError => TPSError::GenericError.into(),
                TPSError::AccessDenied => TPSError::AccessDenied.into(),
                TPSError::Cancel => TPSError::Cancel.into(),
                TPSError::BadFormat => TPSError::BadFormat.into(),
                TPSError::NotImplemented => TPSError::NotImplemented.into(),
                TPSError::NotSupported => TPSError::NotSupported.into(),
                TPSError::NoData => TPSError::NoData.into(),
                TPSError::OutOfMemory => TPSError::OutOfMemory.into(),
                TPSError::Busy => TPSError::Busy.into(),
                TPSError::CommunicationError => TPSError::CommunicationError.into(),
                TPSError::SecurityError => TPSError::SecurityError.into(),
                TPSError::ShortBuffer(n) => TPSError::ShortBuffer(*n).into(),
                TPSError::Deprecated => TPSError::Deprecated.into(),
                TPSError::BadIdentifier => TPSError::BadIdentifier.into(),
                TPSError::NullPointer => TPSError::NullPointer.into(),
                TPSError::BadState => TPSError::BadState.into(),
                TPSError::Timeout => TPSError::Timeout.into(),
                TPSError::Platform => TPSError::Platform.into(),
                TPSError::RuntimeError => TPSError::RuntimeError.into(),
            };
            assert_eq!(actual, *expected);
        }
    }

    /// `ShortBuffer`'s payload (the exact byte count required, per spec §3.3.4 / R12 / R16) must
    /// never leak into the numeric code crossing the C ABI: `Into<u32>` always yields
    /// `ERROR_SHORT_BUFFER` regardless of the carried value, since the byte count is reported via
    /// a separate out-parameter (e.g. `recv_len`/`num_services`) on the C side, not encoded in the
    /// `TPSC_Result` itself.
    #[test]
    fn short_buffer_payload_does_not_affect_wire_value() {
        let a: u32 = TPSError::ShortBuffer(0).into();
        let b: u32 = TPSError::ShortBuffer(usize::MAX).into();
        assert_eq!(a, ERROR_SHORT_BUFFER);
        assert_eq!(b, ERROR_SHORT_BUFFER);
    }

    /// Exhaustive round-trip pin for `from_c_error_code`: every `TPSC_ERROR_*` constant from
    /// Table 4-2 must map back to the correct `TPSError` variant (and `TPSC_SUCCESS` to `Ok(())`).
    #[test]
    fn from_c_error_code_round_trips_every_spec_code() {
        assert!(from_c_error_code(SUCCESS, None).is_ok());

        type VariantCheck = fn(TPSError) -> bool;
        let cases: &[(u32, VariantCheck)] = &[
            (ERROR_ACCESS_DENIED, |e| matches!(e, TPSError::AccessDenied)),
            (ERROR_CANCEL, |e| matches!(e, TPSError::Cancel)),
            (ERROR_BAD_FORMAT, |e| matches!(e, TPSError::BadFormat)),
            (ERROR_NOT_IMPLEMENTED, |e| matches!(e, TPSError::NotImplemented)),
            (ERROR_NOT_SUPPORTED, |e| matches!(e, TPSError::NotSupported)),
            (ERROR_NO_DATA, |e| matches!(e, TPSError::NoData)),
            (ERROR_OUT_OF_MEMORY, |e| matches!(e, TPSError::OutOfMemory)),
            (ERROR_BUSY, |e| matches!(e, TPSError::Busy)),
            (ERROR_COMMUNICATION, |e| matches!(e, TPSError::CommunicationError)),
            (ERROR_SECURITY, |e| matches!(e, TPSError::SecurityError)),
            (ERROR_DEPRECATED, |e| matches!(e, TPSError::Deprecated)),
            (ERROR_BAD_IDENTIFIER, |e| matches!(e, TPSError::BadIdentifier)),
            (ERROR_NULL_POINTER, |e| matches!(e, TPSError::NullPointer)),
            (ERROR_BAD_STATE, |e| matches!(e, TPSError::BadState)),
            (ERROR_TIMEOUT, |e| matches!(e, TPSError::Timeout)),
            (ERROR_PLATFORM, |e| matches!(e, TPSError::Platform)),
            (ERROR_RUNTIME_ERROR, |e| matches!(e, TPSError::RuntimeError)),
        ];
        for (code, check) in cases {
            let err = from_c_error_code(*code, None).expect_err("expected an error variant");
            assert!(check(err), "wrong TPSError variant for code {:#010X}", code);
        }
    }

    /// `ERROR_SHORT_BUFFER` is special-cased: with `Some(len)` it must carry the exact byte count
    /// through into `TPSError::ShortBuffer(len)` (spec §3.3.4's exact-size requirement, R12/R16).
    #[test]
    fn from_c_error_code_short_buffer_with_size_carries_exact_len() {
        match from_c_error_code(ERROR_SHORT_BUFFER, Some(42)) {
            Err(TPSError::ShortBuffer(42)) => {}
            other => panic!("expected ShortBuffer(42), got {:?}", other),
        }
    }

    /// Spec §3.3.4 SHALL-mandates that `ERROR_SHORT_BUFFER` be accompanied by a
    /// buffer-size value; a caller omitting it is non-compliant, so
    /// `from_c_error_code` treats this as a fatal `RuntimeError` rather than a
    /// recoverable state (Stage 1 Finding 4 — human ruling, fixed).
    #[test]
    fn from_c_error_code_short_buffer_without_size_is_a_runtime_error() {
        assert!(matches!(
            from_c_error_code(ERROR_SHORT_BUFFER, None),
            Err(TPSError::RuntimeError)
        ));
    }

    /// Values outside Table 4-2's assigned range (including the `Implementation-Defined`
    /// 0xF0000001-0xF000FFFE band and fully unassigned values) must not panic and currently map to
    /// `TPSError::GenericError` via the catch-all arm. Confirmed correct per Stage 1 Finding 4's
    /// human ruling: an informative spec note will state that an unrecognized
    /// implementation-defined/future-use code SHOULD be treated as `TPSC_ERROR_GENERIC`.
    #[test]
    fn from_c_error_code_unknown_value_maps_to_generic_error() {
        assert!(matches!(
            from_c_error_code(0xF000_0001, None),
            Err(TPSError::GenericError)
        ));
        assert!(matches!(
            from_c_error_code(0xDEAD_BEEF, None),
            Err(TPSError::GenericError)
        ));
    }
}
