/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Internal (non-C-ABI) logic backing this crate's [`crate::CONNECTOR`], generic over the
//! Tock target ([`libtock_runtime::TockSyscalls`] on-device — this crate always builds for a
//! real Tock process, so unlike `tock_ipc` there is no host-test double to also satisfy).
//!
//! Mirrors `rot13_connector::service`'s structure (same `SERVICES` table shape, same
//! `AtomicU32`-based state tracking), but `connect`/`open_session`/`execute_transaction` cross a
//! real Tock IPC boundary via [`tock_ipc`] instead of calling `rot13_service` directly.

use core::sync::atomic::{AtomicU32, Ordering};

use libtock_runtime::TockSyscalls;
use rot13_service::{GPP_ROT13_SERVICE_NAME, GPP_ROT13_SERVICE_VERSION, GPP_TEST_SC_TYPE};
use tps_client_common::c_login::{
    LOGIN_APPLICATION, LOGIN_GROUP, LOGIN_GROUP_APPLICATION, LOGIN_PUBLIC, LOGIN_USER,
    LOGIN_USER_APPLICATION,
};
use tps_client_common::c_structs::{ConnectionData, ServiceIdentifier, UUID};
use tps_error::TPSError;
use tps_minicbor::Decoder;

/// Secure Component instance ID.
///
/// As with `rot13_connector::service::SECURE_COMPONENT_INSTANCE`, a real implementation would
/// derive this from the Secure Component itself. This is a fixed, randomly generated UUIDv4 for
/// test purposes only.
const SECURE_COMPONENT_INSTANCE: [u8; 16] = [
    0x4a, 0x1e, 0x3f, 0x2b, 0x9c, 0x77, 0x4a, 0x0d, 0x8e, 0x21, 0x6b, 0x4d, 0x3a, 0x5f, 0x91, 0xc6,
];

/// Service Instance ID
///
/// UUIDv5 generated using namespace value of `SECURE_COMPONENT_INSTANCE` and the name
/// `GPP ROT13`, matching `rot13_connector::service::SERVICE_INSTANCE_ID`'s derivation (this is a
/// distinct value since the namespace differs).
const SERVICE_INSTANCE_ID: [u8; 16] = [
    0x8d, 0x6c, 0x2a, 0x51, 0x0f, 0x94, 0x53, 0x2d, 0xa1, 0x6e, 0xc3, 0x8b, 0x4f, 0x27, 0x5d, 0x99,
];

/// The literal Tock package name `tock_rot13_service` is built and packaged under, used for
/// `tock_ipc::discover`. This is the sole entry in the UUID-to-process-name table described in
/// the Phase 6 plan: `discover_services` returns a `ServiceIdentifier` synthesized from
/// `rot13_service`'s own UUID/version constants, but the actual Tock-level lookup underneath is
/// by this name.
const ROT13_SERVICE_PROCESS_NAME: &[u8] = b"tock_rot13_service";

const SERVICES: &[ServiceIdentifier] = &[ServiceIdentifier {
    service_instance: UUID {
        bytes: SERVICE_INSTANCE_ID,
    },
    service_id: UUID {
        bytes: GPP_ROT13_SERVICE_NAME,
    },
    secure_component_type: UUID {
        bytes: GPP_TEST_SC_TYPE,
    },
    secure_component_instance: UUID {
        bytes: SECURE_COMPONENT_INSTANCE,
    },
    service_version: GPP_ROT13_SERVICE_VERSION,
}];

/// The discovered Tock process id for the connected service, or 0 if not connected.
/// `tock_ipc::ServiceId` round-trips through this via `as_raw`/`from_raw` (see
/// `tock_ipc::ServiceId::from_raw`'s doc comment) since atomics can't hold it directly.
static CONNECTED_SERVICE: AtomicU32 = AtomicU32::new(0);
/// The active session ID, or 0 if no session is open. This implementation supports only a single
/// session, as `rot13_connector::service` does.
static SESSION_INFO: AtomicU32 = AtomicU32::new(0);
/// Monotonically increasing transaction ID, matching `rot13_connector::service::TRANSACTION_ID`.
static TRANSACTION_ID: AtomicU32 = AtomicU32::new(0);

/// Fixed-size scratch buffer for the single shared Tock IPC buffer per the Phase 6 plan: the
/// request is copied in here, `tock_ipc::call` overwrites it in place with the reply, and the
/// reply's self-delimiting CBOR length (via `tps_minicbor::Decoder::pos`) determines how much of
/// it gets copied back out to the caller's `recv_buf`.
const IPC_BUFFER_SIZE: usize = 128;
static mut IPC_BUFFER: [u8; IPC_BUFFER_SIZE] = [0; IPC_BUFFER_SIZE];

pub(crate) fn connect(
    connection_method: u32,
    _connection_data: Option<&ConnectionData>,
) -> Result<u32, TPSError> {
    if connection_method != LOGIN_PUBLIC
        && connection_method != LOGIN_USER
        && connection_method != LOGIN_GROUP
        && connection_method != LOGIN_APPLICATION
        && connection_method != LOGIN_USER_APPLICATION
        && connection_method != LOGIN_GROUP_APPLICATION
    {
        return Err(TPSError::BadIdentifier);
    }
    if CONNECTED_SERVICE.load(Ordering::Acquire) != 0 {
        return Err(TPSError::Busy);
    }
    let service = tock_ipc::discover::<TockSyscalls>(ROT13_SERVICE_PROCESS_NAME)
        .map_err(|_| TPSError::CommunicationError)?;
    CONNECTED_SERVICE.store(service.as_raw(), Ordering::Release);
    // Always returns connection ID 1, matching rot13_connector::service::connect: this
    // implementation supports only a single connection.
    Ok(1)
}

pub(crate) fn disconnect(_connection_id: u32) -> Result<(), TPSError> {
    if CONNECTED_SERVICE.load(Ordering::Acquire) == 0 {
        return Err(TPSError::BadState);
    }
    CONNECTED_SERVICE.store(0, Ordering::Release);
    Ok(())
}

pub(crate) fn discover_services(
    _connection_id: u32,
) -> Result<&'static [ServiceIdentifier], TPSError> {
    Ok(SERVICES)
}

pub(crate) fn open_session(_connection_id: u32, _service_instance: &UUID) -> Result<u32, TPSError> {
    if CONNECTED_SERVICE.load(Ordering::Acquire) == 0 {
        return Err(TPSError::BadState);
    }
    if SESSION_INFO.load(Ordering::Acquire) != 0 {
        return Err(TPSError::Busy);
    }
    // A thin, validating pass-through: there is one real Tock-level handle (the discovered
    // ServiceId) underneath both the connection and the session, so the session ID just needs to
    // be a stable, non-zero value distinct from "no session" (0).
    const SESSION_ID: u32 = 1;
    SESSION_INFO.store(SESSION_ID, Ordering::Release);
    Ok(SESSION_ID)
}

pub(crate) fn close_session(session_id: u32) -> Result<(), TPSError> {
    let current_session = SESSION_INFO.load(Ordering::Acquire);
    if current_session != 0 && current_session == session_id {
        SESSION_INFO.store(0, Ordering::Release);
        Ok(())
    } else {
        Err(TPSError::BadState)
    }
}

pub(crate) fn execute_transaction(
    session_id: u32,
    in_buf: &[u8],
    out_buf: &mut [u8],
) -> Result<(u32, usize), TPSError> {
    let current_session = SESSION_INFO.load(Ordering::Acquire);
    if current_session == 0 || current_session != session_id {
        return Err(TPSError::BadState);
    }
    let raw_service = CONNECTED_SERVICE.load(Ordering::Acquire);
    if raw_service == 0 {
        return Err(TPSError::BadState);
    }
    if in_buf.len() > IPC_BUFFER_SIZE {
        return Err(TPSError::ShortBuffer(IPC_BUFFER_SIZE));
    }
    let service = tock_ipc::ServiceId::from_raw(raw_service);

    // Safety: this crate is built exclusively for single-threaded Tock userspace processes with
    // no preemptive concurrency within a process, and `execute_transaction` never re-enters
    // itself (there's no way to issue an overlapping IPC call from the same synchronous call
    // stack), so this static's exclusive access for the duration of one call is not contended.
    let buf: &mut [u8; IPC_BUFFER_SIZE] = unsafe { &mut *core::ptr::addr_of_mut!(IPC_BUFFER) };
    buf[..in_buf.len()].copy_from_slice(in_buf);

    tock_ipc::call::<TockSyscalls>(service, buf).map_err(|_| TPSError::CommunicationError)?;

    // Tock IPC's shared buffer carries no out-of-band length, and is overwritten in place, so
    // recover the actual reply length by decoding CBOR's self-delimiting top-level item and
    // reading back how many bytes that consumed.
    let mut decoder = Decoder::new(buf.as_slice());
    decoder.next().map_err(|_| TPSError::CommunicationError)?;
    let reply_len = decoder.pos();

    if reply_len > out_buf.len() {
        return Err(TPSError::ShortBuffer(reply_len));
    }
    out_buf[..reply_len].copy_from_slice(&buf[..reply_len]);

    let last_transaction = TRANSACTION_ID.load(Ordering::Acquire);
    TRANSACTION_ID.store(last_transaction + 1, Ordering::Release);
    Ok((last_transaction + 1, reply_len))
}

pub(crate) fn cancel_transaction(_transaction_id: u32) -> Result<(), TPSError> {
    Err(TPSError::NotSupported)
}
