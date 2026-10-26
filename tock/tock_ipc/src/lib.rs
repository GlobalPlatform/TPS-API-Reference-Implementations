/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![no_std]
//! Minimal Rust wrapper for Tock's IPC syscall driver (`0x10000`), generic
//! over [`libtock_platform::Syscalls`].
//!
//! This is deliberately not a general-purpose IPC library: it exposes just
//! enough of Tock's `kernel::ipc` capsule (discovery, a single shared
//! read-write buffer per peer, and service/client notify) to support one
//! client talking to one discovered service, matching the semantics of the
//! upstream `libtock-c` `ipc.h`/`ipc.c` example this is ported from.
//!
//! Tock's IPC driver keys both the shared buffer and the upcall slot a
//! process listens on by the *peer's process index* (a value only known at
//! runtime, returned by [`discover`]). `libtock_platform::Syscalls::allow_rw`
//! and `::subscribe` require that slot number as a `const` generic, which
//! can't be satisfied by a runtime value, so the peer-indexed operations here
//! are built directly on `RawSyscalls` instead of going through `Syscalls`'s
//! safe allow/subscribe wrappers.

use core::cell::Cell;
use core::convert::TryFrom;
use libtock_platform::share;
use libtock_platform::subscribe::{AnyId, Upcall};
use libtock_platform::{
    allow_ro::AllowRo, syscall_class, DefaultConfig, ErrorCode, Register, ReturnVariant, Syscalls,
};

/// IPC's syscall driver number.
pub const DRIVER_NUM: u32 = 0x10000;

mod command {
    pub const DISCOVER: u32 = 1;
    pub const NOTIFY_SERVICE: u32 = 2;
    pub const NOTIFY_CLIENT: u32 = 3;
}

mod ro_allow {
    pub const SEARCH: u32 = 0;
}

/// The upcall arguments delivered to a service on each client notify:
/// `(client, len, ptr)` of the client's shared buffer.
pub type ServiceUpcallArgs = (u32, u32, u32);

/// A process discovered via [`discover`]. This wraps that process's index,
/// which Tock's IPC driver also uses as the shared-buffer number and upcall
/// subscribe number for talking to that process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ServiceId(u32);

impl ServiceId {
    pub fn as_raw(self) -> u32 {
        self.0
    }

    /// Reconstructs a [`ServiceId`] previously obtained from [`ServiceId::as_raw`], e.g. after
    /// round-tripping it through storage (such as an `AtomicU32`) that can't hold `ServiceId`
    /// itself.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

/// Looks up a running process by its exact package name (as compiled into
/// its TBF header) and returns an id usable with the rest of this crate's
/// functions.
pub fn discover<S: Syscalls>(name: &[u8]) -> Result<ServiceId, ErrorCode> {
    share::scope::<AllowRo<_, DRIVER_NUM, { ro_allow::SEARCH }>, _, _>(|handle| {
        S::allow_ro::<DefaultConfig, DRIVER_NUM, { ro_allow::SEARCH }>(handle, name)?;
        S::command(DRIVER_NUM, command::DISCOVER, 0, 0)
            .to_result::<u32, ErrorCode>()
            .map(ServiceId)
    })
}

/// Client round trip: shares `buf` with `service` (Tock IPC's one
/// read-write buffer per peer), notifies it, and blocks until it notifies
/// back. `buf` is overwritten in place by the service — Tock IPC has no
/// separate reply buffer or out-of-band length, so the caller must be able
/// to tell how much of `buf` is meaningful on return (e.g. because the
/// payload is self-delimiting).
pub fn call<S: Syscalls>(service: ServiceId, buf: &mut [u8]) -> Result<(), ErrorCode> {
    let called: Cell<Option<ServiceUpcallArgs>> = Cell::new(None);

    // Safety: `buf` and `called` both outlive this function's body, and both
    // are unallowed/unsubscribed (via the peer's fixed slot number,
    // `service.as_raw()`) on every exit path below before this function
    // returns, so neither the kernel nor the upcall trampoline can observe
    // them after they become invalid.
    unsafe { raw_allow_rw::<S>(DRIVER_NUM, service.as_raw(), buf) }?;
    let subscribe_result = unsafe { raw_subscribe::<S, _>(DRIVER_NUM, service.as_raw(), &called) };
    if let Err(e) = subscribe_result {
        S::unallow_rw(DRIVER_NUM, service.as_raw());
        return Err(e);
    }

    let notify_result =
        S::command(DRIVER_NUM, command::NOTIFY_SERVICE, service.as_raw(), 0).to_result::<(), ErrorCode>();
    if let Err(e) = notify_result {
        S::unsubscribe(DRIVER_NUM, service.as_raw());
        S::unallow_rw(DRIVER_NUM, service.as_raw());
        return Err(e);
    }

    while called.get().is_none() {
        S::yield_wait();
    }

    S::unsubscribe(DRIVER_NUM, service.as_raw());
    S::unallow_rw(DRIVER_NUM, service.as_raw());
    Ok(())
}

/// Service side: registers `upcall` to be invoked on every client notify.
/// `upcall` receives `(client, len, ptr)` — see [`with_shared_buffer`] to turn
/// `(len, ptr)` into a slice.
///
/// # Safety
/// `own` must be this process's own [`discover`]ed id (found by discovering
/// its own package name), since the kernel schedules service notifies on the
/// receiving process's own index. `upcall` must remain valid for as long as
/// this subscription is active, i.e. until [`unsubscribe_service`] is called
/// with the same `own`. This reference implementation's service apps never
/// call `unsubscribe_service` — they subscribe once from a `main` that never
/// returns, so `upcall` (a stack local in that same, live frame) is valid for
/// the process's entire lifetime.
pub unsafe fn subscribe_service<S: Syscalls>(
    own: ServiceId,
    upcall: &Cell<Option<ServiceUpcallArgs>>,
) -> Result<(), ErrorCode> {
    unsafe { raw_subscribe::<S, _>(DRIVER_NUM, own.as_raw(), upcall) }
}

/// Undoes [`subscribe_service`].
pub fn unsubscribe_service<S: Syscalls>(own: ServiceId) {
    S::unsubscribe(DRIVER_NUM, own.as_raw());
}

/// Notifies a client, typically in response to a notify it sent this
/// process (see [`subscribe_service`]'s upcall).
pub fn notify_client<S: Syscalls>(client: ServiceId) -> Result<(), ErrorCode> {
    S::command(DRIVER_NUM, command::NOTIFY_CLIENT, client.as_raw(), 0).to_result::<(), ErrorCode>()
}

/// Reconstructs the client's shared buffer from the `(len, ptr)` a service's
/// notify upcall was called with, for the duration of `f`.
///
/// # Safety
/// `len`/`ptr` must be the second and third arguments of an
/// [`subscribe_service`] upcall invocation that has not yet returned (the
/// kernel only guarantees the referenced memory is mapped for the duration
/// of that upcall).
pub unsafe fn with_shared_buffer<R>(len: u32, ptr: u32, f: impl FnOnce(&mut [u8]) -> R) -> R {
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len as usize) };
    f(slice)
}

// -----------------------------------------------------------------------------
// Raw (runtime buffer/subscribe number) syscall wrappers.
//
// These replicate the syscall sequencing of `libtock_platform`'s own
// `Syscalls::allow_rw`/`::subscribe` (TRD 104's Read-Write Allow and
// Subscribe classes), but take the buffer/subscribe number as a runtime
// `u32` instead of a `const` generic, since IPC's peer-indexed slots aren't
// known until `discover` returns. Teardown reuses `Syscalls::unallow_rw`/
// `::unsubscribe` directly, since those are already runtime-parameterized in
// the public trait.
// -----------------------------------------------------------------------------

/// # Safety
/// Same contract as `Syscalls::allow_rw`: `buffer` must remain valid and
/// must not be accessed by this process again until `S::unallow_rw(driver_num,
/// buffer_num)` is called.
unsafe fn raw_allow_rw<S: Syscalls>(
    driver_num: u32,
    buffer_num: u32,
    buffer: &mut [u8],
) -> Result<(), ErrorCode> {
    let [r0, r1, _, _] = unsafe {
        S::syscall4::<{ syscall_class::ALLOW_RW }>([
            driver_num.into(),
            buffer_num.into(),
            buffer.as_mut_ptr().into(),
            buffer.len().into(),
        ])
    };
    check_failure_2_u32(r0, r1)
}

/// # Safety
/// Same contract as `Syscalls::subscribe`: `upcall` must remain valid until
/// `S::unsubscribe(driver_num, subscribe_num)` is called.
unsafe fn raw_subscribe<S: Syscalls, U: Upcall<AnyId>>(
    driver_num: u32,
    subscribe_num: u32,
    upcall: &U,
) -> Result<(), ErrorCode> {
    // Safety: `data` must be a reference to a valid instance of `U`, which
    // holds as long as `upcall` (passed as `upcall_data` below) does.
    unsafe extern "C" fn trampoline<S: Syscalls, U: Upcall<AnyId>>(
        arg0: u32,
        arg1: u32,
        arg2: u32,
        data: Register,
    ) {
        let exit: libtock_platform::exit_on_drop::ExitOnDrop<S> = Default::default();
        let upcall: *const U = data.into();
        unsafe { &*upcall }.upcall(arg0, arg1, arg2);
        core::mem::forget(exit);
    }

    let upcall_fcn: Register = (trampoline::<S, U> as *const ()).into();
    let upcall_data: Register = (upcall as *const U).into();
    let [r0, r1, _, _] = unsafe {
        S::syscall4::<{ syscall_class::SUBSCRIBE }>([
            driver_num.into(),
            subscribe_num.into(),
            upcall_fcn,
            upcall_data,
        ])
    };
    check_failure_2_u32(r0, r1)
}

fn check_failure_2_u32(r0: Register, r1: Register) -> Result<(), ErrorCode> {
    let variant: ReturnVariant = r0.as_u32().into();
    if variant == libtock_platform::return_variant::FAILURE_2_U32 {
        return Err(ErrorCode::try_from(r1.as_u32()).unwrap_or(ErrorCode::Fail));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
