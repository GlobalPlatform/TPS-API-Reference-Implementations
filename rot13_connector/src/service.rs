/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/

//! This file contains a minimal example of the connector implementation for a TPS Service, in this
//! case the ROT13 service.
//!
//! It is intended merely as a starting point for a realistic implementation as it supports only a
//! very minimal feature set (single connection, single session, service is, in practice, running
//! in the same memory space as everything else.

extern crate getrandom;

extern crate rot13_service;
extern crate tps_client_common;
extern crate tps_connector;

use rot13_service::{
    message_handler, GPP_ROT13_SERVICE_NAME, GPP_ROT13_SERVICE_VERSION, GPP_TEST_SC_TYPE,
    Rot13Error,
};
use tps_client_common::c_login::{
    LOGIN_APPLICATION, LOGIN_GROUP, LOGIN_GROUP_APPLICATION, LOGIN_PUBLIC, LOGIN_USER,
    LOGIN_USER_APPLICATION,
};
use tps_client_common::c_structs::{ConnectionData, ServiceIdentifier, UUID};
use tps_error::*;
use tps_minicbor::EncodeError;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Secure Component instance ID.
///
/// > In a real implementation, this identity would usually be based on a cryptographically random
/// > value that is defined for the lifetime of the Secure Component. The mechanism to do this is
/// > dependent on the Secure Component implementation and out of scope of this reference
/// > implementation. See your Secure Component vendor.
///
/// The value used here is a randomly generated UUIDv4. You should not use this for anything
/// other than test purposes.
const SECURE_COMPONENT_INSTANCE: [u8; 16] = [
    0x79, 0x62, 0xa2, 0xc1, 0xfe, 0xe3, 0x4d, 0x3e, 0x89, 0x55, 0xd5, 0x97, 0x8d, 0x78, 0x12, 0xf4,
];

/// Service Instance ID
///
/// The value used here is a UUIDv5 generated using namespace value of `SECURE_COMPONENT_INSTANCE`
/// and the name `GPP ROT13`.
const SERVICE_INSTANCE_ID: [u8; 16] = [
    0x26, 0x70, 0x70, 0xe8, 0xd7, 0x66, 0x5a, 0x16, 0x97, 0x6b, 0xf9, 0x21, 0x86, 0xb9, 0xf6, 0x9e,
];

/// Define the set of services supported by this Secure Component.
///
/// In this sample code, the set of services provided is fixed. Real Secure Components may require
/// the set of available services to be determined dynamically, where they support installation
/// and removal of new services.
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

/// Atomic which tracks whether we are connected, in a thread-safe way.
static IS_CONNECTED: AtomicBool = AtomicBool::new(false);
/// Atomic which holds the session ID and whether the (single) session is active
static SESSION_INFO: AtomicU32 = AtomicU32::new(0);
/// Atomic which holds transaction ID
static TRANSACTION_ID: AtomicU32 = AtomicU32::new(0);

/// Connect to a Secure Component
///
/// The connect function performs any operations that need to be performed before calling the
/// `service_discovery` function or the `open_session` function.
///
/// Some Secure Components don't require anything to be done here beyond generating a unique
/// connection ID. Others, e.g. TEE Client API (call to TEEC_InitializeContect) or Open Mobile API
/// (construct an SEService instance and ensure it is connected) may require something to be done
/// at this point.
///
/// > In this sample code, the connection ID is always 1, and we return an error on attempts to
/// > create more than one connection. `IS_CONNECTED` is updated with a single
/// > `compare_exchange`, so two concurrent callers can never both observe "not connected" and
/// > both proceed to claim connection ID 1 (which a separate load-then-store would allow).
pub(crate) fn connect(
    connection_method: u32,
    _connection_data: Option<&ConnectionData>,
) -> Result<u32, TPSError> {
    if connection_method == LOGIN_PUBLIC
        || connection_method == LOGIN_USER
        || connection_method == LOGIN_GROUP
        || connection_method == LOGIN_APPLICATION
        || connection_method == LOGIN_USER_APPLICATION
        || connection_method == LOGIN_GROUP_APPLICATION
    {
        match IS_CONNECTED.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire) {
            // Nothing connected yet; always returns connection ID = 1
            Ok(_) => Ok(1),
            // There is already another connection
            Err(_) => Err(TPSError::Busy),
        }
    } else {
        Err(TPSError::BadIdentifier)
    }
}

/// Disconnect from a Secure Component
///
/// We check whether there is an active connection and then close it, via a single
/// `compare_exchange` so a concurrent `disconnect` call cannot also observe "connected" and race
/// to close a connection this call already closed.
///
/// Some Secure Components may require additional processing to clean-up.
pub(crate) fn disconnect(_connection_id: u32) -> Result<(), TPSError> {
    match IS_CONNECTED.compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Ok(()),
        // There is no active connection
        Err(_) => Err(TPSError::BadState),
    }
}

/// Service Discovery for a Secure Component.
///
/// This function returns a list of the available TPS Services. This is a very simple implementation
/// in which a single, statically defined service is returned. The service is pre-encoded in a
/// static memory block - an approach that can work for several services if they happen not to be
/// updatable.
///
/// Secure Components on which services are installable and updatable will likely require a more
/// sophisticated implementation.
pub(crate) fn discover_services(_connection_id: u32) -> Result<&'static [ServiceIdentifier], TPSError> {
    Ok(SERVICES)
}

/// Open a session.
///
/// This implementation supports only a single session, for simplicity. It does return an
/// unpredictable session ID, which is desirable, and this example shows a very simple
/// implementation of such an approach using the `getrandom` crate which retries data from the
/// system entropy source.
///
/// The active session information is stored in the atomic variable `SESSION_INFO`. 0 is used to
/// indicate no active session.
///
/// An active session is a random integer between 1 and u32::MAX, with a distribution
/// dependent on the system entropy source plus a slight peak at 42, in recognition of Douglas
/// Adams.
///
/// The transition from "no session" (0) to "reserved" (`u32::MAX`, a sentinel distinct from every
/// value this function can hand out as a real session id — see the `session_id == u32::MAX` check
/// below) is a single `compare_exchange`, so two concurrent callers cannot both observe "no
/// session" and both go on to mint and store their own session id.
pub(crate) fn open_session(_connection_id: u32, _service_instance: &UUID) -> Result<u32, TPSError> {
    if SESSION_INFO
        .compare_exchange(0, u32::MAX, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // There is an active session (or another caller is concurrently opening one). Since we
        // support only one session, this is an error.
        return Err(TPSError::Busy);
    }
    // We now exclusively hold the "reserved" state: no other caller can be in this function
    // concurrently until we store a final value below.
    // `getrandom::u32` rather than filling a 4-byte buffer: getrandom 0.2's free `getrandom()`
    // function was renamed to `fill()` in 0.3, and `u32()` is the purpose-built accessor for
    // exactly this case, so the intermediate byte array and its `from_ne_bytes` decode are no
    // longer needed.
    match getrandom::u32() {
        Ok(value) => {
            let mut session_id = value;
            // There is a small probability that we have zero or u32::MAX as session_id, and we
            // are using those values as sentinels, so we will hard-code a solution to this.
            if session_id == 0 || session_id == u32::MAX {
                session_id = 42;
            }
            SESSION_INFO.store(session_id, Ordering::Release);
            Ok(session_id)
        }
        Err(_) => {
            // Release the reservation so a subsequent call is not permanently locked out.
            SESSION_INFO.store(0, Ordering::Release);
            Err(TPSError::GenericError)
        }
    }
}

/// Close a session.
///
/// This implementation supports only a single session, with session information stored in
/// the atomic variable `SESSION_INFO`.
///
/// The value 0 indicates no active session. Any other value is the active session_id. The check
/// and the transition back to 0 are a single `compare_exchange` on the expected `session_id`, so
/// a concurrent `close_session`/`open_session` call cannot race with this one.
pub(crate) fn close_session(session_id: u32) -> Result<(), TPSError> {
    if session_id == 0 || session_id == u32::MAX {
        return Err(TPSError::BadState);
    }
    match SESSION_INFO.compare_exchange(session_id, 0, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Ok(()),
        // There is no active session with this id. Since we support only one session, this is
        // an error.
        Err(_) => Err(TPSError::BadState),
    }
}

/// Execute a transaction.
///
/// `session_id` is validated against the single active session tracked in `SESSION_INFO`; a
/// mismatch (including no active session) is reported as `TPSError::BadState`. Returns the
/// transaction ID and the number of bytes written to `out_buf`.
pub(crate) fn execute_transaction(
    session_id: u32,
    in_buf: &[u8],
    out_buf: &mut [u8],
) -> Result<(u32, usize), TPSError> {
    let current_session = SESSION_INFO.load(Ordering::Acquire);
    if current_session == 0 || current_session != session_id {
        return Err(TPSError::BadState);
    }
    match message_handler(in_buf, out_buf) {
        Ok(recv_len) => {
            // `fetch_add` makes the read-modify-write a single atomic step, so two concurrent
            // transactions can never be assigned the same id (which a separate load-then-store
            // would allow).
            let transaction_id = TRANSACTION_ID.fetch_add(1, Ordering::AcqRel) + 1;
            Ok((transaction_id, recv_len))
        }
        Err(Rot13Error::Encode(EncodeError::OutOfSpace(needed))) => {
            Err(TPSError::ShortBuffer(needed))
        }
        Err(_) => Err(TPSError::GenericError),
    }
}

/// Cancel a transaction
///
/// This sample implementation does not support transaction cancellation.
pub(crate) fn cancel_transaction(_transaction_id: u32) -> Result<(), TPSError> {
    Err(TPSError::NotSupported)
}

/// Stage 3 security review regression tests (rot13_connector). `connect`/`disconnect`/
/// `open_session`/`close_session` used to check-then-act on `IS_CONNECTED`/`SESSION_INFO` via a
/// separate `load` followed by a separate `store`, which is not atomic as a whole: two concurrent
/// callers could both observe "not connected"/"no session" and both proceed, corrupting the
/// single-connection/single-session invariant this module's doc comments claim to hold. Fixed by
/// using `compare_exchange`/`fetch_add` so each check-and-transition is one atomic step.
///
/// All tests here share the same process-global `IS_CONNECTED`/`SESSION_INFO`/`TRANSACTION_ID`
/// statics, and Cargo runs `#[test]`s in a crate concurrently by default, so every test serializes
/// against `TEST_GUARD` (mirroring the pattern already used for the same reason in
/// `tps_client_api/tests/rot13_integration.rs`).
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_GUARD: Mutex<()> = Mutex::new(());

    /// Leaves `IS_CONNECTED`/`SESSION_INFO`/`TRANSACTION_ID` at their initial ("nothing active")
    /// values so each test starts from a known state regardless of what a previous test (or a
    /// panic mid-test) left behind.
    fn reset_state() {
        IS_CONNECTED.store(false, Ordering::Release);
        SESSION_INFO.store(0, Ordering::Release);
        TRANSACTION_ID.store(0, Ordering::Release);
    }

    #[test]
    fn connect_disconnect_round_trip() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        assert!(matches!(connect(LOGIN_PUBLIC, None), Ok(1)));
        assert!(matches!(disconnect(1), Ok(())));
    }

    #[test]
    fn connect_twice_without_disconnect_is_busy() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        assert!(matches!(connect(LOGIN_PUBLIC, None), Ok(1)));
        assert!(matches!(connect(LOGIN_PUBLIC, None), Err(TPSError::Busy)));
        disconnect(1).unwrap();
    }

    #[test]
    fn disconnect_without_connect_is_bad_state() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        assert!(matches!(disconnect(1), Err(TPSError::BadState)));
    }

    #[test]
    fn connect_rejects_unrecognized_login_method() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        assert!(matches!(
            connect(0xFFFF_FFFF, None),
            Err(TPSError::BadIdentifier)
        ));
    }

    #[test]
    fn open_close_session_round_trip() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        let uuid = UUID { bytes: [0; 16] };
        let session_id = open_session(1, &uuid).expect("open_session");
        assert_ne!(session_id, 0);
        assert_ne!(session_id, u32::MAX);
        assert!(matches!(close_session(session_id), Ok(())));
    }

    #[test]
    fn open_session_twice_without_close_is_busy() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        let uuid = UUID { bytes: [0; 16] };
        let session_id = open_session(1, &uuid).expect("open_session");
        assert!(matches!(open_session(1, &uuid), Err(TPSError::Busy)));
        close_session(session_id).unwrap();
    }

    #[test]
    fn close_session_rejects_mismatched_id() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        let uuid = UUID { bytes: [0; 16] };
        let session_id = open_session(1, &uuid).expect("open_session");
        assert!(matches!(
            close_session(session_id.wrapping_add(1)),
            Err(TPSError::BadState)
        ));
        close_session(session_id).unwrap();
    }

    #[test]
    fn close_session_rejects_sentinel_values() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        assert!(matches!(close_session(0), Err(TPSError::BadState)));
        assert!(matches!(close_session(u32::MAX), Err(TPSError::BadState)));
    }

    #[test]
    fn execute_transaction_rejects_unknown_session() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        let mut out = [0u8; 16];
        assert!(matches!(
            execute_transaction(1, &[], &mut out),
            Err(TPSError::BadState)
        ));
    }

    #[test]
    fn execute_transaction_assigns_increasing_ids() {
        let _guard = TEST_GUARD.lock().unwrap();
        reset_state();
        let uuid = UUID { bytes: [0; 16] };
        let session_id = open_session(1, &uuid).expect("open_session");
        // `TPS_GetFeatures_Req` (#6.1, empty map) is the simplest message `message_handler`
        // accepts; only the resulting transaction id sequencing is under test here.
        let mut req_buf = [0u8; 16];
        let req_len = {
            let mut encoder = tps_minicbor::EncoderStd::new(&mut req_buf);
            let msg = encoder.tag(1u64).unwrap().map().unwrap();
            msg.end().unwrap();
            encoder.encoded().len()
        };
        let mut out = [0u8; 100];
        let (first_id, _) =
            execute_transaction(session_id, &req_buf[..req_len], &mut out).expect("transaction 1");
        let (second_id, _) =
            execute_transaction(session_id, &req_buf[..req_len], &mut out).expect("transaction 2");
        assert_eq!(second_id, first_id + 1);
        close_session(session_id).unwrap();
    }
}
