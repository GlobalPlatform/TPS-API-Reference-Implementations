/***************************************************************************************************
 * Copyright (c) 2022, Qualcomm Innovation Center, Inc. All rights reserved.
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
func_trace::init_depth_var!();

/***************************************************************************************************
 * Connector using C language API
 **************************************************************************************************/

/// Connect to a connector instance
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn connect(
    instance: &Connector,
    connection_method: u32,
    connection_data: Option<&ConnectionData>,
) -> Result<u32, TPSError> {
    let mut connection_id: u32 = 0;
    let connect_fn = instance.connect;
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

/// Disconnect from a connector instance
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn disconnect(instance: &Connector, connection_id: u32) -> Result<(), TPSError> {
    let disconnect_fn = instance.disconnect;
    let c_retval = unsafe { disconnect_fn(connection_id) };
    from_c_error_code(c_retval, None)
}

/// Perform Service Discovery.
///
/// This is static for a given connector instance, so no need for Connector ID
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn service_discovery(
    instance: &Connector,
    services: &mut [ServiceIdentifier],
) -> Result<usize, TPSError> {
    let discover_fn = instance.service_discovery;
    let mut no_svcs = services.len();
    // no_svcs holds either the number of items copied (on success) or the number of items
    // we would like to copy (on failure)
    let c_retval = unsafe { discover_fn(services.as_mut_ptr(), &mut no_svcs) };
    from_c_error_code(c_retval, Some(no_svcs)).map(|_| no_svcs)
}

/// Open a session
///
/// Returns session id on success
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn open_session(instance: &Connector, service_instance: &UUID) -> Result<u32, TPSError> {
    let open_fn = instance.open_session;
    let mut session_id: u32 = 0;
    let c_retval = unsafe { open_fn(service_instance, &mut session_id) };
    match from_c_error_code(c_retval, None) {
        Ok(()) => Ok(session_id),
        Err(e) => Err(e),
    }
}

/// Close the session with a given session ID
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn close_session(instance: &Connector, session_id: u32) -> Result<(), TPSError> {
    let close_fn = instance.close_session;
    let c_retval = unsafe { close_fn(session_id) };
    from_c_error_code(c_retval, None)
}

/// Execute a transaction
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn execute_transaction(
    instance: &Connector,
    in_buf: &[u8],
    out_buf: &mut [u8],
) -> Result<u32, TPSError> {
    let execute_fn = instance.execute_transaction;
    let mut transaction_id: u32 = 0;
    let c_retval = unsafe {
        // TODO: Does this properly handle the short buffer case? Should it?
        execute_fn(
            in_buf.as_ptr(),
            in_buf.len(),
            out_buf.as_mut_ptr(),
            out_buf.len(),
            &mut transaction_id,
        )
    };
    match from_c_error_code(c_retval, None) {
        Ok(()) => Ok(transaction_id),
        Err(e) => Err(e),
    }
}

/// Cancel a transaction
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn cancel_transaction(
    instance: &Connector,
    transaction_id: u32,
) -> Result<(), TPSError> {
    let cancel_fn = instance.cancel_transaction;
    let c_retval = unsafe { cancel_fn(transaction_id) };
    from_c_error_code(c_retval, None)
}
