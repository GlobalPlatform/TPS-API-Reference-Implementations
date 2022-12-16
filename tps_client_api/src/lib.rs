/***************************************************************************************************
 * Copyright (c) 2021-2022, Qualcomm Innovation Center, Inc. All rights reserved.
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
#![no_std]

// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set.
#[cfg(any(feature = "trace", test))]
extern crate std;

// If we are really building no_std, pull in core as well. It is aliased as std so that "use"
// statements are always the same
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

extern crate tps_client_common;
extern crate tps_connector;
extern crate tps_error;

mod connector;
mod services;

use std::ptr::{slice_from_raw_parts, slice_from_raw_parts_mut};
use tps_client_common::c_structs::{
    ConnectionData, MessageBuffer, ServiceIdentifier, ServiceSelector, ServiceVersion, Session,
    UUID,
};
use tps_error::TPSError;

/***************************************************************************************************
 * Debug tracing support under `trace` feature
 **************************************************************************************************/
use crate::services::find_service;
#[cfg(feature = "trace")]
use func_trace::trace;

use tps_client_common::c_priv::{MessageBufferPriv, SessionPriv};
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
/// **NB:** Cancellation not supported in first release.
#[cfg_attr(feature = "trace", trace)]
pub fn cancel_transaction(_transaction: &mut MessageBuffer) -> Result<(), TPSError> {
    Err(TPSError::NotImplemented)
}

/// This function clears the data in a TPSC_Transaction instance.
/// Callers may wish to clear the contents of a TPSC_Transaction for several reasons:
///
/// - To ensure that the transaction is cleared to a known state before it is re-used.
/// - To ensure that sensitive information is cleared from memory as soon as it is no-longer needed.
/// - To ensure that information does not remain in memory after the transaction has been finalized.
#[cfg_attr(feature = "trace", trace)]
pub fn clear_transaction(_transaction: &MessageBuffer) -> Result<(), TPSError> {
    Err(TPSError::NotImplemented)
}

/// The function closes a session that has been opened with a TPS Service.
#[cfg_attr(feature = "trace", trace)]
pub fn close_session(session: &Session) -> Result<(), TPSError> {
    // TODO: Below lookup of service_id should not fail
    let service_id = unsafe { session.service_id.as_ref() }.unwrap();
    let connection_id = session.imp.into_inner();
    if let Some(connector) = find_service(service_id) {
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
        Err(TPSError::BadState)
    }
}

/// The function sends a request message and receives a response message within the specified
/// session.
#[cfg_attr(feature = "trace", trace)]
pub fn execute_transaction(
    session: &Session,
    send_buffer: &MessageBuffer,
    recv_buffer: &mut MessageBuffer,
) -> Result<(), TPSError> {
    // TODO: fallible, and should not be
    let service_id = unsafe { session.service_id.as_ref() }.unwrap();
    if let Some(connector) = find_service(service_id) {
        let send = unsafe { &*slice_from_raw_parts(send_buffer.message, send_buffer.size) };
        let recv = unsafe {
            &mut *slice_from_raw_parts_mut((*recv_buffer).message, (*recv_buffer).maxsize)
        };
        connector::execute_transaction(connector, send, recv)?;
        recv_buffer.size = recv.len();
        Ok(())
    } else {
        Err(TPSError::CommunicationError)
    }
}

/// The function finalizes a transaction structure that has been initialized and associated with
/// the session structure.
#[cfg_attr(feature = "trace", trace)]
pub fn finalize_transaction(transaction: &mut MessageBuffer) -> Result<(), TPSError> {
    // Sanitize buffer, reset message size
    unsafe {
        transaction.message.write_bytes(0, transaction.maxsize);
    }
    transaction.size = 0;
    Ok(())
}

/// The function initializes a transaction structure for use in TPSC_Transaction function. The
/// transaction structure may be used multiple times with the TPSC_Transaction function.
#[cfg_attr(feature = "trace", trace)]
pub fn initialize_transaction(
    transaction: &mut MessageBuffer,
    buffer: &mut [u8],
) -> Result<(), TPSError> {
    *transaction = MessageBuffer {
        message: buffer.as_mut_ptr(),
        size: 0,
        maxsize: buffer.len(),
        imp: MessageBufferPriv::new(),
    };
    Ok(())
}

/// The function opens a new session between the TPS Client and the TPS Service identified by the
/// service structure.
#[cfg_attr(feature = "trace", trace)]
pub fn open_session(
    uuid: &UUID,
    connection_method: u32,
    connection_data: Option<&ConnectionData>,
    session: &mut Session,
) -> Result<(), TPSError> {
    // look up the Connector associated with `uuid`
    if let Some(connector) = services::find_service(uuid) {
        let connection_id = connector::connect(connector, connection_method, connection_data)?;
        let session_id = connector::open_session(connector, uuid)?;
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
