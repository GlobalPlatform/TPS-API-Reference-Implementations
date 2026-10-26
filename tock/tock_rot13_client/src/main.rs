/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Tock userspace client app: exercises the unmodified `tps_client_api` surface
//! against `tock_rot13_service` over real Tock IPC, via `tock_connector`.
//!
//! Sends a fixed demo string through `service_discovery` -> `open_session` ->
//! `execute_transaction` (encrypt, then decrypt to verify round-trip) -> `close_session`,
//! printing a PASS/FAIL marker over the console — this is what the QEMU/Ibex automated test
//! (task #26) greps for in the captured serial output.
#![no_main]
#![no_std]

extern crate libtock_debug_panic;
extern crate tock_connector;

use core::fmt::Write;

use libtock_console::Console;
use libtock_runtime::{set_main, stack_size, TockSyscalls};
use rot13_service::{
    GPP_ROT13_CIPHERTEXT_KEY, GPP_ROT13_DECRYPT_REQ, GPP_ROT13_DECRYPT_RSP, GPP_ROT13_ENCRYPT_REQ,
    GPP_ROT13_ENCRYPT_RSP, GPP_ROT13_PLAINTEXT_KEY, GPP_ROT13_SERVICE_NAME,
};
use tps_client_api::{close_session, execute_transaction, open_session, service_discovery, MessageBuf};
use tps_client_common::c_login::LOGIN_PUBLIC;
use tps_client_common::c_priv::SessionPriv;
use tps_client_common::c_structs::{
    ServiceBound, ServiceIdentifier, ServiceRange, ServiceSelector, ServiceVersion, Session,
};
use tps_client_common::c_uuid::UUID_NIL;
use tps_minicbor::{Decoder, EncoderStd, Item, Map};

set_main! {main}
stack_size! {0x2000}

/// `tps_client_api`'s global connector/service registry is guarded by `critical_section`
/// (see `tps_client_api::sync`) rather than CAS atomics, so every no_std binary that links it
/// must supply an [`critical_section::Impl`]. A Tock process is unprivileged userspace with
/// cooperative (not preemptive) concurrency — code only "re-enters" at an explicit `yield`, never
/// asynchronously mid-instruction — so unlike a bare-metal Cortex-M/RISC-V `Impl`, there is
/// nothing to mask: this critical section is trivially always already exclusive.
struct TockCriticalSection;
critical_section::set_impl!(TockCriticalSection);

unsafe impl critical_section::Impl for TockCriticalSection {
    unsafe fn acquire() {}

    unsafe fn release(_restore_state: ()) {}
}

const PLAINTEXT: &str = "thequickbrownfoxjumpsoverthelazydog";

fn wildcard_selector() -> ServiceSelector {
    ServiceSelector {
        service_id: UUID_NIL,
        secure_component_type: UUID_NIL,
        secure_component_instance: UUID_NIL,
        service_version_range: ServiceRange {
            lowest_acceptable_version: ServiceBound::NoBounds,
            first_excluded_version: ServiceBound::NoBounds,
            last_excluded_version: ServiceBound::NoBounds,
            highest_acceptable_version: ServiceBound::NoBounds,
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

fn build_req(buf: &mut [u8], msg_id: u32, key: u32, text: &str) -> usize {
    let mut encoder = EncoderStd::new(buf);
    let mut msg = encoder.tag(msg_id.into()).unwrap().map().unwrap();
    msg.insert_key_value(key, text).unwrap();
    msg.end().unwrap();
    encoder.encoded().len()
}

fn decode_rsp(buf: &[u8], expected_msg_id: u32) -> Option<Map<'_>> {
    let tagged = match Decoder::new(buf).next().ok()? {
        Item::Tag(tagged) => tagged,
        _ => return None,
    };
    if tagged.tag != expected_msg_id as u64 {
        return None;
    }
    match tagged.item().ok()? {
        Item::Map(map) => Some(map),
        _ => None,
    }
}

fn map_get<'a>(map: &Map<'a>, key: u64) -> Option<Item<'a>> {
    map.iter()
        .find_map(|pair| pair.ok().filter(|(k, _)| matches!(k, Item::Unsigned(v) if *v == key)))
        .map(|(_, v)| v)
}

fn run() -> bool {
    let mut console = Console::<TockSyscalls>::writer();
    let selector = wildcard_selector();
    let mut discovered: [ServiceIdentifier; 4] = core::array::from_fn(|_| empty_id());
    let found = match service_discovery(&selector, &mut discovered) {
        Ok(n) => n,
        Err(e) => {
            let _ = writeln!(console, "debug: service_discovery err {:?}", e);
            return false;
        }
    };
    let _ = writeln!(console, "debug: service_discovery found {}", found);
    if found != 1 || discovered[0].service_id.bytes != GPP_ROT13_SERVICE_NAME {
        let _ = writeln!(console, "debug: unexpected discovery result");
        return false;
    }
    let rot13 = &discovered[0];

    let mut session = Session {
        service_id: &rot13.service_instance,
        session_id: 0,
        imp: SessionPriv::new(0),
    };
    if let Err(e) = open_session(&rot13.service_instance, LOGIN_PUBLIC, None, &mut session) {
        let _ = writeln!(console, "debug: open_session err {:?}", e);
        return false;
    }
    let _ = writeln!(console, "debug: open_session ok");

    // `MessageBuf::drop` zeroes its entire backing buffer (mirrors the spec's
    // `TPSC_FinalizeTransaction`), so a reply's contents must be read/copied out while its
    // `MessageBuf` is still in scope — not after the block that created it has ended.
    let mut ciphertext_owned = [0u8; 128];
    let ciphertext_len = {
        let mut send_storage = [0u8; 128];
        let send_len = build_req(
            &mut send_storage,
            GPP_ROT13_ENCRYPT_REQ,
            GPP_ROT13_PLAINTEXT_KEY,
            PLAINTEXT,
        );
        let mut send_buf = MessageBuf::new(&mut send_storage);
        send_buf.size = send_len;
        let mut recv_storage = [0u8; 128];
        let mut recv_buf = MessageBuf::new(&mut recv_storage);
        if let Err(e) = execute_transaction(&session, &send_buf, &mut recv_buf) {
            let _ = writeln!(console, "debug: execute_transaction(encrypt) err {:?}", e);
            let _ = close_session(&session);
            return false;
        }
        let received = unsafe { core::slice::from_raw_parts(recv_buf.message, recv_buf.size) };
        match decode_rsp(received, GPP_ROT13_ENCRYPT_RSP)
            .and_then(|map| map_get(&map, GPP_ROT13_CIPHERTEXT_KEY as u64))
        {
            Some(Item::Text(text)) => {
                let _ = writeln!(console, "debug: ciphertext = {}", text);
                let len = text.len();
                ciphertext_owned[..len].copy_from_slice(text.as_bytes());
                len
            }
            _ => {
                let _ = writeln!(console, "debug: could not decode encrypt response");
                let _ = close_session(&session);
                return false;
            }
        }
    };
    let ciphertext_text = match core::str::from_utf8(&ciphertext_owned[..ciphertext_len]) {
        Ok(s) => s,
        Err(_) => {
            let _ = writeln!(console, "debug: ciphertext not valid utf8");
            let _ = close_session(&session);
            return false;
        }
    };

    let ciphertext_ok = {
        let mut send_storage = [0u8; 128];
        let send_len = build_req(
            &mut send_storage,
            GPP_ROT13_DECRYPT_REQ,
            GPP_ROT13_PLAINTEXT_KEY,
            ciphertext_text,
        );
        let mut send_buf = MessageBuf::new(&mut send_storage);
        send_buf.size = send_len;
        let mut roundtrip_storage = [0u8; 128];
        let mut recv_buf = MessageBuf::new(&mut roundtrip_storage);
        if let Err(e) = execute_transaction(&session, &send_buf, &mut recv_buf) {
            let _ = writeln!(console, "debug: execute_transaction(decrypt) err {:?}", e);
            false
        } else {
            let received = unsafe { core::slice::from_raw_parts(recv_buf.message, recv_buf.size) };
            let ok = matches!(
                decode_rsp(received, GPP_ROT13_DECRYPT_RSP)
                    .and_then(|map| map_get(&map, GPP_ROT13_CIPHERTEXT_KEY as u64)),
                Some(Item::Text(decoded)) if decoded == PLAINTEXT
            );
            if !ok {
                let _ = writeln!(console, "debug: decrypt roundtrip mismatch");
            }
            ok
        }
    };

    let _ = close_session(&session);
    ciphertext_ok
}

fn main() {
    let mut console = Console::<TockSyscalls>::writer();
    if run() {
        let _ = writeln!(console, "tock_rot13_client: PASS");
    } else {
        let _ = writeln!(console, "tock_rot13_client: FAIL");
    }
}
