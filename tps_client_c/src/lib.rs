/***************************************************************************************************
 * Copyright (c) 2021-2022, Qualcomm Innovation Center, Inc. All rights reserved.
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
 * Exported TPS Client API (C language)
 **************************************************************************************************/
#![no_std]

// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set.
#[cfg(any(feature = "std", test))]
extern crate std;

// If we are really building no_std, pull in core as well. It is aliased as std so that "use"
// statements are always the same
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

use std::ptr;
use std::slice::from_raw_parts_mut;

extern crate tps_client_api;
extern crate tps_client_common;

use tps_client_api::{
    cancel_transaction, clear_transaction, close_session, execute_transaction,
    finalize_transaction, initialize_transaction, open_session, service_discovery,
};
use tps_client_common::c_errors::{ERROR_NULL_POINTER, SUCCESS};
use tps_client_common::c_structs::{
    ConnectionData, MessageBuffer, ServiceIdentifier, ServiceSelector, Session, UUID,
};

/***************************************************************************************************
 * Debug tracing support under `trace` feature
 **************************************************************************************************/
#[cfg(feature = "trace")]
use func_trace::trace;

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
/// **NB:** Cancellation not supported in first release.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `transaction` is allocated, correctly aligned and initialized.
///
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_CancelTransaction(transaction: *mut MessageBuffer) -> u32 {
    if let Some(transact) = transaction.as_mut() {
        match cancel_transaction(transact) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// This function clears the data in a TPSC_Transaction instance.
/// Callers may wish to clear the contents of a TPSC_Transaction for several reasons:
///
/// - To ensure that the transaction is cleared to a known state before it is re-used.
/// - To ensure that sensitive information is cleared from memory as soon as it is no-longer needed.
/// - To ensure that information does not remain in memory after the transaction has been finalized.
///
/// # Safety
///
/// - `transaction` is allocated, properly aligned and initialized.
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_ClearTransaction(transaction: *mut MessageBuffer) -> u32 {
    if let Some(transact) = transaction.as_mut() {
        match clear_transaction(transact) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// The function closes a session that has been opened with a TPS Service.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `session` is allocated, properly aligned and initialized.
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_CloseSession(session: *mut Session) -> u32 {
    if let Some(sess) = session.as_mut() {
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
/// - `session` is allocated, correctly aligned and initialized.
/// - `transaction` is allocated, correctly aligned and initialized.
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_ExecuteTransaction(
    session: *const Session,
    send_buf: *const MessageBuffer,
    recv_buf: *mut MessageBuffer,
) -> u32 {
    if let Some(sess) = session.as_ref() {
        match execute_transaction(sess, &*send_buf, &mut *recv_buf) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// The function finalizes a transaction structure that has been initialized and associated with
/// the session structure.
///
/// # Safety
///
/// This function assumes that the caller ensures the following invariants are maintained:
///
/// - `transaction` is allocated, properly aligned and initialized.
/// - `buf` is the address of an uninitialized u8 pointer
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_FinalizeTransaction(transaction: *mut MessageBuffer) -> u32 {
    if let Some(trans) = transaction.as_mut() {
        match finalize_transaction(trans) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
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
/// - `buffer` is allocated and appropriately aligned. It MUST be at least as large as `maxsize`.
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_InitializeTransaction(
    transaction: *mut MessageBuffer,
    buffer: *mut u8,
    maxsize: usize,
) -> u32 {
    if let (Some(trans), Some(buf)) = (transaction.as_mut(), buffer.as_mut()) {
        let array_ref = from_raw_parts_mut(buf, maxsize);
        match initialize_transaction(trans, array_ref) {
            Ok(()) => SUCCESS,
            Err(e) => e.into(),
        }
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
/// - `service` is allocated, properly aligned and initialized.
/// - `connection_data`, if not NULL, is properly aligned and initialized
/// - `session` is properly allocated and aligned. It is not expected to be initialized on entry.
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_OpenSession(
    service: *const UUID,
    connection_method: u32,
    connection_data: *const ConnectionData,
    session: *mut Session,
) -> u32 {
    if let Some(service_ref) = service.as_ref() {
        if let Some(session_ref) = session.as_mut() {
            let conn_data_opt = connection_data.as_ref();
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
/// - `max_entries` points to a properly allocated, aligned and initialized usize which holds the
///   number of entries (not bytes!) allocated for `service_array`
/// - `no_of_services` points to a properly allocated and aligned usize. The value, on calling the
///   function is the number of elements in `service_array`.
/// - `service_array` points to a properly allocated and aligned instance `no_of_services` instances
///   of `ServiceIdentifier`.
///
/// Calls with NULL pointers and where `no_of_services` are zero are caught as errors, but it
/// is not possible for the code to catch initialization and/or alignment errors, where
/// initialization and/or alignment are required.
#[no_mangle]
#[cfg_attr(feature = "trace", trace)]
pub unsafe extern "C" fn TPSC_ServiceDiscovery(
    service_selector: *const ServiceSelector,
    no_of_services: *mut c_size,
    service_array: *mut ServiceIdentifier,
) -> u32 {
    if let Some(svc_selector) = service_selector.as_ref() {
        if let Some(svc_array) = service_array.as_mut() {
            if let Some(no_services) = no_of_services.as_mut() {
                if *no_of_services > 0 {
                    let array_as_slice = from_raw_parts_mut(svc_array, *no_of_services);
                    // Everything checked as much as we can - call the Rust API
                    match service_discovery(svc_selector, array_as_slice) {
                        Ok(n_services) => {
                            ptr::write(no_services, n_services);
                            SUCCESS
                        }
                        Err(e) => e.into(),
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
            // buf_size if NULL
            ERROR_NULL_POINTER
        }
    } else {
        // service_selector is NULL
        ERROR_NULL_POINTER
    }
}
