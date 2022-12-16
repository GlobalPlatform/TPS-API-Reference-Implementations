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
};
use tps_client_common::c_login::{
    LOGIN_APPLICATION, LOGIN_GROUP, LOGIN_GROUP_APPLICATION, LOGIN_PUBLIC, LOGIN_USER,
    LOGIN_USER_APPLICATION,
};
use tps_client_common::c_structs::{ConnectionData, ServiceIdentifier, UUID};
use tps_error::*;

use getrandom::getrandom;
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
/// > create more than one connection. This s done in a way that should not cause race conditions
/// > as it uses atomics.
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
        if !IS_CONNECTED.load(Ordering::Acquire) {
            // Nothing connected yet
            IS_CONNECTED.store(true, Ordering::Release);
            // Always returns connection ID = 1
            Ok(1)
        } else {
            // There is already another connection
            Err(TPSError::Busy)
        }
    } else {
        Err(TPSError::BadIdentifier)
    }
}

/// Disconnect from a Secure Component
///
/// We check whether there is an active connection and then close it.
///
/// Some Secure Components may require additional processing to clean-up.
pub(crate) fn disconnect(_connection_id: u32) -> Result<(), TPSError> {
    if IS_CONNECTED.load(Ordering::Acquire) {
        // Disconnect
        IS_CONNECTED.store(false, Ordering::Release);
        Ok(())
    } else {
        // There is already another connection
        Err(TPSError::BadState)
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
pub(crate) fn service_discovery() -> Result<&'static [ServiceIdentifier], TPSError> {
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
pub(crate) fn open_session(_service_instance: &UUID) -> Result<u32, TPSError> {
    if SESSION_INFO.load(Ordering::Acquire) == 0 {
        // No session active
        let mut rnd_bytes: [u8; 4] = [0, 0, 0, 0];
        match getrandom(rnd_bytes.as_mut_slice()) {
            Ok(()) => {
                let mut session_id = u32::from_ne_bytes(rnd_bytes);
                // There is a small probability that we have zero as session_id, and we are using
                // 0 to indicate no session, so we will hard-code a solution to this.
                if session_id == 0 {
                    session_id += 42;
                }
                SESSION_INFO.store(session_id, Ordering::Release);
                Ok(session_id)
            }
            Err(_) => Err(TPSError::GenericError),
        }
    } else {
        // There is an active session. SInce we support only one session, this is an error
        Err(TPSError::Busy)
    }
}

/// Close a session.
///
/// This implementation supports only a single session, with session information stored in
/// the atomic variable `SESSION_INFO`.
///
/// The value 0 indicates no active session. Any other value is the active session_id.
pub(crate) fn close_session(session_id: u32) -> Result<(), TPSError> {
    let current_session = SESSION_INFO.load(Ordering::Acquire);
    if current_session != 0 && current_session == session_id {
        // Active session
        SESSION_INFO.store(0, Ordering::Release);
        Ok(())
    } else {
        // There is an active session. Since we support only one session, this is an error
        Err(TPSError::BadState)
    }
}

/// Execute a transaction.
///
/// This implementation does not support proper session handling.
///
/// TODO: Implement session handling
pub(crate) fn execute_transaction(in_buf: &[u8], out_buf: &mut [u8]) -> Result<u32, TPSError> {
    match message_handler(in_buf, out_buf) {
        Ok(()) => {
            let last_transaction = TRANSACTION_ID.load(Ordering::Acquire);
            TRANSACTION_ID.store(last_transaction + 1, Ordering::Release);
            Ok(last_transaction + 1)
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
