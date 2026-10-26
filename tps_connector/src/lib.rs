/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![no_std]

//! The TPS Connector ABI: the C-callable function-pointer table (`Connector`, spec §5.4.1
//! `TPSCC_Connector`) that every TPS Connector implementation exposes to the TPS Client API, and
//! `TPSC_GetConnectorAPI`, the sole entry point used to retrieve it.

#[cfg(any(feature = "std", test))]
extern crate std;
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

extern crate tps_client_common;
extern crate tps_error;

use tps_client_common::c_structs::*;

/** The Connector structure is exposed by every instance of a connector, and defines the function
 * calls between the TPS Client API and the connector implementation (spec §5.4.1
 * `TPSCC_Connector`).
 *
 * STAGE 3 FINDING (tps_connector #2): every field is `Option`-wrapped rather than a bare
 * function pointer. A malformed or partially/zero-initialized third-party C connector (e.g. one
 * built via `memset`) can leave these NULL; a bare `unsafe extern "C" fn` field can't represent
 * that, so calling through it would jump to address 0 (UB). Wrapping in `Option` lets callers
 * null-check before invoking. This is an intentional ABI break: cbindgen emits the generated C
 * header's fields as nullable function pointers.
 */
#[repr(C)]
#[derive(Debug)]
pub struct Connector {
    /// Establishes a connection to a Secure Component (spec §5.4.1.3). `None` indicates a
    /// malformed/zero-initialized connector, not that the operation is unsupported.
    pub connect: Option<
        unsafe extern "C" fn(
            connection_method: u32,
            connection_data: *const ConnectionData,
            connection_id: *mut u32,
        ) -> u32,
    >,
    /// Closes a connection previously opened by `connect` (spec §5.4.1.4). `None` indicates a
    /// malformed/zero-initialized connector, not that the operation is unsupported.
    pub disconnect: Option<unsafe extern "C" fn(connection_id: u32) -> u32>,
    /// Lists the TPS Services available over a connection (spec §5.4.1.5). `None` indicates a
    /// malformed/zero-initialized connector, not that the operation is unsupported.
    pub discover_services: Option<
        unsafe extern "C" fn(
            connection_id: u32,
            result_buf: *mut ServiceIdentifier,
            len: *mut usize,
        ) -> u32,
    >,
    /// Opens a session with a TPS Service instance (spec §5.4.1.7). `None` indicates a
    /// malformed/zero-initialized connector, not that the operation is unsupported.
    pub open_session: Option<
        unsafe extern "C" fn(
            connection_id: u32,
            service_instance: *const UUID,
            session_id: *mut u32,
        ) -> u32,
    >,
    /// Closes a session previously opened by `open_session` (spec §5.4.1.2). `None` indicates a
    /// malformed/zero-initialized connector, not that the operation is unsupported.
    pub close_session: Option<unsafe extern "C" fn(session_id: u32) -> u32>,
    /// Sends a request message and receives the response over a session (spec §5.4.1.6). `None`
    /// indicates a malformed/zero-initialized connector, not that the operation is unsupported.
    pub execute_transaction: Option<
        unsafe extern "C" fn(
            session_id: u32,
            send_buf: *const u8,
            send_len: usize,
            recv_buf: *mut u8,
            recv_len: *mut usize,
            transaction_id: *mut u32,
        ) -> u32,
    >,
    /// Cancels an in-flight transaction started by `execute_transaction` (spec §5.4.1.1). `None`
    /// indicates a malformed/zero-initialized connector, not that the operation is unsupported.
    pub cancel_transaction: Option<unsafe extern "C" fn(transaction_id: u32) -> u32>,
}

unsafe extern "C" {
    /// This is the only callable public API exported from the connector.
    ///
    /// The returned [`Connector`] pointer is not guaranteed to be non-NULL — callers (e.g.
    /// `tps_client_api::services`) NULL-check it via `as_ref()` before dereferencing.
    ///
    /// Individual functions to which the Connector provides references may have their own memory
    /// safety requirements as they are also C callable. See [`Connector`]'s documentation.
    pub fn TPSC_GetConnectorAPI() -> *const Connector;
}

/***************************************************************************************************
 * Regression tests
 *
 * Stage 1 spec-compliance audit (against GPP_TPS_Client_API_Spec_v1.0, §5.4.1 `TPSCC_Connector`
 * and §5.4.1.1-§5.4.1.7 per-function subsections). See CODE_REVIEW_CAMPAIGN.md for the full
 * findings list. These tests pin the repr(C) layout and function-pointer signatures of
 * [`Connector`] and the no-argument `TPSC_GetConnectorAPI` extern contract, since both are
 * easily-misread FFI-boundary areas: a reordering or signature drift here would silently break
 * ABI compatibility with connector implementations built against `tpsc_connector_api.h` without
 * necessarily failing to compile on the Rust side (e.g. two adjacent `u32`/`*mut u32` fields
 * swapped would still type-check).
 **************************************************************************************************/
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;
    use core::ptr;
    use tps_client_common::c_uuid::UUID_NIL;

    unsafe extern "C" fn stub_connect(
        _connection_method: u32,
        _connection_data: *const ConnectionData,
        connection_id: *mut u32,
    ) -> u32 {
        if !connection_id.is_null() {
            unsafe {
                *connection_id = 42;
            }
        }
        0
    }
    unsafe extern "C" fn stub_disconnect(_connection_id: u32) -> u32 {
        0
    }
    unsafe extern "C" fn stub_discover_services(
        _connection_id: u32,
        _result_buf: *mut ServiceIdentifier,
        len: *mut usize,
    ) -> u32 {
        if !len.is_null() {
            unsafe {
                *len = 0;
            }
        }
        0
    }
    unsafe extern "C" fn stub_open_session(
        _connection_id: u32,
        _service_instance: *const UUID,
        session_id: *mut u32,
    ) -> u32 {
        if !session_id.is_null() {
            unsafe {
                *session_id = 7;
            }
        }
        0
    }
    unsafe extern "C" fn stub_close_session(_session_id: u32) -> u32 {
        0
    }
    unsafe extern "C" fn stub_execute_transaction(
        _session_id: u32,
        _send_buf: *const u8,
        _send_len: usize,
        _recv_buf: *mut u8,
        recv_len: *mut usize,
        transaction_id: *mut u32,
    ) -> u32 {
        if !recv_len.is_null() {
            unsafe {
                *recv_len = 0;
            }
        }
        if !transaction_id.is_null() {
            unsafe {
                *transaction_id = 99;
            }
        }
        0
    }
    unsafe extern "C" fn stub_cancel_transaction(_transaction_id: u32) -> u32 {
        0
    }

    fn stub_connector() -> Connector {
        Connector {
            connect: Some(stub_connect),
            disconnect: Some(stub_disconnect),
            discover_services: Some(stub_discover_services),
            open_session: Some(stub_open_session),
            close_session: Some(stub_close_session),
            execute_transaction: Some(stub_execute_transaction),
            cancel_transaction: Some(stub_cancel_transaction),
        }
    }

    /// STAGE 3 FINDING (tps_connector #2) regression test: a `Connector` with a `None`
    /// fn-pointer field (as a zero-initialized/malformed third-party connector would produce)
    /// must be detectable via `Option::is_none()`/`ok_or` rather than being callable at all —
    /// proving the null hazard is now representable and checkable instead of silently jumping to
    /// address 0 through a bare non-nullable fn pointer.
    #[test]
    fn connector_with_null_execute_transaction_is_detectable_before_call() {
        let mut connector = stub_connector();
        connector.execute_transaction = None;
        assert!(connector.execute_transaction.is_none());
        assert!(connector.connect.is_some());
    }

    /// Pin the `#[repr(C)]` field declaration order of [`Connector`] against spec §5.4.1's
    /// `TPSCC_Connector` struct definition: connect, disconnect, discover_services,
    /// open_session, close_session, execute_transaction, cancel_transaction (in that order).
    /// Because this struct crosses the FFI boundary, byte-offset order (which for a `repr(C)`
    /// struct with no other attributes tracks declaration order) must match what
    /// `tpsc_connector_api.h` documents. A silent field reorder would break ABI compatibility
    /// with any connector built against the generated C header while still compiling cleanly on
    /// the Rust side.
    #[test]
    fn connector_field_offsets_match_spec_5_4_1_declaration_order() {
        let connect_off = offset_of!(Connector, connect);
        let disconnect_off = offset_of!(Connector, disconnect);
        let discover_off = offset_of!(Connector, discover_services);
        let open_off = offset_of!(Connector, open_session);
        let close_off = offset_of!(Connector, close_session);
        let exec_off = offset_of!(Connector, execute_transaction);
        let cancel_off = offset_of!(Connector, cancel_transaction);

        assert!(connect_off < disconnect_off, "connect must precede disconnect");
        assert!(
            disconnect_off < discover_off,
            "disconnect must precede discover_services"
        );
        assert!(
            discover_off < open_off,
            "discover_services must precede open_session"
        );
        assert!(open_off < close_off, "open_session must precede close_session");
        assert!(
            close_off < exec_off,
            "close_session must precede execute_transaction"
        );
        assert!(
            exec_off < cancel_off,
            "execute_transaction must precede cancel_transaction"
        );
    }

    /// Pin `size_of::<Connector>()` to exactly seven pointer-sized function pointers with no
    /// hidden padding or extra fields — defends against an accidental non-fn-pointer field being
    /// added that would change the C ABI layout without an obvious compile error.
    #[test]
    fn connector_size_matches_seven_function_pointers() {
        assert_eq!(
            core::mem::size_of::<Connector>(),
            7 * core::mem::size_of::<usize>()
        );
    }

    /// Exercise every function-pointer field with a stub matching the exact parameter
    /// types/order/return type mandated by spec §5.4.1 (`TPSCC_Connector`) and its per-function
    /// subsections (§5.4.1.1-§5.4.1.7). If any field's signature silently drifted (e.g. a
    /// parameter reordered, or an out-pointer like `recv_len`/`len`/`session_id`/
    /// `connection_id`/`transaction_id` changed to a plain by-value parameter), this test would
    /// fail to compile — the strongest guarantee available for a `repr(C)` function-pointer
    /// struct.
    #[test]
    fn connector_function_pointer_signatures_match_spec() {
        let connector = stub_connector();

        let mut connection_id: u32 = 0;
        let rc = unsafe { (connector.connect.unwrap())(0, ptr::null(), &mut connection_id) };
        assert_eq!(rc, 0);
        assert_eq!(connection_id, 42);

        let rc = unsafe { (connector.disconnect.unwrap())(connection_id) };
        assert_eq!(rc, 0);

        let mut len: usize = 0;
        let rc = unsafe {
            (connector.discover_services.unwrap())(connection_id, ptr::null_mut(), &mut len)
        };
        assert_eq!(rc, 0);

        let mut session_id: u32 = 0;
        let svc = UUID_NIL;
        let rc =
            unsafe { (connector.open_session.unwrap())(connection_id, &svc, &mut session_id) };
        assert_eq!(rc, 0);
        assert_eq!(session_id, 7);

        let rc = unsafe { (connector.close_session.unwrap())(session_id) };
        assert_eq!(rc, 0);

        let send_buf = [0u8; 4];
        let mut recv_buf = [0u8; 4];
        let mut recv_len: usize = recv_buf.len();
        let mut transaction_id: u32 = 0;
        let rc = unsafe {
            (connector.execute_transaction.unwrap())(
                session_id,
                send_buf.as_ptr(),
                send_buf.len(),
                recv_buf.as_mut_ptr(),
                &mut recv_len,
                &mut transaction_id,
            )
        };
        assert_eq!(rc, 0);
        assert_eq!(transaction_id, 99);

        let rc = unsafe { (connector.cancel_transaction.unwrap())(transaction_id) };
        assert_eq!(rc, 0);
    }

    /// `TPSC_GetConnectorAPI` is the crate's sole `extern "C"` import: a no-argument function
    /// returning `*const Connector`. Provide a test-local definition satisfying the extern
    /// declaration (mirroring the pattern every real connector crate, e.g. `rot13_connector`,
    /// uses) so this test binary links, and confirm the round trip through the declared
    /// signature behaves as expected — a non-null pointer to a `Connector` whose function
    /// pointers are callable.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn TPSC_GetConnectorAPI() -> *const Connector {
        static CONNECTOR: Connector = Connector {
            connect: Some(stub_connect),
            disconnect: Some(stub_disconnect),
            discover_services: Some(stub_discover_services),
            open_session: Some(stub_open_session),
            close_session: Some(stub_close_session),
            execute_transaction: Some(stub_execute_transaction),
            cancel_transaction: Some(stub_cancel_transaction),
        };
        &CONNECTOR
    }

    #[test]
    fn get_connector_api_returns_non_null_callable_connector() {
        let connector_ptr = unsafe { TPSC_GetConnectorAPI() };
        assert!(!connector_ptr.is_null());
        let connector = unsafe { &*connector_ptr };
        let mut connection_id = 0u32;
        let rc = unsafe { (connector.connect.unwrap())(0, ptr::null(), &mut connection_id) };
        assert_eq!(rc, 0);
        assert_eq!(connection_id, 42);
    }
}
