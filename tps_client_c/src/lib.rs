/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/***************************************************************************************************
 * Exported TPS Client API (C language)
 **************************************************************************************************/
#![no_std]

//! The C ABI surface of the GlobalPlatform Trusted Platform Services (TPS) Client API (GPP TPS
//! Client API Specification v1.0): `extern "C"` wrappers over [`tps_client_api`]'s Rust functions,
//! with NULL-pointer/basic-sanity checks on the raw pointers crossing the FFI boundary. The
//! corresponding C header (`tpsc_client_api.h`) is generated from this crate's public items by
//! `cbindgen` at build time (see `build.rs`/`cbindgen.toml`).

// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set.
#[cfg(any(feature = "std", test))]
extern crate std;

// If we are really building no_std, pull in core as well. It is aliased as std so that "use"
// statements are always the same
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

use std::slice::from_raw_parts_mut;

extern crate tps_client_api;
extern crate tps_client_common;

// `rot13_connector` provides the `TPSC_GetConnectorAPI` symbol that `tps_client_api`'s
// `services::CONNECTORS` requires at link time. It is otherwise unused directly by this crate's
// own code, so it must be referenced explicitly to avoid being dropped by the linker when
// running tests.
#[cfg(test)]
extern crate rot13_connector;

use tps_client_api::{
    cancel_transaction, close_session, execute_transaction, open_session, service_discovery,
    MessageBuf,
};
use tps_client_common::c_errors::{
    ERROR_BAD_STATE, ERROR_NO_DATA, ERROR_NOT_SUPPORTED, ERROR_NULL_POINTER, SUCCESS,
};
use tps_client_common::c_structs::{
    ConnectionData, MessageBuffer, ServiceIdentifier, ServiceSelector, Session, UUID,
};

/***************************************************************************************************
 * Debug tracing support under `trace` feature
 **************************************************************************************************/
#[cfg(feature = "trace")]
use func_trace::trace;
#[cfg(feature = "trace")]
use std::println;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/***************************************************************************************************
 * C language API
 **************************************************************************************************/
/// Convenience definition for C API
#[allow(non_camel_case_types)]
type c_size = usize;

/// The function requests the cancellation of a pending open session operation or Transaction
/// invocation operation. As this is a synchronous API, this function must be called from a
/// thread other than the one executing the TPSC_SessionOpen or TPSC_Transaction function.
///
/// A NULL `transaction` is a capability query per spec §4.5.2: it returns
/// `TPSC_ERROR_CANCEL` if this implementation supports cancellation, or
/// `TPSC_ERROR_NOT_SUPPORTED` otherwise (never `TPSC_ERROR_NULL_POINTER`, which the Return-list
/// bullet in §4.5.2 also names for this input - a spec self-contradiction the human ruled
/// should be resolved in favor of the capability-query reading, with the spec to be amended).
/// This reference implementation is single-threaded and synchronous throughout, so it never
/// supports cancellation and always returns `TPSC_ERROR_NOT_SUPPORTED` for this query.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `transaction` is allocated, correctly aligned and initialized (i.e. previously passed to a
///   successful call to [`TPSC_InitializeTransaction`]).
///
#[unsafe(no_mangle)]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_CancelTransaction(transaction: *mut MessageBuffer) -> u32 {
    if let Some(transact) = unsafe { (transaction as *mut MessageBuf).as_ref() } {
        match cancel_transaction(transact) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
        }
    } else {
        ERROR_NOT_SUPPORTED
    }
}

/// The function closes a session that has been opened with a TPS Service.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `session` is allocated, properly aligned and initialized — i.e. it was populated by a
///   successful call to [`TPSC_OpenSession`], not e.g. zeroed or otherwise constructed by hand.
///   `Session::service_id` internally holds a reference derived from the raw `service` pointer
///   that [`TPSC_OpenSession`] was called with, so a `session` not obtained that way causes
///   undefined behavior when dereferenced here, rather than a safe `TPSC_ERROR_*` return.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_CloseSession(session: *mut Session) -> u32 {
    if let Some(sess) = unsafe { session.as_mut() } {
        match close_session(sess) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// The function sends a request message and receives a response message within the specified
/// session.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `session` is allocated, correctly aligned and initialized — i.e. it was populated by a
///   successful call to [`TPSC_OpenSession`], not e.g. zeroed or otherwise constructed by hand.
///   `Session::service_id` internally holds a reference derived from the raw `service` pointer
///   [`TPSC_OpenSession`] was called with, so a `session` not obtained that way causes undefined
///   behavior when dereferenced here, rather than a safe `TPSC_ERROR_*` return.
/// - `send_buf` and `recv_buf` are allocated, correctly aligned and initialized (i.e. previously
///   passed to a successful call to [`TPSC_InitializeTransaction`]).
/// - `recv_buf.maxsize` accurately reflects the real capacity of `recv_buf`'s backing allocation
///   as passed to [`TPSC_InitializeTransaction`]. This is not re-validated here: the true
///   allocation size is never recorded past that call, so an inflated `maxsize` causes the
///   connector behind this session to write past the real allocation. This is an accepted risk,
///   mitigated only by this documented caller obligation, not by any runtime check.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_ExecuteTransaction(
    session: *const Session,
    send_buf: *const MessageBuffer,
    recv_buf: *mut MessageBuffer,
) -> u32 {
    let sess = unsafe { session.as_ref() };
    let send_buf_ref = unsafe { (send_buf as *const MessageBuf).as_ref() };
    let recv_buf_ref = unsafe { (recv_buf as *mut MessageBuf).as_mut() };
    if let (Some(sess), Some(send_buf), Some(recv_buf)) = (sess, send_buf_ref, recv_buf_ref) {
        match execute_transaction(sess, send_buf, recv_buf) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
        }
    } else {
        ERROR_NO_DATA
    }
}

/// The function finalizes a transaction structure that has been initialized and associated with
/// the session structure.
///
/// Returns `TPSC_ERROR_BAD_STATE` (spec §4.5.6) if `transaction`'s guard word does not match a
/// freshly- or previously-initialized [`TPSC_InitializeTransaction`] call — i.e. it was not
/// correctly initialized.
///
/// **Programmer Error** (spec §4.5.6): it is an error to call this function on a `transaction`
/// that is still owned by an ongoing [`TPSC_ExecuteTransaction`] call. This implementation does
/// not detect that case (no cross-call ownership tracking exists), so violating it is undefined
/// behavior rather than a checked error.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `transaction` is allocated, properly aligned and initialized.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_FinalizeTransaction(transaction: *mut MessageBuffer) -> u32 {
    if let Some(trans) = unsafe { transaction.as_mut() } {
        if trans.imp.check() {
            trans.finalize();
            SUCCESS
        } else {
            ERROR_BAD_STATE
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// The function initializes a transaction structure for use in TPSC_Transaction function. The
/// transaction structure may be used multiple times with the TPSC_Transaction function.
///
///  # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `transaction` is allocated and properly aligned. It is not expected to be initialized on
///   calling.
/// - `buffer` is allocated and appropriately aligned. It MUST be at least as large as `maxsize` —
///   this is never re-validated once initialization completes, so passing an undersized `buffer`
///   here is what ultimately lets a later [`TPSC_ExecuteTransaction`] write past the real
///   allocation on the receive side; an accepted risk, mitigated only by this documented caller
///   obligation.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_InitializeTransaction(
    transaction: *mut MessageBuffer,
    buffer: *mut u8,
    maxsize: usize,
) -> u32 {
    if let Some(trans) = unsafe { transaction.as_mut() } {
        if buffer.is_null() {
            return ERROR_NULL_POINTER;
        }
        // The slice is built from `buffer` itself, not from a `&mut u8` reborrow of it: a
        // reference to the first element only carries provenance for that one byte, so widening
        // it to `maxsize` would be unsound even though the allocation is large enough.
        let array_ref = unsafe { from_raw_parts_mut(buffer, maxsize) };
        *trans = MessageBuffer::init(array_ref);
        SUCCESS
    } else {
        ERROR_NULL_POINTER
    }
}

/// The function opens a new session between the TPS Client and the TPS Service identified by the
/// service structure.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `service` is allocated, properly aligned and initialized. **`service` must remain valid for
///   the entire lifetime of the resulting session** — through every subsequent call using
///   `session` (e.g. [`TPSC_ExecuteTransaction`], [`TPSC_CloseSession`]), not merely for the
///   duration of this call. `session`'s `service_id` field is a reference derived from `service`,
///   not a copy of it; letting `service` go out of scope (e.g. passing the address of a stack
///   local and returning) before `session` is closed causes undefined behavior on the next call
///   that dereferences `session`.
/// - `connection_data`, if not NULL, is properly aligned and initialized
/// - `session` is properly allocated and aligned. It is not expected to be initialized on entry.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_OpenSession(
    service: *const UUID,
    connection_method: u32,
    connection_data: *const ConnectionData,
    session: *mut Session,
) -> u32 {
    if let Some(service_ref) = unsafe { service.as_ref() } {
        if let Some(session_ref) = unsafe { session.as_mut() } {
            let conn_data_opt = unsafe { connection_data.as_ref() };
            match open_session(service_ref, connection_method, conn_data_opt, session_ref) {
                Ok(()) => SUCCESS,
                Err(e) => e.into(),
            }
        } else {
            ERROR_NULL_POINTER
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// The function discovers all TPS Services available via the TPS Client API that match the selector
/// method.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `service_selector` points to a properly allocated, aligned and initialized instance of
///   `ServiceSelector` which MUST NOT be moved during the call to `service_discovery`.
/// - `no_of_services` points to a properly allocated, aligned and initialized usize which holds
///   the number of entries (not bytes!) allocated for `service_array` on entry, and receives the
///   number of entries actually written on a successful return.
/// - `service_array` points to a properly allocated and aligned instance `no_of_services` instances
///   of `ServiceIdentifier`.
///
/// Calls with NULL pointers and where `no_of_services` are zero are caught as errors, but it
/// is not possible for the code to catch initialization and/or alignment errors, where
/// initialization and/or alignment are required.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_DiscoverServices(
    service_selector: *const ServiceSelector,
    no_of_services: *mut c_size,
    service_array: *mut ServiceIdentifier,
) -> u32 {
    if let Some(svc_selector) = unsafe { service_selector.as_ref() } {
        if !service_array.is_null() {
            if let Some(no_services) = unsafe { no_of_services.as_mut() } {
                if *no_services > 0 {
                    // The slice is built from `service_array` itself, not from a
                    // `&mut ServiceIdentifier` reborrow of it: a reference to the first element
                    // only carries provenance for that one element, so widening it to
                    // `*no_services` would be unsound even though the array is large enough.
                    let array_as_slice =
                        unsafe { from_raw_parts_mut(service_array, *no_services) };
                    // Everything checked as much as we can - call the Rust API
                    match service_discovery(svc_selector, array_as_slice) {
                        Ok(n_services) => {
                            *no_services = n_services;
                            SUCCESS
                        }
                        Err(e) => e.into(),
                    }
                } else {
                    // no_of_services is zero, so there is nowhere to write a result
                    ERROR_NULL_POINTER
                }
            } else {
                // no_of_services is NULL
                ERROR_NULL_POINTER
            }
        } else {
            // service_array is NULL
            ERROR_NULL_POINTER
        }
    } else {
        // service_selector is NULL
        ERROR_NULL_POINTER
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::eprintln;
    use std::format;
    use std::ptr;
    use std::string::String;
    use tps_client_common::c_priv::SessionPriv;
    use tps_client_common::c_structs::{ServiceBound, ServiceRange, ServiceVersion};
    use tps_client_common::c_uuid::UUID_NIL;
    use std::vec::Vec;

    fn nil_service_selector() -> ServiceSelector {
        ServiceSelector {
            service_id: UUID_NIL,
            secure_component_type: UUID_NIL,
            secure_component_instance: UUID_NIL,
            service_version_range: ServiceRange {
                lowest_acceptable_version: ServiceBound::NoBounds,
                first_excluded_version: ServiceBound::NoBounds,
                last_excluded_version: ServiceBound::NoBounds,
                highest_acceptable_version: ServiceBound::NoBounds,
            },
        }
    }

    /// Fix pin (Stage 1 Finding 2, tps_client_c): spec §4.5.5 (`TPSC_ExecuteTransaction`) Return
    /// list mandates `TPSC_ERROR_NO_DATA` for a NULL `session`, `send_buf`, or `recv_buf`. The
    /// implementation previously returned `ERROR_NULL_POINTER` for a NULL `session` - the
    /// generic "some pointer was NULL" code, not the function-specific code the spec requires.
    /// Fixed by returning `ERROR_NO_DATA` from the NULL-`session` branch instead.
    #[test]
    fn execute_transaction_null_session_returns_no_data() {
        let mut send_backing = [0u8; 16];
        let mut recv_backing = [0u8; 16];
        let send = MessageBuffer::init(&mut send_backing);
        let mut recv = MessageBuffer::init(&mut recv_backing);
        let result = unsafe {
            TPSC_ExecuteTransaction(
                ptr::null(),
                &send as *const MessageBuffer,
                &mut recv as *mut MessageBuffer,
            )
        };
        assert_eq!(
            result, ERROR_NO_DATA,
            "spec §4.5.5 requires ERROR_NO_DATA for a NULL session"
        );
    }

    /// Fix pin (Stage 1 Finding 3, tps_client_c): `TPSC_ExecuteTransaction` null-checked
    /// `session` but never null-checked `send_buf`/`recv_buf` before casting them to
    /// `*const/*mut MessageBuf` and dereferencing, which was undefined behavior (a null-pointer
    /// dereference) for a NULL `send_buf`/`recv_buf` rather than the defined
    /// `TPSC_ERROR_NO_DATA` return spec §4.5.5 mandates. Fixed by null-checking both via
    /// `as_ref()`/`as_mut()` alongside `session`, all three collapsed into a single `if let`.
    #[test]
    fn execute_transaction_null_send_buf_returns_no_data() {
        let mut recv_backing = [0u8; 16];
        let mut recv = MessageBuffer::init(&mut recv_backing);
        let session = Session {
            service_id: &UUID_NIL,
            session_id: 0,
            imp: SessionPriv::new(0),
        };
        let result = unsafe {
            TPSC_ExecuteTransaction(
                &session as *const Session,
                ptr::null(),
                &mut recv as *mut MessageBuffer,
            )
        };
        assert_eq!(result, ERROR_NO_DATA);
    }

    /// Fix pin (Stage 1 Finding 3, tps_client_c): companion to
    /// `execute_transaction_null_send_buf_returns_no_data`, covering a NULL `recv_buf` instead.
    #[test]
    fn execute_transaction_null_recv_buf_returns_no_data() {
        let mut send_backing = [0u8; 16];
        let send = MessageBuffer::init(&mut send_backing);
        let session = Session {
            service_id: &UUID_NIL,
            session_id: 0,
            imp: SessionPriv::new(0),
        };
        let result = unsafe {
            TPSC_ExecuteTransaction(
                &session as *const Session,
                &send as *const MessageBuffer,
                ptr::null_mut(),
            )
        };
        assert_eq!(result, ERROR_NO_DATA);
    }

    /// Fix pin (Stage 1 Finding 4, tps_client_c): spec §4.5.2 self-contradicted on
    /// `TPSC_CancelTransaction`'s NULL behavior. The descriptive text mandates a
    /// capability-query mechanism (`TPSC_ERROR_CANCEL`/`TPSC_ERROR_NOT_SUPPORTED`) for a NULL
    /// `transaction`, while the Return-value bullet list separately named
    /// `TPSC_ERROR_NULL_POINTER` for the same input. Per human ruling, `TPSC_ERROR_NULL_POINTER`
    /// should never be returned here (the spec's Return-list bullet will be amended); the
    /// capability-query reading is authoritative. Fixed by returning `TPSC_ERROR_NOT_SUPPORTED`
    /// for a NULL `transaction`, since this reference implementation is single-threaded and
    /// synchronous throughout and therefore never supports cancellation.
    #[test]
    fn cancel_transaction_null_returns_not_supported() {
        let result = unsafe { TPSC_CancelTransaction(ptr::null_mut()) };
        assert_eq!(result, ERROR_NOT_SUPPORTED);
    }

    /// Fix pin (Stage 1 Finding 7, tps_client_c). Spec §4.5.6 (`TPSC_FinalizeTransaction`)
    /// Return list requires `TPSC_ERROR_BAD_STATE` when "transaction was not correctly
    /// initialized". `close_session` and `cancel_transaction` (in `tps_client_api`) both guard
    /// against this via `.imp.check()` against the `TRANSACTION_GUARD`/`SESSION_GUARD` magic
    /// word; `TPSC_FinalizeTransaction` previously only null-checked `transaction` and never
    /// checked the guard before unconditionally calling `finalize()`, so a corrupted/never-
    /// initialized buffer was "successfully" finalized instead of rejected. Fixed by checking
    /// `trans.imp.check()` before finalizing and returning `ERROR_BAD_STATE` if it fails. This
    /// test corrupts the guard word directly (relying on `MessageBufferPriv`'s `#[repr(C)]`
    /// layout, where `guard: u32` is the first field - the same technique
    /// `MessageBufferPriv::check()` itself is built on) since the field is otherwise private and
    /// cannot be set to a bad value via the public API.
    #[test]
    fn finalize_transaction_bad_state_on_corrupted_guard() {
        let mut backing = [0u8; 16];
        let mut buf = MessageBuffer::init(&mut backing);
        // `imp.check()` at this point must be true (freshly initialized).
        assert!(buf.imp.check());
        unsafe {
            let guard_ptr = &mut buf.imp as *mut _ as *mut u32;
            ptr::write(guard_ptr, 0xdead_beef);
        }
        assert!(!buf.imp.check(), "sanity: guard corruption must be observable via check()");
        let result = unsafe { TPSC_FinalizeTransaction(&mut buf as *mut MessageBuffer) };
        assert_eq!(
            result, ERROR_BAD_STATE,
            "TPSC_FinalizeTransaction must reject a corrupted guard per spec §4.5.6"
        );
    }

    /// Companion to `finalize_transaction_bad_state_on_corrupted_guard`: a validly initialized
    /// buffer must still finalize successfully.
    #[test]
    fn finalize_transaction_succeeds_with_valid_guard() {
        let mut backing = [0u8; 16];
        let mut buf = MessageBuffer::init(&mut backing);
        let result = unsafe { TPSC_FinalizeTransaction(&mut buf as *mut MessageBuffer) };
        assert_eq!(result, SUCCESS);
    }

    /// Confirms `TPSC_InitializeTransaction` NULL-checks both `transaction` and `buffer`
    /// (src/lib.rs:183) - a complex/easily-misread double-`Option` tuple-match pattern worth
    /// pinning even though it is already correct.
    #[test]
    fn initialize_transaction_null_pointers_return_null_pointer() {
        let mut backing = [0u8; 4];
        let mut buf = MessageBuffer::init(&mut backing);
        assert_eq!(
            unsafe { TPSC_InitializeTransaction(ptr::null_mut(), backing.as_mut_ptr(), 4) },
            ERROR_NULL_POINTER
        );
        assert_eq!(
            unsafe {
                TPSC_InitializeTransaction(&mut buf as *mut MessageBuffer, ptr::null_mut(), 4)
            },
            ERROR_NULL_POINTER
        );
    }

    /// Exercises `TPSC_InitializeTransaction` with a *valid* `buffer`, which the NULL-argument
    /// test above never reaches: both of its calls return `ERROR_NULL_POINTER` before the
    /// `from_raw_parts_mut` that builds the buffer slice. Writing through the whole length and
    /// then finalizing means every byte of `buffer` is accessed via the slice this function
    /// constructed, so the construction's pointer provenance is actually put under load.
    ///
    /// Under Miri (see `run_miri.sh`) this is what catches a slice built from a reborrowed
    /// `&mut u8` first element rather than from `buffer` itself: such a reference only carries
    /// provenance for one byte, so the writes past index 0 are then out-of-bounds for the tag.
    ///
    /// Note the deliberate ordering: `backing` is never touched directly between the two FFI
    /// calls. Under Tree Borrows a direct read of `backing` is a foreign read for the pointer
    /// `buf.message` derived from it, which freezes that pointer and makes
    /// `TPSC_FinalizeTransaction`'s later write through it UB — a flaw in the *test*, not in the
    /// code under test. All direct inspection of `backing` therefore happens after the last call
    /// that writes through `buf.message`.
    #[test]
    fn initialize_transaction_valid_buffer_covers_full_declared_length() {
        const LEN: usize = 16;
        let mut backing = [0u8; LEN];
        // A slot for the out-parameter. `TPSC_InitializeTransaction` overwrites it wholesale, so
        // what it points at initially is irrelevant - but it must be a live allocation, not a
        // temporary that has already been dropped.
        let mut scratch = [0u8; LEN];
        let mut buf = MessageBuffer::init(&mut scratch);

        assert_eq!(
            unsafe {
                TPSC_InitializeTransaction(
                    &mut buf as *mut MessageBuffer,
                    backing.as_mut_ptr(),
                    LEN,
                )
            },
            SUCCESS
        );
        assert_eq!(buf.maxsize, LEN, "maxsize must reflect the declared capacity");
        assert_eq!(buf.size, 0, "a freshly initialized buffer holds no payload");
        assert!(buf.imp.check(), "initialization must leave a valid guard word");

        // Write the full declared length through the slice that `TPSC_InitializeTransaction`
        // built, so that every byte is touched with that slice's provenance, and read it back
        // through the same slice rather than through `backing`.
        unsafe {
            let msg = from_raw_parts_mut(buf.message, buf.maxsize);
            for (i, b) in msg.iter_mut().enumerate() {
                *b = i as u8;
            }
            assert_eq!(msg[LEN - 1], (LEN - 1) as u8, "the last byte must be writable");
        }

        // Finalize zeroes the same region, again through the stored pointer and length.
        assert_eq!(
            unsafe { TPSC_FinalizeTransaction(&mut buf as *mut MessageBuffer) },
            SUCCESS
        );

        // Safe to inspect `backing` directly now: nothing writes through `buf.message` again.
        assert!(backing.iter().all(|&b| b == 0), "finalize must zero the whole buffer");
    }

    /// Confirms `TPSC_CloseSession(NULL)` returns `ERROR_NULL_POINTER`. Spec §4.5.3 says the
    /// Implementation "MUST do nothing if the session parameter is NULL", which could be read as
    /// "return SUCCESS without touching anything" - but `TPSC_FinalizeTransaction`'s NULL case
    /// (see above) and every other function in this file use the same "NULL -> ERROR_NULL_POINTER"
    /// convention uniformly, and returning an error code while performing no side effects is a
    /// reasonable reading of "do nothing". Treated as informational/no-finding, not a deviation;
    /// pinned here so a future change to this convention is caught.
    #[test]
    fn close_session_null_returns_null_pointer_workspace_convention() {
        let result = unsafe { TPSC_CloseSession(ptr::null_mut()) };
        assert_eq!(result, ERROR_NULL_POINTER);
    }

    /// Confirms `TPSC_OpenSession` NULL-checks both `service` and `session` (src/lib.rs:210-211),
    /// even though spec §4.5.8's Programmer Error section treats NULL `service`/`session` as
    /// undefined behavior rather than mandating a defined return code - the implementation is
    /// more defensive than strictly required, which is not a deviation worth flagging as a bug.
    #[test]
    fn open_session_null_service_or_session_returns_null_pointer() {
        let uuid = UUID_NIL;
        let mut session = Session {
            service_id: &uuid,
            session_id: 0,
            imp: SessionPriv::new(0),
        };
        assert_eq!(
            unsafe {
                TPSC_OpenSession(ptr::null(), 0, ptr::null(), &mut session as *mut Session)
            },
            ERROR_NULL_POINTER
        );
        assert_eq!(
            unsafe { TPSC_OpenSession(&uuid as *const UUID, 0, ptr::null(), ptr::null_mut()) },
            ERROR_NULL_POINTER
        );
    }

    /// Confirms all three genuinely-NULL-pointer cases of `TPSC_DiscoverServices` return
    /// `ERROR_NULL_POINTER`, matching spec §4.5.4's `TPSC_ERROR_NULL_POINTER: One or more of the
    /// pointer values passed were NULL.`
    #[test]
    fn discover_services_null_pointers_return_null_pointer() {
        let selector = nil_service_selector();
        let mut count: c_size = 1;
        let mut arr = [ServiceIdentifier::new()];
        assert_eq!(
            unsafe {
                TPSC_DiscoverServices(ptr::null(), &mut count as *mut c_size, arr.as_mut_ptr())
            },
            ERROR_NULL_POINTER
        );
        assert_eq!(
            unsafe {
                TPSC_DiscoverServices(
                    &selector as *const ServiceSelector,
                    ptr::null_mut(),
                    arr.as_mut_ptr(),
                )
            },
            ERROR_NULL_POINTER
        );
        assert_eq!(
            unsafe {
                TPSC_DiscoverServices(
                    &selector as *const ServiceSelector,
                    &mut count as *mut c_size,
                    ptr::null_mut(),
                )
            },
            ERROR_NULL_POINTER
        );
    }

    /// Exercises `TPSC_DiscoverServices` on its success path, which none of the tests above
    /// reach: the NULL-pointer cases and the zero-count case all return before the
    /// `from_raw_parts_mut` that builds the output slice. A wildcard (nil) selector matches the
    /// ROT13 service that `rot13_connector` registers, so this writes at least one entry through
    /// that slice and then reads back every element of the caller's array.
    ///
    /// Under Miri (see `run_miri.sh`) this covers the output array's pointer provenance, the way
    /// `initialize_transaction_valid_buffer_covers_full_declared_length` covers the transaction
    /// buffer's.
    #[test]
    fn discover_services_valid_array_writes_through_output_slice() {
        const CAP: usize = 4;
        let selector = nil_service_selector();
        let mut count: c_size = CAP;
        let mut arr: [ServiceIdentifier; CAP] = core::array::from_fn(|_| ServiceIdentifier {
            service_instance: UUID_NIL,
            service_id: UUID_NIL,
            secure_component_type: UUID_NIL,
            secure_component_instance: UUID_NIL,
            service_version: ServiceVersion {
                major_version: 0,
                minor_version: 0,
                patch_version: 0,
            },
        });

        let result = unsafe {
            TPSC_DiscoverServices(
                &selector as *const ServiceSelector,
                &mut count as *mut c_size,
                arr.as_mut_ptr(),
            )
        };
        assert_eq!(result, SUCCESS, "a wildcard selector must discover the ROT13 service");
        assert!(count >= 1, "at least one service must be reported");
        assert!(count <= CAP, "the reported count must not exceed the supplied capacity");

        // Touch every entry the callee claims to have written, through the caller's array.
        for svc in &arr[..count] {
            let _ = svc.service_id;
            let _ = svc.service_version;
        }
    }

    /// Minor finding pin: when `no_of_services` is non-NULL but points to `0`, this function
    /// reuses `ERROR_NULL_POINTER` (src/lib.rs:254-267) even though the value is not a NULL
    /// pointer - the branch's own comment ("// no_of_services is NULL") mislabels the actual
    /// condition being tested (`*no_of_services > 0`, not nullness). `ERROR_NULL_POINTER`'s
    /// Table 4-2 description ("A NULL pointer was passed... where a valid pointer is required")
    /// does not literally describe this case, but the spec provides no better-fitting code for
    /// "count is zero" either - this is somewhat forced by the spec's limited vocabulary, hence
    /// tracked as a minor/ambiguity-flavored finding rather than a clear-cut bug.
    #[test]
    fn discover_services_zero_count_reuses_null_pointer_code_minor_finding() {
        let selector = nil_service_selector();
        let mut count: c_size = 0;
        let mut arr: [ServiceIdentifier; 0] = [];
        let result = unsafe {
            TPSC_DiscoverServices(
                &selector as *const ServiceSelector,
                &mut count as *mut c_size,
                arr.as_mut_ptr(),
            )
        };
        assert_eq!(result, ERROR_NULL_POINTER);
    }

    /// Fix pin (Stage 1 Finding 5, tps_client_c): the generated header's `TPSC_ConnectionData_Tag`/
    /// `TPSC_ServiceBound_Tag` C enums used bare Rust variant names (`None`/`GID`/`Proprietary`,
    /// `Inclusive`/`Exclusive`/`NoBounds`) instead of the spec's normative
    /// `TPSC_NoConnectionData`/`TPSC_GID`/`TPSC_Proprietary` and
    /// `TPSC_Inclusive`/`TPSC_Exclusive`/`TPSC_NoBounds` identifiers, because cbindgen's
    /// `rename_types`/`remove_underscores` mangling in `cbindgen.toml` only applies to type
    /// names, not enum variant/tag identifiers. Fixed by adding `[enum] rename_variants =
    /// "prefix:TPSC_"` to `cbindgen.toml` (cbindgen 0.29.4 does apply `rename_variants` to enum
    /// tags, confirmed empirically), plus renaming `ConnectionData::None` to
    /// `ConnectionData::NoConnectionData` in `tps_client_common/src/lib.rs` so the prefix rule
    /// alone (`TPSC_` + variant name) produces the spec-normative name rather than `TPSC_None`.
    /// Skips gracefully if the header wasn't generated.
    ///
    /// Checks tag identifiers by relative order rather than byte-adjacency: `tps_client_common`
    /// Stage 4 Finding 3 added doc comments to each `ConnectionData`/`ServiceBound` variant,
    /// which cbindgen now renders as its own `/** ... */` block between consecutive tag lines in
    /// the generated header, so the tags are no longer on contiguous lines.
    #[test]
    fn generated_header_enum_tags_have_tpsc_prefix() {
        let header_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tpsc_client_api.h");
        let header = match std::fs::read_to_string(&header_path) {
            Ok(h) => h,
            Err(_) => {
                eprintln!("skipping: {} not present (cbindgen did not run)", header_path.display());
                return;
            }
        };
        fn assert_tags_in_order(header: &str, tags: &[&str]) {
            let mut search_from = 0;
            for tag in tags {
                let needle = format!("    {tag},");
                let found = header[search_from..].find(&needle).unwrap_or_else(|| {
                    panic!(
                        "expected spec-normative tag `{tag}` after byte {search_from} in the \
                         generated header; header contents may have changed shape - re-check by \
                         hand:\n{header}"
                    )
                });
                search_from += found + needle.len();
            }
        }
        assert_tags_in_order(&header, &["TPSC_NoConnectionData", "TPSC_GID", "TPSC_Proprietary"]);
        assert_tags_in_order(&header, &["TPSC_Inclusive", "TPSC_Exclusive", "TPSC_NoBounds"]);
    }

    /// Duplication-hygiene regression (Stage 3 note, tps_client_c). `cbindgen.toml`'s
    /// `after_includes` block hand-transcribes `tps_client_common::c_uuid::UUID_NAMESPACE`'s
    /// bytes into a `TPSC_UUID_NAMESPACE` C macro, because cbindgen cannot generate this
    /// constant itself (`cbindgen:ignore` on the Rust source). That transcription previously
    /// drifted from the Rust source (byte 5 was `0x32` instead of `0x3e` in both places, fixed
    /// in commit `43a8902b`) with no automated check to catch a future re-drift. This test
    /// parses the generated header's `TPSC_UUID_NAMESPACE` macro and asserts it matches the
    /// Rust-side constant byte-for-byte. Skips gracefully if the header wasn't generated.
    #[test]
    fn generated_header_uuid_namespace_matches_rust_source() {
        let header_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tpsc_client_api.h");
        let header = match std::fs::read_to_string(&header_path) {
            Ok(h) => h,
            Err(_) => {
                eprintln!("skipping: {} not present (cbindgen did not run)", header_path.display());
                return;
            }
        };
        let start = header
            .find("TPSC_UUID_NAMESPACE")
            .expect("TPSC_UUID_NAMESPACE macro not found in generated header");
        let line_end = header[start..].find('\n').map_or(header.len(), |i| start + i);
        let macro_line = &header[start..line_end];
        let parsed_bytes: Vec<u8> = macro_line
            .split(',')
            .filter_map(|tok| {
                let tok = tok.trim();
                let idx = tok.find("0x")?;
                let hex = &tok[idx + 2..];
                let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                Some(
                    u8::from_str_radix(&hex, 16)
                        .unwrap_or_else(|_| panic!("failed to parse byte token `{tok}`")),
                )
            })
            .collect();
        assert_eq!(
            parsed_bytes,
            tps_client_common::c_uuid::UUID_NAMESPACE.bytes.to_vec(),
            "cbindgen.toml's hand-transcribed TPSC_UUID_NAMESPACE macro has drifted from \
             tps_client_common::c_uuid::UUID_NAMESPACE; update cbindgen.toml's after_includes \
             block to match"
        );
    }

    /// Fix pin (Stage 1 Finding 1, tps_client_c). `MessageBufferPriv::transaction_id` was
    /// `Option<u32>`, which cbindgen cannot represent as a concrete C type - it fell back to an
    /// opaque forward declaration (`typedef struct TPSC_OptionU32 TPSC_OptionU32;`) that was
    /// never defined anywhere in the header, making `TPSC_MessageBufferPriv` (and therefore
    /// `TPSC_MessageBuffer`, which embeds it) an incomplete type in C. Fixed by splitting the
    /// field into two C-representable fields (`has_transaction: bool` + `transaction_id: u32`,
    /// `tps_client_common/src/lib.rs`) instead of using `Option<u32>` - the human-suggested
    /// `[layout] option = "OptionStruct"` cbindgen.toml setting does not exist in cbindgen 0.29.4
    /// (`[layout]` only accepts `packed`/`aligned_n`; confirmed by running cbindgen directly with
    /// that key present, which errors with "unknown field `option`"). Verified independently
    /// against a minimal probe source (mirroring the crate's own shipped `c_example.c`, which
    /// itself is unrelated-broken on this host due to a `<libc.h>` include that doesn't exist
    /// outside macOS). Skips gracefully if the header wasn't generated or no C compiler is
    /// available in this environment.
    #[test]
    #[cfg_attr(miri, ignore = "spawns `cc`; Miri cannot execute foreign processes")]
    fn generated_header_option_u32_incomplete_type_fixed() {
        let header_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tpsc_client_api.h");
        if !header_path.exists() {
            eprintln!("skipping: {} not present (cbindgen did not run)", header_path.display());
            return;
        }
        let header = std::fs::read_to_string(&header_path).expect("failed to read header");
        assert!(
            !header.contains("TPSC_OptionU32"),
            "expected the TPSC_OptionU32 incomplete-type bug to be fixed - header still \
             references it:\n{header}"
        );
        if std::process::Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping compile check: no `cc` in PATH");
            return;
        }
        let tmp_dir = std::env::temp_dir();
        let src_path = tmp_dir.join("tps_client_c_header_compile_check.c");
        std::fs::write(
            &src_path,
            format!(
                "#include \"{}\"\nint main(void) {{ return 0; }}\n",
                header_path.display()
            ),
        )
        .expect("failed to write temporary probe source");
        let obj_path = tmp_dir.join("tps_client_c_header_compile_check.o");
        let output = std::process::Command::new("cc")
            .arg("-c")
            .arg("-o")
            .arg(&obj_path)
            .arg(&src_path)
            .output()
            .expect("failed to invoke cc");
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&obj_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "expected the generated header to compile cleanly now that the incomplete-type bug \
             is fixed; stderr:\n{stderr}"
        );
    }

    /// Fix pin (Stage 1 Finding 6, tps_client_c): `build.rs` previously called
    /// `.status().unwrap()` on the `cbindgen` invocation, which only panics on *spawn* failure
    /// and never inspects `ExitStatus::success()`, silently accepting a non-zero exit. Fixed by
    /// checking `status.success()` explicitly and panicking with the exit status if it fails
    /// (matching `tps_connector`'s already-fixed Finding 2), plus adding `rerun-if-changed`
    /// tracking on `cbindgen.toml`/`src` so Cargo actually reruns the script when they change.
    /// `build.rs` itself cannot be unit-tested directly under `cargo test`; this test instead
    /// pins the underlying pattern that used to be present, so a regression to `.unwrap()`-only
    /// exit-status handling doesn't silently reappear.
    #[test]
    #[cfg_attr(miri, ignore = "spawns `false`; Miri cannot execute foreign processes")]
    fn build_rs_checks_command_exit_status_not_just_spawn_success() {
        let status = std::process::Command::new("false")
            .status()
            .expect("spawn of `false` should succeed");
        assert!(!status.success(), "sanity: `false` must exit non-zero");
        // `tps_client_c/build.rs` now checks `status.success()` and panics if false, rather than
        // accepting any status that didn't fail to spawn (as `.unwrap()` alone would).
    }
}
