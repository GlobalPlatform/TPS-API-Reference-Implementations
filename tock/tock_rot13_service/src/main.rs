/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Tock userspace service app: wraps `rot13_service::message_handler` over
//! Tock IPC via `tock_ipc`.
#![no_main]
#![no_std]

extern crate libtock_debug_panic;

use core::cell::Cell;
use core::fmt::Write;

use libtock_console::Console;
use libtock_platform::Syscalls;
use libtock_runtime::{set_main, stack_size, TockSyscalls};
use tock_ipc::ServiceId;

set_main! {main}
stack_size! {0x1000}

/// This process's own package name, as compiled into its TBF header — must match the literal
/// name `tock_ipc::discover` looks up, and the name `tock_connector::service` discovers this
/// process by (see `ROT13_SERVICE_PROCESS_NAME` there).
const OWN_PROCESS_NAME: &[u8] = b"tock_rot13_service";

/// Matches `tock_connector::service::IPC_BUFFER_SIZE` — the single shared IPC buffer's capacity
/// is set by the client (via `allow_rw`), so this is just this side's local scratch capacity for
/// decoupling the incoming request from `message_handler`'s outgoing write (see `main`'s loop).
const SCRATCH_SIZE: usize = 128;

fn main() {
    let mut console = Console::<TockSyscalls>::writer();

    let own = match tock_ipc::discover::<TockSyscalls>(OWN_PROCESS_NAME) {
        Ok(id) => id,
        Err(_) => {
            let _ = writeln!(console, "tock_rot13_service: discover(self) failed");
            return;
        }
    };

    let received: Cell<Option<tock_ipc::ServiceUpcallArgs>> = Cell::new(None);
    // Safety: `received` is a stack local in this frame, which never returns while the
    // subscription is active — this app subscribes once and loops forever.
    if unsafe { tock_ipc::subscribe_service::<TockSyscalls>(own, &received) }.is_err() {
        let _ = writeln!(console, "tock_rot13_service: subscribe_service failed");
        return;
    }

    let _ = writeln!(console, "tock_rot13_service: ready");

    let mut scratch: [u8; SCRATCH_SIZE] = [0; SCRATCH_SIZE];
    loop {
        TockSyscalls::yield_wait();
        let Some((client, len, ptr)) = received.take() else {
            continue;
        };

        // Safety: `len`/`ptr` are exactly the arguments this process's own `subscribe_service`
        // upcall was just invoked with, and that upcall invocation has not yet returned (we're
        // still inside the same `yield_wait` that delivered it).
        let out_len = unsafe {
            tock_ipc::with_shared_buffer(len, ptr, |buf| {
                // Tock IPC's shared buffer carries no out-of-band length (it's the client's
                // full `allow_rw`'d buffer, zero-padded past the actual request) — recover the
                // real request length the same way `tock_connector::service::execute_transaction`
                // recovers the reply length: CBOR's top-level item is self-delimiting, so decode
                // it once and read back how many bytes that consumed.
                let mut decoder = tps_minicbor::Decoder::new(buf);
                let in_len = match decoder.next() {
                    Ok(_) => decoder.pos(),
                    Err(_) => return None,
                };
                let in_len = in_len.min(SCRATCH_SIZE);
                scratch[..in_len].copy_from_slice(&buf[..in_len]);
                rot13_service::message_handler(&scratch[..in_len], buf).ok()
            })
        };

        if out_len.is_none() {
            let _ = writeln!(console, "tock_rot13_service: message_handler failed");
        }
        let _ = tock_ipc::notify_client::<TockSyscalls>(ServiceId::from_raw(client));
    }
}
