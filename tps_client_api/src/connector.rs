/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/// Implementation of connector handling where there is a single connector instance.
///
/// The handling in this file is intended to support any implementation of the connector API, with
/// the limitation in this specific case being that we are using static linking, which allows only
/// a single instance of [[TPSC_GetConnectorAPI]] to exist. Using `dlopen()` and `dlsym()` on
/// systems with dynamic linking would allow a multi-connector implementation.
use std::ptr;

use tps_client_common::c_structs::{ConnectionData, ServiceIdentifier, UUID};
use tps_connector::Connector;
use tps_error::{from_c_error_code, TPSError};

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
 * Connector using C language API
 **************************************************************************************************/

/// Connect to a connector instance.
///
/// Returns `TPSError::NullPointer` if `instance.connect` is `None` (a malformed/zero-initialized
/// connector).
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn connect(
    instance: &Connector,
    connection_method: u32,
    connection_data: Option<&ConnectionData>,
) -> Result<u32, TPSError> {
    let mut connection_id: u32 = 0;
    let connect_fn = instance.connect.ok_or(TPSError::NullPointer)?;
    match connection_data {
        None => {
            let c_retval =
                unsafe { connect_fn(connection_method, ptr::null(), &mut connection_id) };
            from_c_error_code(c_retval, None)?;
            Ok(connection_id)
        }
        Some(conn_data) => {
            let c_retval = unsafe { connect_fn(connection_method, conn_data, &mut connection_id) };
            from_c_error_code(c_retval, None)?;
            Ok(connection_id)
        }
    }
}

/// Disconnect from a connector instance.
///
/// Returns `TPSError::NullPointer` if `instance.disconnect` is `None` (a malformed/zero-initialized
/// connector).
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn disconnect(instance: &Connector, connection_id: u32) -> Result<(), TPSError> {
    let disconnect_fn = instance.disconnect.ok_or(TPSError::NullPointer)?;
    let c_retval = unsafe { disconnect_fn(connection_id) };
    from_c_error_code(c_retval, None)
}

/// Perform Service Discovery on the connection identified by `connection_id`.
///
/// Returns `TPSError::NullPointer` if `instance.discover_services` is `None` (a
/// malformed/zero-initialized connector). Returns `TPSError::RuntimeError` if the connector
/// reports having written more items than `services`'s capacity — this would otherwise let a
/// malformed or malicious connector induce out-of-bounds indexing in the caller.
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn discover_services(
    instance: &Connector,
    connection_id: u32,
    services: &mut [ServiceIdentifier],
) -> Result<usize, TPSError> {
    let discover_fn = instance.discover_services.ok_or(TPSError::NullPointer)?;
    let capacity = services.len();
    let mut no_svcs = capacity;
    // no_svcs holds either the number of items copied (on success) or the number of items
    // we would like to copy (on failure)
    let c_retval = unsafe { discover_fn(connection_id, services.as_mut_ptr(), &mut no_svcs) };
    from_c_error_code(c_retval, Some(no_svcs))?;
    // The connector reports how many items it wrote into `services`; on success that count must
    // not exceed the capacity we gave it, or later indexing on the strength of this value would
    // read past `services`.
    if no_svcs > capacity {
        return Err(TPSError::RuntimeError);
    }
    Ok(no_svcs)
}

/// Open a session on the connection identified by `connection_id`.
///
/// Returns session id on success. Returns `TPSError::NullPointer` if `instance.open_session` is
/// `None` (a malformed/zero-initialized connector).
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn open_session(
    instance: &Connector,
    connection_id: u32,
    service_instance: &UUID,
) -> Result<u32, TPSError> {
    let open_fn = instance.open_session.ok_or(TPSError::NullPointer)?;
    let mut session_id: u32 = 0;
    let c_retval = unsafe { open_fn(connection_id, service_instance, &mut session_id) };
    match from_c_error_code(c_retval, None) {
        Ok(()) => Ok(session_id),
        Err(e) => Err(e),
    }
}

/// Close the session with a given session ID.
///
/// Returns `TPSError::NullPointer` if `instance.close_session` is `None` (a
/// malformed/zero-initialized connector).
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn close_session(instance: &Connector, session_id: u32) -> Result<(), TPSError> {
    let close_fn = instance.close_session.ok_or(TPSError::NullPointer)?;
    let c_retval = unsafe { close_fn(session_id) };
    from_c_error_code(c_retval, None)
}

/// Execute a transaction on the session identified by `session_id`.
///
/// Returns the transaction ID and the actual length of the response written into `out_buf` on
/// success. Returns `TPSError::NullPointer` if `instance.execute_transaction` is `None` (a
/// malformed/zero-initialized connector). Returns `TPSError::RuntimeError` if the connector
/// reports having written more bytes than `out_buf`'s capacity — this would otherwise let a
/// malformed or malicious connector induce out-of-bounds indexing in the caller.
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn execute_transaction(
    instance: &Connector,
    session_id: u32,
    in_buf: &[u8],
    out_buf: &mut [u8],
) -> Result<(u32, usize), TPSError> {
    let execute_fn = instance.execute_transaction.ok_or(TPSError::NullPointer)?;
    let mut transaction_id: u32 = 0;
    let capacity = out_buf.len();
    let mut recv_len = capacity;
    let c_retval = unsafe {
        execute_fn(
            session_id,
            in_buf.as_ptr(),
            in_buf.len(),
            out_buf.as_mut_ptr(),
            &mut recv_len,
            &mut transaction_id,
        )
    };
    from_c_error_code(c_retval, Some(recv_len))?;
    // The connector reports how many bytes it wrote into `out_buf`; on success that count must
    // not exceed the capacity we gave it, or the caller would be told a larger `size` than the
    // backing buffer actually holds.
    if recv_len > capacity {
        return Err(TPSError::RuntimeError);
    }
    Ok((transaction_id, recv_len))
}

/// Cancel a transaction.
///
/// Returns `TPSError::NullPointer` if `instance.cancel_transaction` is `None` (a
/// malformed/zero-initialized connector).
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn cancel_transaction(
    instance: &Connector,
    transaction_id: u32,
) -> Result<(), TPSError> {
    let cancel_fn = instance.cancel_transaction.ok_or(TPSError::NullPointer)?;
    let c_retval = unsafe { cancel_fn(transaction_id) };
    from_c_error_code(c_retval, None)
}

/***************************************************************************************************
 * Regression tests
 *
 * Stage 3 security review, Finding 2 (tps_client_api): the connector-reported item/byte counts
 * from `discover_services`/`execute_transaction` were trusted verbatim with no check against the
 * capacity actually handed to the connector. A malformed or malicious connector reporting a count
 * larger than the buffer it was given caused out-of-bounds indexing/state corruption downstream.
 * These tests use stub `Connector` instances that deliberately misreport an oversized count/length
 * to confirm the wrapper functions now reject that, rather than passing it through.
 **************************************************************************************************/
#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;
    use tps_client_common::c_uuid::UUID_NIL;

    fn empty_service_identifier() -> ServiceIdentifier {
        ServiceIdentifier {
            service_instance: UUID_NIL,
            service_id: UUID_NIL,
            secure_component_type: UUID_NIL,
            secure_component_instance: UUID_NIL,
            service_version: tps_client_common::c_structs::ServiceVersion {
                major_version: 0,
                minor_version: 0,
                patch_version: 0,
            },
        }
    }

    unsafe extern "C" fn stub_connect(
        _connection_method: u32,
        _connection_data: *const ConnectionData,
        _connection_id: *mut u32,
    ) -> u32 {
        0
    }
    unsafe extern "C" fn stub_disconnect(_connection_id: u32) -> u32 {
        0
    }
    unsafe extern "C" fn stub_discover_services_overreports(
        _connection_id: u32,
        _result_buf: *mut ServiceIdentifier,
        len: *mut usize,
    ) -> u32 {
        // Deliberately report far more items than the caller's buffer can hold.
        unsafe {
            *len = 1000;
        }
        0
    }
    unsafe extern "C" fn stub_open_session(
        _connection_id: u32,
        _service_instance: *const UUID,
        _session_id: *mut u32,
    ) -> u32 {
        0
    }
    unsafe extern "C" fn stub_close_session(_session_id: u32) -> u32 {
        0
    }
    unsafe extern "C" fn stub_execute_transaction_overreports(
        _session_id: u32,
        _send_buf: *const u8,
        _send_len: usize,
        _recv_buf: *mut u8,
        recv_len: *mut usize,
        _transaction_id: *mut u32,
    ) -> u32 {
        // Deliberately report far more bytes written than the caller's buffer can hold.
        unsafe {
            *recv_len = 10_000_000;
        }
        0
    }
    unsafe extern "C" fn stub_cancel_transaction(_transaction_id: u32) -> u32 {
        0
    }

    fn stub_connector(
        discover: unsafe extern "C" fn(u32, *mut ServiceIdentifier, *mut usize) -> u32,
        execute: unsafe extern "C" fn(u32, *const u8, usize, *mut u8, *mut usize, *mut u32) -> u32,
    ) -> Connector {
        Connector {
            connect: Some(stub_connect),
            disconnect: Some(stub_disconnect),
            discover_services: Some(discover),
            open_session: Some(stub_open_session),
            close_session: Some(stub_close_session),
            execute_transaction: Some(execute),
            cancel_transaction: Some(stub_cancel_transaction),
        }
    }

    #[test]
    fn discover_services_rejects_connector_reported_count_exceeding_capacity() {
        let connector = stub_connector(
            stub_discover_services_overreports,
            stub_execute_transaction_overreports,
        );
        let mut services: Vec<ServiceIdentifier> =
            (0..10).map(|_| empty_service_identifier()).collect();
        assert!(matches!(
            discover_services(&connector, 0, &mut services),
            Err(TPSError::RuntimeError)
        ));
    }

    #[test]
    fn execute_transaction_rejects_connector_reported_recv_len_exceeding_capacity() {
        let connector = stub_connector(
            stub_discover_services_overreports,
            stub_execute_transaction_overreports,
        );
        let mut out_buf = [0u8; 16];
        assert!(matches!(
            execute_transaction(&connector, 0, &[1, 2, 3], &mut out_buf),
            Err(TPSError::RuntimeError)
        ));
    }

    /// STAGE 3 FINDING (tps_connector #2) regression test, exercised from the tps_client_api call
    /// site: a `Connector` with a `None` fn-pointer field (as a malformed/zero-initialized
    /// third-party connector would produce) must be rejected with `TPSError::NullPointer` before
    /// any attempt to call through it, rather than jumping to address 0.
    #[test]
    fn execute_transaction_rejects_null_execute_transaction_fn() {
        let mut connector = stub_connector(
            stub_discover_services_overreports,
            stub_execute_transaction_overreports,
        );
        connector.execute_transaction = None;
        let mut out_buf = [0u8; 16];
        assert!(matches!(
            execute_transaction(&connector, 0, &[1, 2, 3], &mut out_buf),
            Err(TPSError::NullPointer)
        ));
    }
}
