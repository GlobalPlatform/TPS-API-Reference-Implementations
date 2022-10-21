/***************************************************************************************************
 * Copyright (c) 2022 Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all copies or
 * substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 **************************************************************************************************/
extern crate getrandom;
extern crate tps_client_common;
extern crate tps_error;

use tps_client_common::c_structs::*;

/** The Connector structure is exposed by every instance of a connector, and defines the function
 * calls between the TPS Client API and the connector implementation.
 */
#[repr(C)]
#[derive(Debug)]
pub struct Connector {
    pub connect: unsafe extern "C" fn(
        connection_method: u32,
        connection_data: *const ConnectionData,
        connection_id: *mut u32,
    ) -> u32,
    pub disconnect: unsafe extern "C" fn(connection_id: u32) -> u32,
    pub service_discovery:
        unsafe extern "C" fn(result_buf: *mut ServiceIdentifier, len: *mut usize) -> u32,
    pub open_session:
        unsafe extern "C" fn(service_instance: *const UUID, session_id: *mut u32) -> u32,
    pub close_session: unsafe extern "C" fn(session_id: u32) -> u32,
    pub execute_transaction: unsafe extern "C" fn(
        send_buf: *const u8,
        send_len: usize,
        recv_buf: *mut u8,
        recv_len: usize,
        transaction_id: *mut u32,
    ) -> u32,
    pub cancel_transaction: unsafe extern "C" fn(transaction_id: u32) -> u32,
}

// This is the only callable public API exported from the connector
//
// The returned [Connector] reference cannot be NULL as it is statically defined and compiler.
//
// Individual functions to which the Connector provides references may have their own memory safety
// requirements as they are also C callable. See [CONNECTOR] documentation.
extern "C" {
    pub fn TPSC_GetConnectorAPI() -> *const Connector;
}
