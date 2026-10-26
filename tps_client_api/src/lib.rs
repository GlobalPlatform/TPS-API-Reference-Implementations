/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![no_std]

//! A reference implementation of the GlobalPlatform Trusted Platform Services (TPS) Client API
//! (GPP TPS Client API Specification v1.0): the normative Rust API surface for discovering TPS
//! Services, opening sessions with them over a [`tps_connector::Connector`], and exchanging
//! request/response messages within a session.

// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set.
#[cfg(any(feature = "std", test))]
extern crate std;

// If we are really building no_std, pull in core as well. It is aliased as std so that "use"
// statements are always the same
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

extern crate tps_client_common;
extern crate tps_connector;
extern crate tps_error;

// `rot13_connector` provides the `TPSC_GetConnectorAPI` symbol that `services::CONNECTORS`
// requires at link time. It is otherwise unused directly by this crate's own code, so it must be
// referenced explicitly to avoid being dropped by the linker when running tests.
#[cfg(test)]
extern crate rot13_connector;

mod connector;
mod message_buf;
mod services;
mod sync;

use std::ptr::{slice_from_raw_parts, slice_from_raw_parts_mut};
use tps_client_common::c_structs::{
    ConnectionData, ServiceIdentifier, ServiceSelector, ServiceVersion, Session, UUID,
};
use tps_error::TPSError;

pub use message_buf::MessageBuf;


/***************************************************************************************************
 * Debug tracing support under `trace` feature
 **************************************************************************************************/
use crate::services::find_service;
#[cfg(feature = "trace")]
use func_trace::trace;
#[cfg(feature = "trace")]
use std::println;

use tps_client_common::c_priv::SessionPriv;
use tps_client_common::c_uuid::UUID_NIL;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/***************************************************************************************************
 * Rust Language API
 **************************************************************************************************/
/// The function requests the cancellation of a pending open session operation or Transaction
/// invocation operation. As this is a synchronous API, this function must be called from a
/// thread other than the one executing the TPSC_SessionOpen or TPSC_Transaction function.
///
/// **NB:** This reference implementation is single-threaded and synchronous throughout, so there
/// is no in-progress call on another thread to cancel by the time this function can run. It is
/// provided so that a Connector which does support genuine concurrent cancellation can be plugged
/// in without further changes to this layer.
#[cfg_attr(feature = "trace", trace)]
pub fn cancel_transaction(transaction: &MessageBuf) -> Result<(), TPSError> {
    match transaction.imp.transaction() {
        Some((transaction_id, service_id)) => {
            let result = match unsafe { service_id.as_ref() } {
                Some(service_id) => {
                    if let Some(connector) = find_service(service_id) {
                        connector::cancel_transaction(connector, transaction_id)
                    } else {
                        Err(TPSError::BadState)
                    }
                }
                None => Err(TPSError::BadState),
            };
            // Check whether the guard has been corrupted, mirroring the check performed by
            // `close_session` on `SessionPriv`.
            if transaction.imp.check() {
                result
            } else {
                Err(TPSError::BadState)
            }
        }
        None => Err(TPSError::BadState),
    }
}

/// The function closes a session that has been opened with a TPS Service.
#[cfg_attr(feature = "trace", trace)]
pub fn close_session(session: &Session) -> Result<(), TPSError> {
    let connection_id = session.imp.into_inner();
    if let Some(connector) = find_service(session.service_id) {
        connector::close_session(connector, session.session_id)?;
        // It is now safe to close the connection associated with this session.
        connector::disconnect(connector, connection_id)?;
        // Check whether the guard has been corrupted. We have done our best to clean up, but
        // higher-level error handling is probably needed.
        if session.imp.check() {
            Ok(())
        } else {
            Err(TPSError::BadState)
        }
    } else {
        Err(TPSError::CommunicationError)
    }
}

/// The function sends a request message and receives a response message within the specified
/// session.
///
/// Returns `TPSError::BadState` if `send_buffer.size` exceeds `send_buffer.maxsize` — this guard
/// prevents building an oversized slice over `send_buffer.message` that would read past its
/// backing allocation.
///
/// # Recv-side note (Stage 3 Finding B, tps_client_c)
///
/// `recv_buffer.maxsize` is trusted as the receive slice's length: `connector::execute_transaction`
/// already rejects a connector report of `recv_len > recv_buffer.maxsize` (an `TPSError::RuntimeError`,
/// closing the "connector writes/reports past capacity" direction), and the `debug_assert!` below
/// re-checks that same invariant defensively at the point `recv_buffer.size` is written. Neither
/// catches `recv_buffer.maxsize` itself exceeding the real size of the allocation backing
/// `recv_buffer.message` — that value is supplied by the caller at `TPSC_InitializeTransaction`
/// time and never re-verifiable afterward (see that function's `# Safety` doc), so an inflated
/// `maxsize` remains an accepted risk, mitigated only by documentation, not by any runtime check.
#[cfg_attr(feature = "trace", trace)]
pub fn execute_transaction(
    session: &Session,
    send_buffer: &MessageBuf,
    recv_buffer: &mut MessageBuf,
) -> Result<(), TPSError> {
    if send_buffer.size > send_buffer.maxsize {
        return Err(TPSError::BadState);
    }
    if let Some(connector) = find_service(session.service_id) {
        let send = unsafe { &*slice_from_raw_parts(send_buffer.message, send_buffer.size) };
        let recv = unsafe {
            &mut *slice_from_raw_parts_mut(recv_buffer.message, recv_buffer.maxsize)
        };
        let (transaction_id, recv_len) =
            connector::execute_transaction(connector, session.session_id, send, recv)?;
        debug_assert!(
            recv_len <= recv_buffer.maxsize,
            "connector::execute_transaction is documented to reject recv_len > capacity"
        );
        recv_buffer.size = recv_len;
        recv_buffer.imp.set_transaction(transaction_id, session.service_id);
        Ok(())
    } else {
        Err(TPSError::CommunicationError)
    }
}

/// The function opens a new session between the TPS Client and the TPS Service identified by the
/// service structure.
///
/// `session` borrows `uuid` for its `'a` lifetime: the resulting `Session<'a>`'s `service_id`
/// field points at the same `UUID` the caller passed in, so the borrow checker enforces that
/// `uuid` outlives the session rather than relying on caller discipline alone.
#[cfg_attr(feature = "trace", trace)]
pub fn open_session<'a>(
    uuid: &'a UUID,
    connection_method: u32,
    connection_data: Option<&ConnectionData>,
    session: &mut Session<'a>,
) -> Result<(), TPSError> {
    // look up the Connector associated with `uuid`
    if let Some(connector) = services::find_service(uuid) {
        let connection_id = connector::connect(connector, connection_method, connection_data)?;
        let session_id = connector::open_session(connector, connection_id, uuid)?;
        *session = Session {
            service_id: uuid,
            session_id,
            imp: SessionPriv::new(connection_id),
        };
        Ok(())
    } else {
        Err(TPSError::CommunicationError)
    }
}

/// The function discovers all TPS Services available via the TPS Client API that match the selector
/// method.
#[cfg_attr(feature = "trace", trace)]
pub fn service_discovery(
    service_selector: &ServiceSelector,
    service_ids_array: &mut [ServiceIdentifier],
) -> Result<usize, TPSError> {
    // Const fn provides a handy way to initialize services_array
    const fn empty_id() -> ServiceIdentifier {
        ServiceIdentifier {
            service_instance: UUID_NIL,
            service_id: UUID_NIL,
            secure_component_type: UUID_NIL,
            secure_component_instance: UUID_NIL,
            service_version: ServiceVersion {
                major_version: 0,
                minor_version: 0,
                patch_version: 0,
            },
        }
    }
    let mut services_array: [ServiceIdentifier; 10] = [
        empty_id(),
        empty_id(),
        empty_id(),
        empty_id(),
        empty_id(),
        empty_id(),
        empty_id(),
        empty_id(),
        empty_id(),
        empty_id(),
    ];
    // Get a list of all of the services available on the platform
    let found_services = services::populate_services_array(&mut services_array)?;
    // Iterate over the services to place those matching in [`service_array`].
    let matched_services = services::select_matched_services(
        &services_array[..found_services],
        service_selector,
        service_ids_array,
    )?;
    Ok(matched_services)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fix pin (Stage 1 Finding 2, tps_client_api). Spec §4.5.3 (`TPSC_CloseSession`) lists
    /// `TPSC_ERROR_COMMUNICATION: No instance of the Connector to which this session refers can
    /// be found` as the documented return for exactly this situation. `open_session` (line ~170)
    /// and `execute_transaction` (via its own `find_service` miss, mapped to
    /// `TPSError::CommunicationError`) both already use `TPSError::CommunicationError` for their
    /// analogous "connector not found" branch; `close_session`'s equivalent branch now matches.
    /// Uses `UUID_NIL` as a service id that is guaranteed to never be registered in the
    /// process-global `services::SERVICES` table (this unit-test binary never calls
    /// `service_discovery`), which is exactly what drives `find_service` to return `None` and
    /// hit this branch.
    #[test]
    fn close_session_unknown_connector_returns_communication_error() {
        let session = Session {
            service_id: &UUID_NIL,
            session_id: 0,
            imp: SessionPriv::new(0),
        };
        assert!(matches!(
            close_session(&session),
            Err(TPSError::CommunicationError)
        ));
    }

    /// Stage 3 Finding 1 (tps_client_api, security review). `execute_transaction` built the
    /// `send` slice from the caller-reported `send_buffer.size` with no check against the
    /// backing buffer's real capacity (`send_buffer.maxsize`), so a caller (or a corrupted
    /// `MessageBuffer`) reporting `size > maxsize` caused an out-of-bounds read from a
    /// `slice_from_raw_parts` built over the too-small backing allocation. Checked before the
    /// connector lookup, so this rejects even for a service id with no registered connector.
    #[test]
    fn execute_transaction_send_size_exceeding_maxsize_is_rejected() {
        let mut backing = [0u8; 8];
        let mut send_buf = crate::MessageBuf::new(&mut backing);
        send_buf.size = 1_000_000;
        let mut recv_backing = [0u8; 8];
        let mut recv_buf = crate::MessageBuf::new(&mut recv_backing);
        let session = Session {
            service_id: &UUID_NIL,
            session_id: 0,
            imp: SessionPriv::new(0),
        };
        assert!(matches!(
            execute_transaction(&session, &send_buf, &mut recv_buf),
            Err(TPSError::BadState)
        ));
    }

    /// `tps_client_common` Stage 3 Finding 2 (fixed). `cancel_transaction` used to
    /// `.unwrap()` the raw `service_id` pointer stored in the transaction record, which panicked
    /// if that pointer was NULL or dangling instead of returning an error. Records a transaction
    /// against a NULL service id (which a corrupted or malformed `MessageBuf` could carry) and
    /// asserts a graceful `Err`, not a panic.
    #[test]
    fn cancel_transaction_null_service_id_returns_bad_state_instead_of_panicking() {
        let mut backing = [0u8; 8];
        let mut buf = crate::MessageBuf::new(&mut backing);
        buf.imp.set_transaction(1, core::ptr::null());
        assert!(matches!(cancel_transaction(&buf), Err(TPSError::BadState)));
    }
}
