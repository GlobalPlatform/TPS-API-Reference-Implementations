/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/

//! End-to-end integration test exercising `tps_client_api`'s Rust API against the `rot13_connector`
//! / `rot13_service` pair: service discovery -> open_session -> execute_transaction (encrypt then
//! decrypt) -> close_session, plus short-buffer handling for both `service_discovery` and
//! `execute_transaction`.

extern crate rot13_connector;
extern crate tps_client_api;
extern crate tps_client_common;
extern crate tps_error;
extern crate tps_minicbor;

use rot13_service::{
    GPP_ROT13_CIPHERTEXT_KEY, GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_ENCRYPT_RSP, GPP_ROT13_PLAINTEXT_KEY,
    GPP_ROT13_SERVICE_NAME,
};
use std::sync::Mutex;
use tps_client_api::{close_session, execute_transaction, open_session, service_discovery, MessageBuf};
use tps_client_common::c_login::LOGIN_PUBLIC;
use tps_client_common::c_structs::{
    ServiceIdentifier, ServiceRange, ServiceSelector, ServiceVersion, Session,
};
use tps_client_common::c_uuid::UUID_NIL;
use tps_error::TPSError;
use tps_minicbor::{Decoder, EncoderStd, Item, Map};

/// `rot13_connector` supports only a single connection/session at a time, tracked in
/// process-global atomics. Cargo runs tests in this file concurrently on separate threads by
/// default, so all tests must serialize against this guard to avoid spurious `Busy` errors.
static TEST_GUARD: Mutex<()> = Mutex::new(());

/// A `ServiceSelector` that matches any service (all fields wildcarded, no version constraints).
fn wildcard_selector() -> ServiceSelector {
    ServiceSelector {
        service_id: UUID_NIL,
        secure_component_type: UUID_NIL,
        secure_component_instance: UUID_NIL,
        service_version_range: ServiceRange {
            lowest_acceptable_version: tps_client_common::c_structs::ServiceBound::NoBounds,
            first_excluded_version: tps_client_common::c_structs::ServiceBound::NoBounds,
            last_excluded_version: tps_client_common::c_structs::ServiceBound::NoBounds,
            highest_acceptable_version: tps_client_common::c_structs::ServiceBound::NoBounds,
        },
    }
}

fn empty_id() -> ServiceIdentifier {
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

/// Build a `#6.<msg_id>({ 1 => text })` request into `buf`, returning the length of the encoded
/// message. Mirrors `rot13_service/tests/rot13_test.rs::build_req`.
fn build_req(buf: &mut [u8], msg_id: u32, key: u32, text: &str) -> usize {
    let mut encoder = EncoderStd::new(buf);
    let mut msg = encoder.tag(msg_id.into()).unwrap().map().unwrap();
    msg.insert_key_value(key, text).unwrap();
    msg.end().unwrap();
    encoder.encoded().len()
}

/// Decode a `#6.<msg_id>({ ... })` response, returning the response body map.
fn decode_rsp(buf: &[u8], expected_msg_id: u32) -> Map<'_> {
    let tagged = match Decoder::new(buf).next().expect("decode response") {
        Item::Tag(tagged) => tagged,
        item => panic!("expected tagged response, got {item:?}"),
    };
    assert_eq!(tagged.tag, expected_msg_id as u64);
    match tagged.item().expect("decode tagged body") {
        Item::Map(map) => map,
        item => panic!("expected map body, got {item:?}"),
    }
}

fn map_get<'a>(map: &Map<'a>, key: u64) -> Option<Item<'a>> {
    map.iter()
        .find_map(|pair| pair.ok().filter(|(k, _)| matches!(k, Item::Unsigned(v) if *v == key)))
        .map(|(_, v)| v)
}

/// Discover the ROT13 service, open a session against it, run `f` with the open session, then
/// close the session. Everything stays within one stack frame so that `Session::service_id` (a
/// raw pointer, not a borrow-checked reference) never outlives the `ServiceIdentifier` it points
/// into — returning either of them by value out of this function would risk a dangling pointer
/// once the move relocates that memory.
fn with_rot13_session<R>(f: impl FnOnce(&Session) -> R) -> R {
    let _guard = TEST_GUARD.lock().unwrap();

    let selector = wildcard_selector();
    let mut discovered: [ServiceIdentifier; 10] = core::array::from_fn(|_| empty_id());
    let found = service_discovery(&selector, &mut discovered).expect("service_discovery");
    assert_eq!(found, 1, "expected exactly the ROT13 service to be discovered");
    let rot13 = &discovered[0];
    assert_eq!(rot13.service_id.bytes, GPP_ROT13_SERVICE_NAME);

    let mut session = Session {
        service_id: &rot13.service_instance,
        session_id: 0,
        imp: tps_client_common::c_priv::SessionPriv::new(0),
    };
    open_session(&rot13.service_instance, LOGIN_PUBLIC, None, &mut session).expect("open_session");

    let result = f(&session);

    close_session(&session).expect("close_session");
    result
}

#[test]
fn discover_open_execute_close_round_trip() {
    with_rot13_session(|session| {
        let plaintext = "thequickbrownfoxjumpsoverthelazydog";
        let ciphertext = "gurdhvpxoebjasbkwhzcfbiregurynmlqbt";

        let mut send_storage = [0u8; 128];
        let send_len = build_req(
            &mut send_storage,
            GPP_ROT13_ENCRYPT_REQ,
            GPP_ROT13_PLAINTEXT_KEY,
            plaintext,
        );
        let mut send_buf = MessageBuf::new(&mut send_storage);
        send_buf.size = send_len;

        let mut recv_storage = [0u8; 128];
        let mut recv_buf = MessageBuf::new(&mut recv_storage);

        execute_transaction(session, &send_buf, &mut recv_buf)
            .expect("execute_transaction (encrypt)");

        let received = unsafe { std::slice::from_raw_parts(recv_buf.message, recv_buf.size) };
        let map = decode_rsp(received, GPP_ROT13_ENCRYPT_RSP);
        match map_get(&map, GPP_ROT13_CIPHERTEXT_KEY as u64) {
            Some(Item::Text(text)) => assert_eq!(text, ciphertext),
            other => panic!("unexpected response body: {other:?}"),
        }
    });
}

#[test]
fn service_discovery_short_buffer_reports_required_count() {
    let _guard = TEST_GUARD.lock().unwrap();

    let selector = wildcard_selector();
    // Buffer of length 0 can't hold the one matching service.
    let mut discovered: [ServiceIdentifier; 0] = [];
    match service_discovery(&selector, &mut discovered) {
        Err(TPSError::ShortBuffer(needed)) => assert_eq!(needed, 1),
        other => panic!("expected ShortBuffer(1), got {other:?}"),
    }
}

#[test]
fn execute_transaction_short_buffer_reports_exact_needed_bytes() {
    let plaintext = "thequickbrownfoxjumpsoverthelazydog";

    // First, run the request into a generously sized buffer to learn the exact response size
    // the service will produce.
    let full_len = with_rot13_session(|session| {
        let mut send_storage = [0u8; 128];
        let send_len = build_req(
            &mut send_storage,
            GPP_ROT13_ENCRYPT_REQ,
            GPP_ROT13_PLAINTEXT_KEY,
            plaintext,
        );
        let mut send_buf = MessageBuf::new(&mut send_storage);
        send_buf.size = send_len;

        let mut full_recv_storage = [0u8; 128];
        let mut full_recv_buf = MessageBuf::new(&mut full_recv_storage);
        execute_transaction(session, &send_buf, &mut full_recv_buf)
            .expect("execute_transaction with adequate buffer");
        full_recv_buf.size
    });

    // Re-run the same request (in a fresh session — the sample connector only supports one
    // session at a time) against a receive buffer one byte too small.
    with_rot13_session(|session| {
        let mut send_storage = [0u8; 128];
        let send_len = build_req(
            &mut send_storage,
            GPP_ROT13_ENCRYPT_REQ,
            GPP_ROT13_PLAINTEXT_KEY,
            plaintext,
        );
        let mut send_buf = MessageBuf::new(&mut send_storage);
        send_buf.size = send_len;

        let mut short_storage = vec![0u8; full_len - 1];
        let mut short_recv_buf = MessageBuf::new(&mut short_storage);

        match execute_transaction(session, &send_buf, &mut short_recv_buf) {
            Err(TPSError::ShortBuffer(needed)) => assert_eq!(needed, full_len),
            other => panic!("expected ShortBuffer({full_len}), got {other:?}"),
        }
    });
}
