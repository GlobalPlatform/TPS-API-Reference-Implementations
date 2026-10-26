/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/

use crate::service;

use tps_client_common::c_errors::{ERROR_NULL_POINTER, SUCCESS};
use tps_client_common::c_structs::{ConnectionData, ServiceIdentifier, UUID};
use tps_error::TPSError;

use crate::service::execute_transaction;
use std::slice::{from_raw_parts, from_raw_parts_mut};

/// C language API for calling the [`connect`] function
///
/// Performs any operations required to connect to a Secure Component so that the
/// [`c_service_discovery`] function can be called.
///
/// # Parameters
///
/// - `connection_method`: A `uint32_t`, which should have one of the values specified in TPS Client
///   API Specification, Section 4.4.2.
/// - `connection_data`:
///   - If NULL, indicates that there is no ConnectionData supplied
///   - If non-NULL, points to a valid [`ConnectionData`] structure.
/// - `connection_id`: A pointer to a `uint32_t` which will be updated with transaction ID if the
///   function returns successfully.
///
/// # Safety
///
/// - `connection_id` must be a valid *and writeable* instance of `u32`. This implementation assumes
///   that it can update `connection_id` at any time.
///
/// Basic checks that `connection_data` and `connection_id` are not NULL are performed, and an error
/// will be returned if they are NULL.
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c_connect(
    connection_method: u32,
    connection_data: *const ConnectionData,
    connection_id: *mut u32,
) -> u32 {
    if !connection_id.is_null() {
        let connection_info = if connection_data.is_null() {
            None
        } else {
            Some(unsafe { &*connection_data })
        };
        match service::connect(connection_method, connection_info) {
            Ok(conn_id) => {
                unsafe { *connection_id = conn_id };
                SUCCESS
            }
            Err(e) => e.into(),
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// C language API for calling the [`disconnect`] function
///
/// Performs any functions required to disconnect from a Secure Component
///
/// # Parameters
///
/// - `connection_id`: The connection ID value to disconnect.
///
/// # Safety
///
/// There are no specific memory-safety issues with this function.
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c_disconnect(connection_id: u32) -> u32 {
    match service::disconnect(connection_id) {
        Ok(()) => SUCCESS,
        Err(e) => e.into(),
    }
}

/// C language API for calling the [`discover_services`] function.
///
/// This implementation assumes that result_buf will not be read or written into by any other
/// entity while the function is executing. Ity is the responsibility of the caller to ensure this.
///
/// # Parameters
///
/// - `connection_id`: The connection ID previously returned by [`c_connect`].
///
/// # Safety
///
/// - `result_buf` must be large enough to hold at least the number of instances of
///   [`ServiceIdentifier`] specified by `len` when the function is called.
///
/// Both `result_buf` and `len` are checked before use to ensure that they are not NULL.
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c_discover_services(
    connection_id: u32,
    result_buf: *mut ServiceIdentifier,
    len: *mut usize,
) -> u32 {
    match service::discover_services(connection_id) {
        Ok(services) => {
            let out_sz = services.len();
            if !len.is_null() && !result_buf.is_null() {
                if out_sz > unsafe { *len } {
                    // Provided buffer too small for the data. Indicate required size
                    unsafe { *len = out_sz };
                    TPSError::ShortBuffer(out_sz).into()
                } else {
                    // Copy Service identifier bytes into result_buf
                    let dest = unsafe { from_raw_parts_mut(result_buf, *len) };
                    dest[..out_sz].clone_from_slice(&services[..out_sz]);
                    // Update len with number of items copied
                    unsafe { *len = out_sz };
                    SUCCESS
                }
            } else {
                // One of len or result_buffer is NULL. Same handling for both
                TPSError::NullPointer.into()
            }
        }
        Err(e) => e.into(),
    }
}

/// C language API for calling [`open_session`].
///
/// # Parameters
///
/// - `connection_id`: The connection ID previously returned by [`c_connect`].
///
/// # Safety
///
/// - The memory area pointed to by `session_id` must be writable by callee.
///
/// `service_instance` and `session_id` are checked to be non-NULL before use.
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c_open_session(
    connection_id: u32,
    service_instance: *const UUID,
    session_id: *mut u32,
) -> u32 {
    if !service_instance.is_null() && !session_id.is_null() {
        let svc_uuid: &UUID = unsafe { &*service_instance };
        match service::open_session(connection_id, svc_uuid) {
            Ok(session) => {
                unsafe { *session_id = session };
                SUCCESS
            }
            Err(e) => e.into(),
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// C language API for calling [`close_session`].
///
/// # Safety
///
/// There are no safety issues with this function.
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c_close_session(session_id: u32) -> u32 {
    match service::close_session(session_id) {
        Ok(()) => SUCCESS,
        Err(e) => e.into(),
    }
}

/// C callable API for performing a transaction with the [`execute_transaction`] function.
///
/// # Parameters
///
/// - `session_id`: The session ID previously returned by [`c_open_session`].
/// - `recv_len`: On entry, the capacity (in bytes) of `recv_buf`. On successful return, updated
///   with the number of bytes actually written to `recv_buf`.
///
/// # Safety
///
/// - `send_buf` must point to a readable memory area of at least length `send_len` bytes.
/// - `recv_buf` must point to a writable memory area of at least length `*recv_len` bytes.
/// - `recv_len` and `transaction_id` must be writable.
///
/// In general, all pointers must be properly aligned for the target architecture. All pointer
/// values are checked to ensure that they are non-NULL before use.
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c_execute_transaction(
    session_id: u32,
    send_buf: *const u8,
    send_len: usize,
    recv_buf: *mut u8,
    recv_len: *mut usize,
    transaction_id: *mut u32,
) -> u32 {
    if !send_buf.is_null() && !recv_buf.is_null() && !recv_len.is_null() && !transaction_id.is_null()
    {
        let send_slice = unsafe { from_raw_parts(send_buf, send_len) };
        let recv_slice = unsafe { from_raw_parts_mut(recv_buf, *recv_len) };
        match execute_transaction(session_id, send_slice, recv_slice) {
            Ok((t_id, out_len)) => {
                unsafe {
                    *transaction_id = t_id;
                    *recv_len = out_len;
                }
                SUCCESS
            }
            Err(e) => {
                if let TPSError::ShortBuffer(needed) = e {
                    unsafe { *recv_len = needed };
                }
                e.into()
            }
        }
    } else {
        ERROR_NULL_POINTER
    }
}

/// C callable API for transaction cancellation using the [`cancel_transaction`] function.
///
/// # Safety
///
/// There are no safety considerations with this function.
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c_cancel_transaction(transaction_id: u32) -> u32 {
    match service::cancel_transaction(transaction_id) {
        Ok(()) => SUCCESS,
        Err(e) => e.into(),
    }
}
