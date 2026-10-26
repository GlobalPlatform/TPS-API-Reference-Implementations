/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! C-ABI boundary layer, mirroring `rot13_connector::c_api`'s null-check / pointer-marshalling /
//! error-conversion conventions exactly, so that this crate's [`crate::CONNECTOR`] satisfies the
//! same [`tps_connector::Connector`] contract `tps_client_api::connector` calls into.

use core::slice::{from_raw_parts, from_raw_parts_mut};

use tps_client_common::c_errors::{ERROR_GENERIC, ERROR_NULL_POINTER, SUCCESS};
use tps_client_common::c_structs::{ConnectionData, ServiceIdentifier, UUID};
use tps_error::TPSError;

use crate::service;

/// # Safety
///
/// `connection_id` must be a valid, writable `u32`. `connection_data`, if non-null, must point
/// to a valid [`ConnectionData`].
#[no_mangle]
pub unsafe extern "C" fn c_connect(
    connection_method: u32,
    connection_data: *const ConnectionData,
    connection_id: *mut u32,
) -> u32 {
    if connection_id.is_null() {
        return ERROR_NULL_POINTER;
    }
    let connection_info = if connection_data.is_null() {
        None
    } else {
        Some(&*connection_data)
    };
    match service::connect(connection_method, connection_info) {
        Ok(conn_id) => {
            *connection_id = conn_id;
            SUCCESS
        }
        Err(e) => e.into(),
    }
}

/// # Safety
///
/// No specific memory-safety requirements.
#[no_mangle]
pub unsafe extern "C" fn c_disconnect(connection_id: u32) -> u32 {
    match service::disconnect(connection_id) {
        Ok(()) => SUCCESS,
        Err(e) => e.into(),
    }
}

/// # Safety
///
/// `result_buf` must be writable for at least `*len` instances of [`ServiceIdentifier`] when
/// non-null. `len` and `result_buf` are checked for null before use.
#[no_mangle]
pub unsafe extern "C" fn c_discover_services(
    connection_id: u32,
    result_buf: *mut ServiceIdentifier,
    len: *mut usize,
) -> u32 {
    if len.is_null() || result_buf.is_null() {
        return TPSError::NullPointer.into();
    }
    match service::discover_services(connection_id) {
        Ok(services) => {
            let out_sz = services.len();
            if out_sz > *len {
                *len = out_sz;
                TPSError::ShortBuffer(out_sz).into()
            } else {
                let dest = from_raw_parts_mut(result_buf, *len);
                dest[..out_sz].clone_from_slice(&services[..out_sz]);
                *len = out_sz;
                SUCCESS
            }
        }
        Err(e) => e.into(),
    }
}

/// # Safety
///
/// `service_instance` must point to a valid [`UUID`]; `session_id` must be writable. Both are
/// checked for null before use.
#[no_mangle]
pub unsafe extern "C" fn c_open_session(
    connection_id: u32,
    service_instance: *const UUID,
    session_id: *mut u32,
) -> u32 {
    if service_instance.is_null() || session_id.is_null() {
        return ERROR_NULL_POINTER;
    }
    let svc_uuid: &UUID = &*service_instance;
    match service::open_session(connection_id, svc_uuid) {
        Ok(session) => {
            *session_id = session;
            SUCCESS
        }
        Err(e) => e.into(),
    }
}

/// # Safety
///
/// No specific memory-safety requirements.
#[no_mangle]
pub unsafe extern "C" fn c_close_session(session_id: u32) -> u32 {
    match service::close_session(session_id) {
        Ok(()) => SUCCESS,
        Err(e) => e.into(),
    }
}

/// # Safety
///
/// `send_buf` must be readable for `send_len` bytes; `recv_buf` must be writable for `*recv_len`
/// bytes; `recv_len` and `transaction_id` must be writable. All pointers are checked for null
/// before use.
#[no_mangle]
pub unsafe extern "C" fn c_execute_transaction(
    session_id: u32,
    send_buf: *const u8,
    send_len: usize,
    recv_buf: *mut u8,
    recv_len: *mut usize,
    transaction_id: *mut u32,
) -> u32 {
    if send_buf.is_null() || recv_buf.is_null() || recv_len.is_null() || transaction_id.is_null() {
        return ERROR_NULL_POINTER;
    }
    let send_slice = from_raw_parts(send_buf, send_len);
    let recv_slice = from_raw_parts_mut(recv_buf, *recv_len);
    match service::execute_transaction(session_id, send_slice, recv_slice) {
        Ok((t_id, out_len)) => {
            *transaction_id = t_id;
            *recv_len = out_len;
            SUCCESS
        }
        Err(e) => {
            if let TPSError::ShortBuffer(needed) = e {
                *recv_len = needed;
            }
            e.into()
        }
    }
}

/// # Safety
///
/// No specific memory-safety requirements.
#[no_mangle]
pub unsafe extern "C" fn c_cancel_transaction(transaction_id: u32) -> u32 {
    match service::cancel_transaction(transaction_id) {
        Ok(()) => SUCCESS,
        Err(_) => ERROR_GENERIC,
    }
}
