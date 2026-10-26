/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![no_std]

//! `tps_connector::Connector` implementation backed by Tock IPC ([`tock_ipc`]).
//!
//! Structured like `rot13_connector` (a `c_api` C-ABI boundary over an internal `service`
//! module), except `service::execute_transaction` crosses a real Tock process boundary via
//! `tock_ipc::call` instead of calling a service's message handler directly in-process.

extern crate rot13_service;
extern crate tps_client_common;
extern crate tps_connector;

use tps_connector::Connector;

mod c_api;
mod service;

const CONNECTOR: Connector = Connector {
    connect: c_api::c_connect,
    disconnect: c_api::c_disconnect,
    discover_services: c_api::c_discover_services,
    open_session: c_api::c_open_session,
    close_session: c_api::c_close_session,
    execute_transaction: c_api::c_execute_transaction,
    cancel_transaction: c_api::c_cancel_transaction,
};

/// # Safety
///
/// The returned reference is never NULL, as `CONNECTOR` is a statically defined `const`.
#[no_mangle]
pub unsafe extern "C" fn TPSC_GetConnectorAPI() -> *const Connector {
    &CONNECTOR
}
