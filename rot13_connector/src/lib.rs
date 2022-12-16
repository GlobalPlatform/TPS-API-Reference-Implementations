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
// Default to no use of standard library
#![no_std]

/// rot13_connector
///
/// A minimal sample implementation of a TPS Client API Connector.
///
/// This implementation is designed for use in constrained systems and requires neither the Rust
/// standard library nor an allocator.

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

use tps_connector::Connector;

mod c_api;
mod service;

/// In this implementation we export a static struct instance with the required function
/// pointers. This is a reasonable solution for many embedded RTOS targets.
///
/// See also:
///
/// - [c_connect], connect to the Secure Component managed via a [Connector]
/// - [c_disconnect], disconnect from the Secure Component
/// - [c_service_discovery], determine what services are offered by a Secure Component
/// - [c_open_session], open a session to a particular service
/// - [c_close_session], close a session with a particular service
/// - [c_execute_transaction], send a message to the service identified by a session and receive a
///   response
/// - [c_cancel_transaction], cancel a pending transaction (not supported by this implementation)
///
/// # Safety
///
/// See the documentation for the individual functions.
const CONNECTOR: Connector = Connector {
    connect: c_api::c_connect,
    disconnect: c_api::c_disconnect,
    service_discovery: c_api::c_service_discovery,
    open_session: c_api::c_open_session,
    close_session: c_api::c_close_session,
    execute_transaction: c_api::c_execute_transaction,
    cancel_transaction: c_api::c_cancel_transaction,
};

/// This is the only callable public API exported from the connector
///
/// # Safety
///
/// The returned [Connector] reference cannot be NULL as it is statically defined and compiler.
///
/// Individual functions to which the Connector provides references may have their own memory safety
/// requirements as they are also C callable. See [CONNECTOR] documentation.
#[no_mangle]
pub unsafe extern "C" fn TPSC_GetConnectorAPI() -> *const Connector {
    &CONNECTOR
}
